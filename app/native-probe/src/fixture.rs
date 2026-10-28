//! Synthetic loopback desktop for exercising the native client without a device.
use crate::rfb::{connect, Frame};
use anyhow::{ensure, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
};
use vnc::X11Event;

pub(crate) async fn handshake(socket: &mut TcpStream, width: u16, height: u16) -> Result<Vec<i32>> {
    socket.write_all(b"RFB 003.008\n").await?;
    let mut version = [0; 12];
    socket.read_exact(&mut version).await?;
    ensure!(&version == b"RFB 003.008\n", "RFB version");
    socket.write_all(&[1, 1]).await?;
    ensure!(socket.read_u8().await? == 1, "security selection");
    socket.write_u32(0).await?;
    socket.read_u8().await?;
    socket.write_u16(width).await?;
    socket.write_u16(height).await?;
    socket
        .write_all(&[32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 0, 8, 16, 0, 0, 0])
        .await?;
    socket.write_u32(7).await?;
    socket.write_all(b"fixture").await?;
    ensure!(socket.read_u8().await? == 0, "pixel format message");
    let mut format = [0; 19];
    socket.read_exact(&mut format).await?;
    ensure!(socket.read_u8().await? == 2, "encodings message");
    socket.read_u8().await?;
    let count = socket.read_u16().await?;
    let mut encodings = Vec::new();
    for _ in 0..count {
        encodings.push(socket.read_i32().await?);
    }
    Ok(encodings)
}

async fn server(listener: TcpListener) -> Result<()> {
    let (mut socket, _) = listener.accept().await?;
    handshake(&mut socket, 320, 180).await?;
    let mut pointer = (160usize, 90usize);
    let mut count = 0u8;
    loop {
        let kind = match socket.read_u8().await {
            Ok(kind) => kind,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        match kind {
            3 => {
                let mut request = [0; 9];
                socket.read_exact(&mut request).await?;
                socket.write_all(&[0, 0, 0, 1]).await?;
                for n in [0, 0, 320, 180] {
                    socket.write_u16(n).await?;
                }
                socket.write_i32(0).await?;
                let mut pixels = Vec::with_capacity(320 * 180 * 4);
                for y in 0..180usize {
                    for x in 0..320usize {
                        let target = x.abs_diff(pointer.0) < 12 && y.abs_diff(pointer.1) < 12;
                        let pixel = if target {
                            [230, 170, 70, 255]
                        } else {
                            [25 + (x / 3) as u8, 45 + (y / 2) as u8, 110 + count, 255]
                        };
                        pixels.extend_from_slice(&pixel);
                    }
                }
                socket.write_all(&pixels).await?;
            }
            4 => {
                let mut key = [0; 7];
                socket.read_exact(&mut key).await?;
                if key[0] == 1 {
                    count = count.wrapping_add(1) % 100;
                }
            }
            5 => {
                let mut mouse = [0; 5];
                socket.read_exact(&mut mouse).await?;
                pointer = (
                    u16::from_be_bytes([mouse[1], mouse[2]]) as usize,
                    u16::from_be_bytes([mouse[3], mouse[4]]) as usize,
                );
            }
            6 => {
                let mut header = [0; 7];
                socket.read_exact(&mut header).await?;
                let length = u32::from_be_bytes(header[3..7].try_into()?) as usize;
                ensure!(length <= 1024 * 1024, "fixture clipboard too large");
                let mut text = vec![0; length];
                socket.read_exact(&mut text).await?;
                socket.write_all(&[3, 0, 0, 0]).await?;
                socket.write_u32(length as u32).await?;
                socket.write_all(&text).await?;
            }
            _ => anyhow::bail!("unexpected fixture client message {kind}"),
        }
    }
}

pub async fn run_desktop(
    frames: watch::Sender<Frame>,
    mut inputs: mpsc::Receiver<X11Event>,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(server(listener));
    let client = connect(TcpStream::connect(address).await?).await?;
    let mut frame = Frame::default();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(33));
    let result=async {
        loop {
            if *stop.borrow() {break;}
            while let Some(event)=client.poll_event().await? {
                if frame.apply(event)? {frames.send_replace(frame.clone());}
            }
            tokio::select! {
                _=stop.changed()=>break,
                _=tick.tick()=>{client.input(X11Event::Refresh).await?;},
                input=inputs.recv()=>match input {Some(input)=>client.input(input).await?,None=>break},
            }
        }
        Ok::<_,anyhow::Error>(())
    }.await;
    let closed = client.close().await;
    let ended = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
    result?;
    closed?;
    if let Err(error) = ended?? {
        // Closing our client may reset a fixture that is writing its final frame.
        let expected = error.downcast_ref::<std::io::Error>().is_some_and(|e| {
            matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
            )
        });
        if !expected {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn loopback_desktop_stops_and_a_new_session_receives_frames() -> Result<()> {
        for _ in 0..2 {
            let (sender, mut frames) = watch::channel(Frame::default());
            let (_input, inputs) = mpsc::channel(8);
            let (stop, stopped) = watch::channel(false);
            let job = tokio::spawn(run_desktop(sender, inputs, stopped));
            tokio::time::timeout(std::time::Duration::from_secs(3), async {
                loop {
                    frames.changed().await?;
                    if frames.borrow().pixels.iter().any(|&n| n != 0) {
                        break;
                    }
                }
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            stop.send_replace(true);
            tokio::time::timeout(std::time::Duration::from_secs(3), job).await???;
        }
        Ok(())
    }
}

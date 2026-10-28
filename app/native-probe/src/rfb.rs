//! Controlled loopback RFB probe. No device credentials or external listener.
use anyhow::{bail, ensure, Result};
use vnc::{PixelFormat, Rect, VncClient, VncConnector, VncEncoding, VncEvent};

/// Translate Slint's logical key values into X11 keysyms; Unicode uses the RFB Unicode prefix.
pub fn keysym(key: char) -> u32 {
    match key {
        '\u{8}' => 0xff08,
        '\t' => 0xff09,
        '\n' | '\r' => 0xff0d,
        '\u{1b}' => 0xff1b,
        '\u{7f}' => 0xffff,
        '\u{10}' => 0xffe1,
        '\u{11}' => 0xffe3,
        '\u{12}' => 0xffe9,
        '\u{13}' => 0xfe03,
        '\u{14}' => 0xffe5,
        '\u{15}' => 0xffe2,
        '\u{16}' => 0xffe4,
        '\u{f700}' => 0xff52,
        '\u{f701}' => 0xff54,
        '\u{f702}' => 0xff51,
        '\u{f703}' => 0xff53,
        '\u{f704}'..='\u{f726}' => 0xffbe + (key as u32 - 0xf704),
        '\u{f727}' => 0xff63,
        '\u{f729}' => 0xff50,
        '\u{f72b}' => 0xff57,
        '\u{f72c}' => 0xff55,
        '\u{f72d}' => 0xff56,
        c if c as u32 <= 255 => c as u32,
        c => 0x01000000 | c as u32,
    }
}

#[derive(Default, Clone)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
    pub clipboard: String,
}
impl Frame {
    fn region(&self, rect: Rect) -> Result<()> {
        ensure!(
            u32::from(rect.x) + u32::from(rect.width) <= u32::from(self.width)
                && u32::from(rect.y) + u32::from(rect.height) <= u32::from(self.height),
            "RFB rectangle outside framebuffer"
        );
        Ok(())
    }
    fn draw(&mut self, rect: Rect, data: &[u8]) -> Result<()> {
        self.region(rect)?;
        let stride = usize::from(rect.width) * 4;
        ensure!(
            data.len() == stride * usize::from(rect.height),
            "RFB pixel length mismatch"
        );
        for y in 0..usize::from(rect.height) {
            let target =
                ((usize::from(rect.y) + y) * usize::from(self.width) + usize::from(rect.x)) * 4;
            self.pixels[target..target + stride]
                .copy_from_slice(&data[y * stride..(y + 1) * stride]);
            for pixel in self.pixels[target..target + stride].chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
        Ok(())
    }
    pub fn apply(&mut self, event: VncEvent) -> Result<bool> {
        match event {
            VncEvent::SetResolution(size) => {
                let length = usize::from(size.width) * usize::from(size.height) * 4;
                ensure!(
                    length > 0 && length <= 128 * 1024 * 1024,
                    "RFB framebuffer size unsupported"
                );
                if (self.width, self.height) != (size.width, size.height) {
                    self.width = size.width;
                    self.height = size.height;
                    self.pixels = vec![0; length];
                    return Ok(true);
                }
                Ok(false)
            }
            VncEvent::RawImage(rect, data) => {
                self.draw(rect, &data)?;
                Ok(true)
            }
            VncEvent::Copy(destination, source) => {
                self.region(source)?;
                self.region(destination)?;
                ensure!(
                    (source.width, source.height) == (destination.width, destination.height),
                    "RFB CopyRect size mismatch"
                );
                let mut pixels =
                    Vec::with_capacity(usize::from(source.width) * usize::from(source.height) * 4);
                for y in 0..usize::from(source.height) {
                    let start = ((usize::from(source.y) + y) * usize::from(self.width)
                        + usize::from(source.x))
                        * 4;
                    pixels.extend_from_slice(
                        &self.pixels[start..start + usize::from(source.width) * 4],
                    );
                }
                self.draw(destination, &pixels)?;
                Ok(true)
            }
            VncEvent::JpegImage(rect, data) => {
                self.region(rect)?;
                let mut reader = image::ImageReader::with_format(
                    std::io::Cursor::new(data),
                    image::ImageFormat::Jpeg,
                );
                let mut limits = image::Limits::default();
                limits.max_alloc = Some(128 * 1024 * 1024);
                limits.max_image_width = Some(u32::from(rect.width));
                limits.max_image_height = Some(u32::from(rect.height));
                reader.limits(limits);
                let decoded = reader.decode()?.into_rgba8();
                ensure!(
                    decoded.dimensions() == (u32::from(rect.width), u32::from(rect.height)),
                    "RFB JPEG dimensions mismatch"
                );
                self.draw(rect, &decoded)?;
                Ok(true)
            }
            VncEvent::Text(text) => {
                self.clipboard = text;
                Ok(false)
            }
            VncEvent::Error(error) => bail!("RFB protocol: {error}"),
            _ => Ok(false),
        }
    }
}

/// Stream may later be a verified SSH channel. The probe connects only its own loopback fixture.
pub async fn connect(
    stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
) -> Result<VncClient> {
    Ok(VncConnector::new(stream)
        .set_auth_method(async { Ok(String::new()) })
        .set_pixel_format(PixelFormat::rgba())
        .allow_shared(true)
        .add_encoding(VncEncoding::Tight)
        .add_encoding(VncEncoding::Zrle)
        .add_encoding(VncEncoding::CopyRect)
        .add_encoding(VncEncoding::Raw)
        .build()?
        .try_start()
        .await?
        .finish()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, time::Duration};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };
    #[test]
    fn logical_keys_match_x11_including_modifiers_and_unicode() {
        assert_eq!(keysym('\u{f700}'), 0xff52);
        assert_eq!(keysym('\n'), 0xff0d);
        assert_eq!(keysym('\u{11}'), 0xffe3);
        assert_eq!(keysym('中'), 0x01004e2d);
        assert_eq!(keysym('A'), 65);
    }

    async fn handshake(stream: &mut TcpStream) -> Result<Vec<i32>> {
        stream.write_all(b"RFB 003.008\n").await?;
        let mut version = [0; 12];
        stream.read_exact(&mut version).await?;
        ensure!(&version == b"RFB 003.008\n", "RFB version");
        stream.write_all(&[1, 1]).await?;
        ensure!(stream.read_u8().await? == 1, "security selection");
        stream.write_u32(0).await?;
        stream.read_u8().await?;
        stream.write_u16(4).await?;
        stream.write_u16(2).await?;
        stream
            .write_all(&[32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 0, 8, 16, 0, 0, 0])
            .await?;
        stream.write_u32(7).await?;
        stream.write_all(b"fixture").await?;
        // SetPixelFormat and SetEncodings are transmitted before the first refresh.
        ensure!(stream.read_u8().await? == 0, "pixel format message");
        let mut format = [0; 19];
        stream.read_exact(&mut format).await?;
        ensure!(stream.read_u8().await? == 2, "encodings message");
        stream.read_u8().await?;
        let count = stream.read_u16().await?;
        let mut encodings = Vec::new();
        for _ in 0..count {
            encodings.push(stream.read_i32().await?);
        }
        Ok(encodings)
    }
    async fn update(
        stream: &mut TcpStream,
        x: u16,
        width: u16,
        encoding: i32,
        payload: &[u8],
    ) -> Result<()> {
        stream.write_all(&[0, 0, 0, 1]).await?;
        for n in [x, 0, width, 2] {
            stream.write_u16(n).await?;
        }
        stream.write_i32(encoding).await?;
        stream.write_all(payload).await?;
        Ok(())
    }
    async fn next_frame(client: &VncClient, frame: &mut Frame) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(event) = client.poll_event().await? {
                    let changed = matches!(
                        event,
                        VncEvent::RawImage(..) | VncEvent::JpegImage(..) | VncEvent::Copy(..)
                    );
                    frame.apply(event)?;
                    if changed {
                        return Ok::<_, anyhow::Error>(());
                    }
                } else {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        })
        .await??;
        Ok(())
    }
    async fn encoding_case(encoding: i32) -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let encodings = handshake(&mut socket).await?;
            ensure!(encodings.contains(&encoding), "encoding not negotiated");
            let mut request = [0; 10];
            socket.read_exact(&mut request).await?;
            ensure!(request[0] == 3, "refresh expected");
            let rgb = [40, 120, 200];
            let payload = match encoding {
                0 => [40, 120, 200, 0].repeat(8),
                7 => vec![0x80, rgb[0], rgb[1], rgb[2]], // Tight fill; JPEG handled in a separate test.
                16 => {
                    let mut encoder =
                        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    encoder.write_all(&[1, rgb[0], rgb[1], rgb[2]])?; // ZRLE solid tile
                    encoder.flush()?; // RFB retains the zlib stream between rectangles.
                    let compressed = encoder.get_ref().clone();
                    let mut data = (compressed.len() as u32).to_be_bytes().to_vec();
                    data.extend(compressed);
                    data
                }
                _ => unreachable!(),
            };
            update(&mut socket, 0, 4, encoding, &payload).await?;
            let mut key = [0; 8];
            socket.read_exact(&mut key).await?;
            ensure!(
                key == [4, 1, 0, 0, 0, 0, 0, 65],
                "keyboard input did not arrive"
            );
            let mut pointer = [0; 6];
            socket.read_exact(&mut pointer).await?;
            ensure!(
                pointer == [5, 1, 0, 2, 0, 1],
                "pointer input did not arrive"
            );
            let mut clipboard = [0; 8];
            socket.read_exact(&mut clipboard).await?;
            ensure!(clipboard[0] == 6, "clipboard message missing");
            let mut text = vec![0; u32::from_be_bytes(clipboard[4..8].try_into()?) as usize];
            socket.read_exact(&mut text).await?;
            ensure!(text == b"copy", "clipboard payload");
            socket.read_exact(&mut request).await?;
            ensure!(request[1] == 1, "incremental refresh expected");
            update(&mut socket, 2, 2, 1, &[0, 0, 0, 0]).await?; // overlapping copy from left half
            let mut end = [0; 1];
            ensure!(
                socket.read(&mut end).await? == 0,
                "client socket not closed"
            );
            Ok::<_, anyhow::Error>(())
        });
        let client = connect(TcpStream::connect(address).await?).await?;
        let mut frame = Frame::default();
        next_frame(&client, &mut frame).await?;
        assert_eq!((frame.width, frame.height), (4, 2));
        assert!(frame
            .pixels
            .chunks_exact(4)
            .all(|p| p == [40, 120, 200, 255]));
        client
            .input(vnc::X11Event::KeyEvent((65, true).into()))
            .await?;
        client
            .input(vnc::X11Event::PointerEvent((2, 1, 1).into()))
            .await?;
        client.input(vnc::X11Event::CopyText("copy".into())).await?;
        client.input(vnc::X11Event::Refresh).await?;
        next_frame(&client, &mut frame).await?;
        client.close().await?;
        tokio::time::timeout(Duration::from_secs(3), server).await???;
        Ok(())
    }
    #[tokio::test]
    async fn raw_frame_copyrect_keyboard_pointer_clipboard_and_close() -> Result<()> {
        encoding_case(0).await
    }
    #[tokio::test]
    async fn tight_fill_negotiation_and_incremental_update() -> Result<()> {
        encoding_case(7).await
    }
    #[tokio::test]
    async fn zrle_negotiation_and_incremental_update() -> Result<()> {
        encoding_case(16).await
    }
    #[test]
    fn jpeg_decodes_without_a_browser_and_bad_rectangles_are_rejected() -> Result<()> {
        let mut frame = Frame::default();
        frame.apply(VncEvent::SetResolution((4, 2).into()))?;
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95).encode(
            &[200, 40, 20].repeat(8),
            4,
            2,
            image::ExtendedColorType::Rgb8,
        )?;
        let rect = Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
        };
        frame.apply(VncEvent::JpegImage(rect, jpeg))?;
        assert!(frame.pixels[0] > 180 && frame.pixels[1] < 60);
        assert!(frame
            .apply(VncEvent::RawImage(Rect { x: 3, ..rect }, vec![0; 32]))
            .is_err());
        assert!(frame.apply(VncEvent::RawImage(rect, vec![0; 3])).is_err());
        Ok(())
    }
}

//! A synthetic RFB peer. It never invokes a shell or changes a display.
use anyhow::{ensure, Result};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Clone, Copy, Default)]
#[allow(dead_code)] // The GUI examples use Normal; integration tests also exercise failure modes.
pub enum Behavior {
    #[default]
    Normal,
    NoFirstFrame,
    PartialFirstFrame,
    ExtraAuthentication,
}
#[derive(Default)]
pub struct Counts {
    pub active_processes: AtomicUsize,
    pub process_eof: AtomicUsize,
    pub active_streams: AtomicUsize,
    pub streams_closed: AtomicUsize,
    pub configs: Mutex<Vec<serde_json::Value>>,
    pub encodings: Mutex<Vec<Vec<i32>>>,
    pub requests: AtomicUsize,
    pub full_requests: AtomicUsize,
    pub keys: Mutex<Vec<(u32, bool)>>,
    pub held: Mutex<BTreeSet<u32>>,
    pub pointers: Mutex<Vec<(u16, u16, u8)>>,
    pub clipboard: Mutex<Vec<Vec<u8>>>,
    pub resizes: AtomicUsize,
}
impl Counts {
    pub fn report(&self) -> serde_json::Value {
        let pointers = self.pointers.lock().unwrap();
        let pointer_count = pointers.len();
        let last_pointer = pointers.last().copied();
        drop(pointers);
        let keys=self.keys.lock().unwrap();
        let key_count=keys.len();
        let ascii=keys.iter().filter(|(k,down)|*down&&(32..127).contains(k)).count();
        let unicode=keys.iter().filter(|(k,down)|*down&&*k>=0x01000000).count();
        let supplementary=keys.iter().filter(|(k,down)|*down&&*k>=0x01010000).count();
        drop(keys);
        serde_json::json!({"activeProcesses":self.active_processes.load(Ordering::SeqCst),"processEof":self.process_eof.load(Ordering::SeqCst),"activeStreams":self.active_streams.load(Ordering::SeqCst),"closedStreams":self.streams_closed.load(Ordering::SeqCst),"configs":*self.configs.lock().unwrap(),"encodings":*self.encodings.lock().unwrap(),"requests":self.requests.load(Ordering::SeqCst),"fullRequests":self.full_requests.load(Ordering::SeqCst),"keyEvents":key_count,"asciiKeyDowns":ascii,"unicodeKeyDowns":unicode,"supplementaryKeyDowns":supplementary,"heldKeys":self.held.lock().unwrap().len(),"pointerEvents":pointer_count,"lastPointer":last_pointer,"clipboardBytes":self.clipboard.lock().unwrap().iter().map(Vec::len).collect::<Vec<_>>(),"resizes":self.resizes.load(Ordering::SeqCst)})
    }
}
struct Active<'a>(&'a Counts);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active_streams.fetch_sub(1, Ordering::SeqCst);
        self.0.streams_closed.fetch_add(1, Ordering::SeqCst);
    }
}
async fn header<S: AsyncWrite + Unpin>(
    s: &mut S,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    encoding: i32,
) -> Result<()> {
    for n in [x, y, w, h] {
        s.write_u16(n).await?;
    }
    s.write_i32(encoding).await?;
    Ok(())
}
async fn update<S: AsyncWrite + Unpin>(
    s: &mut S,
    size: (u16, u16),
    pointer: (u16, u16, u8),
    keys: usize,
    partial: bool,
) -> Result<()> {
    let (w, h) = size;
    let width = if partial { w / 2 } else { w };
    s.write_all(&[0, 0, 0, 1]).await?;
    header(s, 0, 0, width, h, 0).await?;
    let mut pixels = Vec::with_capacity(usize::from(width) * usize::from(h) * 4);
    for y in 0..h {
        for x in 0..width {
            let target = x.abs_diff(pointer.0) < 9 && y.abs_diff(pointer.1) < 9;
            pixels.extend_from_slice(&if target {
                [240, 180, 70, 0]
            } else {
                [
                    25 + (x % 180) as u8,
                    40 + (y % 150) as u8,
                    90 + (keys % 100) as u8,
                    0,
                ]
            });
        }
    }
    s.write_all(&pixels).await?;
    Ok(())
}
async fn cursor<S: AsyncWrite + Unpin>(s: &mut S) -> Result<()> {
    s.write_all(&[0, 0, 0, 1]).await?;
    header(s, 1, 1, 8, 8, -239).await?;
    s.write_all(&[255, 255, 255, 0].repeat(64)).await?;
    s.write_all(&[0x80, 0xc0, 0xe0, 0xf0, 0xf8, 0xe0, 0xa0, 0x10])
        .await?;
    Ok(())
}
async fn resize<S: AsyncWrite + Unpin>(s: &mut S, size: (u16, u16)) -> Result<()> {
    s.write_all(&[0, 0, 0, 1]).await?;
    header(s, 0, 0, size.0, size.1, -223).await?;
    Ok(())
}
pub async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    mut s: S,
    counts: &Counts,
    behavior: Behavior,
) -> Result<()> {
    counts.active_streams.fetch_add(1, Ordering::SeqCst);
    let _active = Active(counts);
    s.write_all(b"RFB 003.008\n").await?;
    let mut version = [0; 12];
    s.read_exact(&mut version).await?;
    ensure!(&version == b"RFB 003.008\n", "fixture version");
    let extra = matches!(behavior, Behavior::ExtraAuthentication);
    s.write_all(&[1, if extra { 2 } else { 1 }]).await?;
    let security = s.read_u8().await?;
    if extra {
        ensure!(security == 2, "extra auth selection");
        let mut end = [0];
        let _ = s.read(&mut end).await;
        return Ok(());
    }
    ensure!(security == 1, "fixture security");
    s.write_u32(0).await?;
    s.read_u8().await?;
    let mut size = (320, 180);
    s.write_u16(size.0).await?;
    s.write_u16(size.1).await?;
    s.write_all(&[32, 24, 0, 255, 0, 255, 0, 255, 0, 255, 0, 8, 16, 0, 0, 0])
        .await?;
    s.write_u32(7).await?;
    s.write_all(b"fixture").await?;
    ensure!(s.read_u8().await? == 0, "pixel format");
    let mut format = [0; 19];
    s.read_exact(&mut format).await?;
    ensure!(s.read_u8().await? == 2, "encodings");
    s.read_u8().await?;
    let count = s.read_u16().await?;
    let mut encodings = Vec::new();
    for _ in 0..count {
        encodings.push(s.read_i32().await?);
    }
    ensure!(
        encodings.contains(&0) && encodings.contains(&-223) && encodings.contains(&-239),
        "native encodings"
    );
    counts.encodings.lock().unwrap().push(encodings);
    let mut position = (160, 90, 0);
    let mut dirty = true;
    let mut pending = false;
    let mut first = true;
    let mut same_size = false;
    loop {
        let kind = match s.read_u8().await {
            Ok(kind) => kind,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(())
            }
            Err(e) => return Err(e.into()),
        };
        match kind {
            3 => {
                let mut request = [0; 9];
                s.read_exact(&mut request).await?;
                counts.requests.fetch_add(1, Ordering::SeqCst);
                let full = request[0] == 0;
                if full {
                    counts.full_requests.fetch_add(1, Ordering::SeqCst);
                }
                if matches!(behavior, Behavior::NoFirstFrame) {
                    continue;
                }
                if first {
                    cursor(&mut s).await?;
                    first = false;
                }
                if !same_size && !full {
                    resize(&mut s, size).await?;
                    same_size = true;
                    continue;
                }
                if full || dirty {
                    let keys = counts.keys.lock().unwrap().len();
                    update(
                        &mut s,
                        size,
                        position,
                        keys,
                        matches!(behavior, Behavior::PartialFirstFrame),
                    )
                    .await?;
                    dirty = false;
                } else {
                    pending = true;
                }
            }
            4 => {
                let down = s.read_u8().await? != 0;
                s.read_u16().await?;
                let key = s.read_u32().await?;
                counts.keys.lock().unwrap().push((key, down));
                {
                    let mut held = counts.held.lock().unwrap();
                    if down {
                        held.insert(key);
                    } else {
                        held.remove(&key);
                    }
                }
                if down && key == 0xffbf {
                    size = if size == (320, 180) {
                        (240, 240)
                    } else {
                        (320, 180)
                    };
                    counts.resizes.fetch_add(1, Ordering::SeqCst);
                    resize(&mut s, size).await?;
                    pending = false;
                }
                dirty = true;
            }
            5 => {
                let mask = s.read_u8().await?;
                let x = s.read_u16().await?;
                let y = s.read_u16().await?;
                position = (x, y, mask);
                counts.pointers.lock().unwrap().push(position);
                dirty = true;
            }
            6 => {
                let mut pad = [0; 3];
                s.read_exact(&mut pad).await?;
                let length = s.read_u32().await? as usize;
                ensure!(length <= 1024 * 1024, "fixture clipboard");
                let mut text = vec![0; length];
                s.read_exact(&mut text).await?;
                counts.clipboard.lock().unwrap().push(text.clone());
                s.write_all(&[3, 0, 0, 0]).await?;
                s.write_u32(length as u32).await?;
                s.write_all(&text).await?;
            }
            other => anyhow::bail!("unexpected native fixture message {other}"),
        }
        if pending && dirty {
            let keys = counts.keys.lock().unwrap().len();
            update(&mut s, size, position, keys, false).await?;
            pending = false;
            dirty = false;
        }
    }
}

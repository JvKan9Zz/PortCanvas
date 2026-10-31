//! Native RFB session, bounded input and complete-frame ownership.
use crate::rfb::{self, Frame};
use anyhow::{bail, ensure, Context, Result};
use portcanvas_core::{desktop_settings::DesktopMode, services::Services};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use vnc::{Rect, VncClient, VncEvent, X11Event};

#[derive(Clone, Copy, Default)]
pub struct Motion {
    pub sequence: u64,
    pub view: u64,
    pub x: u16,
    pub y: u16,
}
pub struct Input {
    pub view: u64,
    pub command: Command,
}
pub enum Command {
    Key(u32, bool),
    Text(String),
    Pointer(Motion, u8),
    Wheel(Motion, u8, u8),
    Release(u64),
    Clipboard(String),
}
#[derive(Clone, Copy)]
pub struct State {
    pub id: u64,
    pub generation: u64,
    pub view: u64,
    pub ready: bool,
}
pub type StateSink = Arc<dyn Fn(State) + Send + Sync>;
pub struct Channels {
    pub inputs: mpsc::Receiver<Input>,
    pub motion: watch::Receiver<Motion>,
    pub frames: watch::Sender<Option<Arc<Frame>>>,
    pub clipboard: watch::Sender<Option<String>>,
    pub state: StateSink,
}

struct Coverage {
    width: usize,
    height: usize,
    bits: Vec<u64>,
    remaining: usize,
}
impl Coverage {
    fn new(width: u16, height: u16) -> Self {
        let size = usize::from(width) * usize::from(height);
        Self {
            width: width.into(),
            height: height.into(),
            bits: vec![0; size.div_ceil(64)],
            remaining: size,
        }
    }
    fn contains(&self, index: usize) -> bool {
        self.bits[index / 64] & (1 << (index % 64)) != 0
    }
    fn put(&mut self, index: usize, value: bool) {
        let old = self.contains(index);
        if old == value {
            return;
        }
        if value {
            self.bits[index / 64] |= 1 << (index % 64);
            self.remaining -= 1;
        } else {
            self.bits[index / 64] &= !(1 << (index % 64));
            self.remaining += 1;
        }
    }
    fn mark(&mut self, rect: Rect) {
        for y in usize::from(rect.y)..usize::from(rect.y + rect.height) {
            let mut start = y * self.width + usize::from(rect.x);
            let end = start + usize::from(rect.width);
            while start < end {
                let count = (64 - start % 64).min(end - start);
                let mask = (u64::MAX >> (64 - count)) << (start % 64);
                let word = &mut self.bits[start / 64];
                self.remaining -= ((!*word) & mask).count_ones() as usize;
                *word |= mask;
                start += count;
            }
        }
    }
    fn copy(&mut self, to: Rect, from: Rect) {
        let mut known = Vec::with_capacity(usize::from(from.width) * usize::from(from.height));
        for y in 0..usize::from(from.height) {
            for x in 0..usize::from(from.width) {
                known.push(
                    self.contains((usize::from(from.y) + y) * self.width + usize::from(from.x) + x),
                );
            }
        }
        for y in 0..usize::from(to.height) {
            for x in 0..usize::from(to.width) {
                self.put(
                    (usize::from(to.y) + y) * self.width + usize::from(to.x) + x,
                    known[y * usize::from(to.width) + x],
                );
            }
        }
    }
    fn full(&self) -> bool {
        self.width > 0 && self.height > 0 && self.remaining == 0
    }
}
#[derive(Default)]
struct Held {
    keys: BTreeSet<u32>,
    buttons: u8,
    position: Motion,
}
async fn send(client: &VncClient, event: X11Event, cancel: &CancellationToken) -> Result<()> {
    tokio::select! {biased;_=cancel.cancelled()=>bail!("桌面连接已取消。"),result=tokio::time::timeout(Duration::from_millis(500),client.input(event))=>result.context("桌面输入队列繁忙。")?.map_err(Into::into)}
}
impl Held {
    async fn key(
        &mut self,
        client: &VncClient,
        key: u32,
        down: bool,
        cancel: &CancellationToken,
    ) -> Result<()> {
        send(client, X11Event::KeyEvent((key, down).into()), cancel).await?;
        if down {
            self.keys.insert(key);
        } else {
            self.keys.remove(&key);
        }
        Ok(())
    }
    async fn pointer(
        &mut self,
        client: &VncClient,
        motion: Motion,
        buttons: Option<u8>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if motion.sequence < self.position.sequence {
            return Ok(());
        }
        let mask = buttons.unwrap_or(self.buttons);
        send(
            client,
            X11Event::PointerEvent((motion.x, motion.y, mask).into()),
            cancel,
        )
        .await?;
        self.position = motion;
        self.buttons = mask;
        Ok(())
    }
    async fn release(
        &mut self,
        client: &VncClient,
        sequence: u64,
        cancel: &CancellationToken,
    ) -> Result<()> {
        for key in self.keys.iter().copied().collect::<Vec<_>>() {
            self.key(client, key, false, cancel).await?;
        }
        if self.buttons != 0 {
            let mut point = self.position;
            point.sequence = point.sequence.max(sequence);
            self.pointer(client, point, Some(0), cancel).await?;
        }
        self.position.sequence = self.position.sequence.max(sequence);
        Ok(())
    }
    async fn input(
        &mut self,
        client: &VncClient,
        input: Input,
        ready: bool,
        view: u64,
        cancel: &CancellationToken,
    ) -> Result<()> {
        match input.command {
            Command::Release(sequence) => self.release(client, sequence, cancel).await,
            Command::Key(key, false) => {
                if self.keys.contains(&key) {
                    self.key(client, key, false, cancel).await
                } else {
                    Ok(())
                }
            }
            Command::Pointer(point, 0) => {
                if self.buttons != 0 || (ready && input.view == view) {
                    self.pointer(client, point, Some(0), cancel).await
                } else {
                    Ok(())
                }
            }
            _ if !ready || input.view != view => Ok(()),
            Command::Key(key, down) => self.key(client, key, down, cancel).await,
            Command::Text(text) => {
                ensure!(text.len() <= 65536, "单次文字输入过长。");
                for c in text.chars() {
                    let key = if u32::from(c) > 255 {
                        0x01000000 | u32::from(c)
                    } else {
                        rfb::keysym(c)
                    };
                    self.key(client, key, true, cancel).await?;
                    self.key(client, key, false, cancel).await?;
                }
                Ok(())
            }
            Command::Pointer(point, buttons) => {
                self.pointer(client, point, Some(buttons), cancel).await
            }
            Command::Wheel(point, wheel, count) => {
                if point.sequence < self.position.sequence {
                    return Ok(());
                }
                self.pointer(client, point, None, cancel).await?;
                for _ in 0..count.min(8) {
                    send(
                        client,
                        X11Event::PointerEvent((point.x, point.y, self.buttons | wheel).into()),
                        cancel,
                    )
                    .await?;
                    send(
                        client,
                        X11Event::PointerEvent((point.x, point.y, self.buttons).into()),
                        cancel,
                    )
                    .await?;
                }
                Ok(())
            }
            Command::Clipboard(text) => send(client, X11Event::CopyText(text), cancel).await,
        }
    }
}
pub async fn run(
    services: Arc<Services>,
    expected_generation: u64,
    mode: DesktopMode,
    channels: Channels,
    cancel: CancellationToken,
) -> Result<()> {
    let Channels {
        mut inputs,
        mut motion,
        frames,
        clipboard,
        state,
    } = channels;
    let mut lease = services
        .desktop
        .start_native(mode, expected_generation, cancel.clone())
        .await?;
    let id = lease.id;
    let generation = lease.generation;
    let backend_cancel = lease.cancellation();
    state(State {
        id,
        generation,
        view: 0,
        ready: false,
    });
    let result=async {
        let stream=lease.take_stream()?;
        let client=tokio::select!{biased;_=cancel.cancelled()=>bail!("桌面连接已取消。"),result=tokio::time::timeout(Duration::from_secs(10),rfb::connect_with_mode(stream,mode))=>result.context("RFB握手超时。")??};
        services.desktop.awaiting_frame(id);
        let mut frame=Frame::default();let mut coverage=None::<Coverage>;let mut ready=false;let mut dirty=false;let mut outstanding=true;let mut full_request=false;let mut full_retry=false;let mut held=Held::default();let mut motion_open=true;let mut pixels_received=false;
        let deadline=tokio::time::sleep(Duration::from_secs(20));tokio::pin!(deadline);
        let mut tick=tokio::time::interval(Duration::from_secs_f64(mode.tuning()["frameInterval"].as_f64().unwrap()));tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let operation=async {loop {tokio::select! {
            biased;
            _=cancel.cancelled()=>break,
            _=frames.closed()=>break,
            _=backend_cancel.cancelled()=>bail!("远程桌面通道已结束。"),
            _=&mut deadline,if !ready=>bail!("等待完整桌面画面超时。"),
            command=inputs.recv()=>match command {Some(command)=>held.input(&client,command,ready,frame.view,&cancel).await?,None=>break},
            changed=motion.changed(),if motion_open=>{if changed.is_err(){motion_open=false;}else if ready {let point=*motion.borrow_and_update();if point.view==frame.view {held.pointer(&client,point,None,&cancel).await?;}}},
            _=tick.tick()=>{if !outstanding {send(&client,if full_request{X11Event::FullRefresh}else{X11Event::Refresh},&cancel).await?;outstanding=true;full_request=false;}},
            event=client.recv_event()=>{
                let event=event?;
                let size=match &event {VncEvent::SetResolution(s)=>Some((s.width,s.height)),VncEvent::DesktopUpdate(update)=>update.layout.as_ref().map(|s|(s.width,s.height)),_=>None};
                if let Some((width,height))=size {
                    if (frame.width,frame.height)!=(width,height) {
                        frame.apply(VncEvent::SetResolution((width,height).into()))?;
                        frame.view=frame.view.wrapping_add(1);
                        coverage=Some(Coverage::new(width,height));dirty=true;
                        if ready {held.position.x=held.position.x.min(width-1);held.position.y=held.position.y.min(height-1);held.release(&client,held.position.sequence,&cancel).await?;ready=false;full_request=true;full_retry=true;services.desktop.awaiting_frame(id);deadline.as_mut().reset(tokio::time::Instant::now()+Duration::from_secs(20));}
                        state(State{id,generation,view:frame.view,ready:false});
                    }
                } else {match event {
                    VncEvent::Text(text)=>{clipboard.send_replace(Some(text));},
                    VncEvent::FrameEnd=>{
                        if ready||pixels_received||full_request {outstanding=false;}
                        if !ready&&coverage.as_ref().is_some_and(Coverage::full) {ready=true;coverage=None;services.desktop.ready(id);state(State{id,generation,view:frame.view,ready:true});}
                        else if !ready&&pixels_received&&!full_retry {full_request=true;full_retry=true;}
                        if ready&&dirty {frames.send_replace(Some(Arc::new(frame.clone())));dirty=false;}
                        pixels_received=false;
                    },
                    VncEvent::RawImage(rect,data)=>{frame.apply(VncEvent::RawImage(rect,data))?;if let Some(c)=&mut coverage{c.mark(rect);}dirty=true;pixels_received=true;},
                    VncEvent::JpegImage(rect,data)=>{frame.apply(VncEvent::JpegImage(rect,data))?;if let Some(c)=&mut coverage{c.mark(rect);}dirty=true;pixels_received=true;},
                    VncEvent::Copy(to,from)=>{frame.apply(VncEvent::Copy(to,from))?;if let Some(c)=&mut coverage{c.copy(to,from);}dirty=true;pixels_received=true;},
                    other=>{dirty|=frame.apply(other)?;},
                }}
            }
        }}Ok::<_,anyhow::Error>(())}.await;
        // Input cleanup precedes client/tunnel cancellation, including page/minimize shutdown.
        let cleanup=CancellationToken::new();let _=tokio::time::timeout(Duration::from_millis(250),async {held.release(&client,u64::MAX,&cleanup).await?;client.flush_input().await?;Ok::<_,anyhow::Error>(())}).await;
        let closed=client.close().await;
        operation?;closed?;Ok::<_,anyhow::Error>(())
    }.await;
    drop(lease);
    if result.is_err()
        && !cancel.is_cancelled()
        && services.devices.snapshot()["ssh"] == "connected"
    {
        services
            .desktop
            .failed(id, result.as_ref().unwrap_err().to_string())
            .await;
    } else {
        services.desktop.stop(Some(id)).await;
    }
    result
}

pub fn point(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    remote_width: u16,
    remote_height: u16,
) -> Option<(u16, u16)> {
    if width <= 0.0
        || height <= 0.0
        || remote_width == 0
        || remote_height == 0
        || ![x, y, width, height].iter().all(|v| v.is_finite())
    {
        return None;
    }
    Some((
        (x / width * f32::from(remote_width))
            .floor()
            .clamp(0.0, f32::from(remote_width - 1)) as u16,
        (y / height * f32::from(remote_height))
            .floor()
            .clamp(0.0, f32::from(remote_height - 1)) as u16,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_pixels_and_overlapping_copy_are_required_before_input() {
        let mut c = Coverage::new(4, 2);
        let left = Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        c.mark(left);
        assert!(!c.full());
        c.copy(Rect { x: 2, ..left }, left);
        assert!(c.full());
        let mut c = Coverage::new(4, 2);
        c.mark(Rect { x: 2, ..left });
        c.copy(Rect { x: 2, ..left }, left);
        assert!(!c.full());
        assert_eq!(c.remaining, 8);
    }
    #[test]
    fn scaled_pointer_mapping_clamps_edges_and_rejects_bad_geometry() {
        assert_eq!(point(100.0, 50.0, 200.0, 100.0, 320, 180), Some((160, 90)));
        assert_eq!(point(200.0, -1.0, 200.0, 100.0, 320, 180), Some((319, 0)));
        assert!(point(0.0, 0.0, 0.0, 100.0, 320, 180).is_none());
        assert!(point(f32::NAN, 0.0, 2.0, 2.0, 4, 4).is_none());
    }
}

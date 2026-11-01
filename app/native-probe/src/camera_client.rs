//! Bounded native JPEG decoding; no local HTTP client, server or WebView.
use anyhow::{ensure, Result};
use image::ImageDecoder;
use portcanvas_core::services::Services;
use std::{io::Cursor, sync::Arc, time::Duration};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
pub struct Frame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
pub fn decode(sequence: u64, jpeg: &[u8]) -> Result<Frame> {
    let decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(jpeg))?;
    let (width, height) = decoder.dimensions();
    ensure!(
        width > 0 && height > 0 && width <= 2048 && height <= 2048,
        "摄像头图像尺寸超限。"
    );
    let pixels = image::DynamicImage::from_decoder(decoder)?
        .into_rgb8()
        .into_raw();
    Ok(Frame {
        sequence,
        width,
        height,
        pixels,
    })
}
pub async fn run(
    s: Arc<Services>,
    generation: u64,
    path: String,
    fps: u32,
    frames: watch::Sender<Option<Arc<Frame>>>,
    cancel: CancellationToken,
) -> Result<()> {
    let state = s
        .arena
        .start_native(path, fps, generation, cancel.clone())
        .await?;
    let id = state["id"].as_u64().unwrap();
    let result = async {
        let mut sequence = 0;
        let mut tick = tokio::time::interval(Duration::from_millis(33));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;_=cancel.cancelled()=>break,_=tick.tick()=>{}}
            ensure!(s.devices.generation() == generation, "设备连接已变化。");
            let status = s.arena.status();
            ensure!(
                !matches!(status["phase"].as_str(), Some("error" | "idle")),
                "{}",
                status["message"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("摄像头采集已结束。")
            );
            if let Some(frame) = s.arena.native_frame(id, sequence, generation)? {
                sequence = frame.sequence;
                let decoded = decode(sequence, &frame.jpeg)?;
                if cancel.is_cancelled() {
                    break;
                }
                frames.send_replace(Some(Arc::new(decoded)));
            }
        }
        Ok(())
    }
    .await;
    s.arena.stop(Some(id)).await;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_jpeg_decode_checks_payload_and_dimensions() {
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut data)
            .encode(
                &[255, 0, 0, 0, 255, 0],
                2,
                1,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        let f = decode(7, &data).unwrap();
        assert_eq!(
            (f.sequence, f.width, f.height, f.pixels.len()),
            (7, 2, 1, 6)
        );
        assert!(decode(1, b"not an image").is_err());
    }
}

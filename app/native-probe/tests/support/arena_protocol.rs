//! Synthetic SSH peer; never opens a device or executes the supplied script.
use base64::Engine;
use russh::{server, ChannelId};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tokio_util::sync::CancellationToken;
#[derive(Default)]
pub struct Counts {
    pub cameras: AtomicUsize,
    pub poses: AtomicUsize,
    pub metrics: AtomicUsize,
    pub pose_mode: AtomicUsize,
    pub bad_jpeg: AtomicBool,
}
pub fn arguments(command: &str) -> Option<Vec<Value>> {
    command
        .split(|c: char| !c.is_ascii_alphanumeric() && !"+/=".contains(c))
        .find_map(|part| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(part)
                .ok()?;
            let v: Vec<Value> = serde_json::from_slice(&bytes).ok()?;
            matches!(
                v.first()?.as_str()?,
                "metrics" | "cameras" | "pose" | "camera"
            )
            .then_some(v)
        })
}
pub fn start(
    args: Vec<Value>,
    id: ChannelId,
    session: &mut server::Session,
    c: Arc<Counts>,
) -> Result<Option<CancellationToken>, russh::Error> {
    let kind = args[0].as_str().unwrap();
    if kind == "metrics" || kind == "cameras" {
        let value = if kind == "cameras" {
            json!([{"path":"/dev/video0","name":"受控图像（非实机）","accessible":true}])
        } else {
            let n = c.metrics.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            json!({"clock":n*2,"cpuTotal":n*100,"cpuIdle":n*40,"memoryTotal":1073741824u64,"memoryAvailable":536870912,"swapTotal":0,"swapFree":0,"uptime":600,"load":[0.1,0.2,0.3],"network":[{"name":"fixture","received":n*2000,"sent":n*1000}]})
        };
        session.data(id, (value.to_string() + "\n").into_bytes())?;
        session.exit_status_request(id, 0)?;
        session.close(id)?;
        return Ok(None);
    }
    let camera = kind == "camera";
    let token = CancellationToken::new();
    let cancel = token.clone();
    let handle = session.handle();
    if camera {
        c.cameras.fetch_add(1, Ordering::SeqCst);
        session.extended_data(id, 1, b"PORTCANVAS_CAMERA:ffmpeg-copy\n".to_vec())?;
    } else {
        c.poses.fetch_add(1, Ordering::SeqCst);
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
        let mut sequence = 0u8;
        loop {
            tokio::select! {biased;_=cancel.cancelled()=>break,_=tick.tick()=>{}}
            let packet = if camera {
                let mut jpeg = Vec::new();
                let rgb = [sequence, 90, 180].repeat(64 * 36);
                sequence = sequence.wrapping_add(1);
                if c.bad_jpeg.load(Ordering::SeqCst) {
                    jpeg = b"bad jpeg".to_vec();
                } else if image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
                    .encode(&rgb, 64, 36, image::ExtendedColorType::Rgb8)
                    .is_err()
                {
                    break;
                }
                let mut packet = (jpeg.len() as u32).to_be_bytes().to_vec();
                packet.extend(jpeg);
                packet
            } else {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                let mut v = json!({"source":"sample","nowMs":now,"pid":12345,"sample":{"schema":"portcanvas.pose/1","x":1.25,"y":2.60,"heading":90,"timestamp":now}});
                match c.pose_mode.load(Ordering::SeqCst) {
                    0 => v = json!({"source":"missing"}),
                    2 => v["sample"]["timestamp"] = json!(now - 5000),
                    3 => v["sample"]["x"] = json!(true),
                    4 => v["sample"]["valid"] = json!(false),
                    _ => {}
                }
                (v.to_string() + "\n").into_bytes()
            };
            if handle.data(id, packet).await.is_err() {
                break;
            }
        }
        if camera {
            c.cameras.fetch_sub(1, Ordering::SeqCst);
        } else {
            c.poses.fetch_sub(1, Ordering::SeqCst);
        }
        let _ = handle.close(id).await;
    });
    Ok(Some(token))
}

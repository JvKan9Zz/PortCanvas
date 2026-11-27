//! Synthetic SSH peer; never opens a device or executes the supplied script.
use base64::Engine;
use russh::{server, ChannelId};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio_util::sync::CancellationToken;
#[derive(Default)]
pub struct Counts {
    pub cameras: AtomicUsize,
    pub poses: AtomicUsize,
    pub vision_streams: AtomicUsize,
    pub vision_configs: Mutex<Vec<Value>>,
    pub metrics: AtomicUsize,
    pub hold_metrics: AtomicBool,
    pub metrics_pending: AtomicUsize,
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
                "metrics" | "cameras" | "pose" | "camera" | "vision-inspect" | "vision-stream"
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
    if kind == "vision-inspect" {
        let data = json!({"cameras":[],"holders":[],"windows":[],"sharedFrame":{"available":true},"capabilities":{"cv2":true}});
        session.data(id, (data.to_string() + "\n").into_bytes())?;
        session.exit_status_request(id, 0)?;
        session.close(id)?;
        return Ok(None);
    }
    if kind == "metrics" && c.hold_metrics.load(Ordering::SeqCst) {
        let cancel = CancellationToken::new();
        let pending = cancel.clone();
        c.metrics_pending.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            pending.cancelled().await;
            c.metrics_pending.fetch_sub(1, Ordering::SeqCst);
        });
        return Ok(Some(cancel));
    }
    if kind == "metrics" || kind == "cameras" {
        let value = if kind == "cameras" {
            json!([{"path":"/dev/video0","name":"受控图像（非实机）","accessible":true}])
        } else {
            let n = c.metrics.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            let bump = (n % 4) * 8;
            json!({
                "clock": n * 2,
                "cpuTotal": n * 100,
                "cpuIdle": n * 40 + bump,
                "cores": 4,
                "cpuCores": [
                    {"id": 0, "total": n * 100, "idle": n * 25 + bump},
                    {"id": 1, "total": n * 100, "idle": n * 48},
                    {"id": 2, "total": n * 100, "idle": n * 62},
                    {"id": 3, "total": n * 100, "idle": n * 18 + (n % 3) * 4}
                ],
                "cpuFreqMhz": 1800.0,
                "memoryTotal": 4294967296u64,
                "memoryAvailable": 2040109465u64.saturating_sub((n % 5) * 33554432),
                "memoryCached": 536870912u64,
                "swapTotal": 1073741824u64,
                "swapFree": 939524096u64,
                "uptime": 7540 + n * 2,
                "load": [0.42, 0.35, 0.28],
                "network": [
                    {"name": "eth0", "received": n * 245760, "sent": n * 81920},
                    {"name": "wlan0", "received": n * 24576, "sent": n * 12288}
                ],
                "storage": [
                    {"mount": "/", "device": "/dev/mmcblk0p2", "fsType": "ext4", "totalBytes": 31268536320u64, "usedBytes": 19327352832u64, "availableBytes": 10630044057u64},
                    {"mount": "/boot", "device": "/dev/mmcblk0p1", "fsType": "vfat", "totalBytes": 268435456u64, "usedBytes": 75497472u64, "availableBytes": 192937984u64},
                    {"mount": "/mnt/sd", "device": "/dev/mmcblk1p1", "fsType": "exfat", "totalBytes": 64424509440u64, "usedBytes": 52183852646u64, "availableBytes": 12240656794u64}
                ],
                "thermals": [
                    {"label": "cpu-thermal", "celsius": 48.5},
                    {"label": "gpu-thermal", "celsius": 44.2}
                ],
                "system": {
                    "hostname": "bcsh-fixture",
                    "os": "Ubuntu 20.04.6 LTS",
                    "kernel": "4.19.172-rk3399",
                    "arch": "aarch64"
                }
            })
        };
        session.data(id, (value.to_string() + "\n").into_bytes())?;
        session.exit_status_request(id, 0)?;
        session.close(id)?;
        return Ok(None);
    }
    let vision = kind == "vision-stream";
    let camera = kind == "camera" || vision;
    let token = CancellationToken::new();
    let cancel = token.clone();
    let handle = session.handle();
    if vision {
        c.vision_streams.fetch_add(1, Ordering::SeqCst);
        session.extended_data(id, 1, b"PORTCANVAS_VISION:{\"activeSource\":\"shared\",\"width\":64,\"height\":36,\"pid\":1234}\n".to_vec())?;
    } else if camera {
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
                let mut v = json!({"source":"sample","nowMs":now,"pid":12345,"sample":{"schema":"portcanvas.pose/1","x":1.25,"y":2.60,"heading":90,"confidence":0.96,"timestamp":now}});
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
        if vision {
            c.vision_streams.fetch_sub(1, Ordering::SeqCst);
        } else if camera {
            c.cameras.fetch_sub(1, Ordering::SeqCst);
        } else {
            c.poses.fetch_sub(1, Ordering::SeqCst);
        }
        let _ = handle.close(id).await;
    });
    Ok(Some(token))
}

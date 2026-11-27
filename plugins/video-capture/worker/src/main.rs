//! PortCanvas Video Capture Worker (`pc-worker-v1`)
//! Native low-latency Windows MediaFoundation capture card / webcam worker.
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    net::TcpStream,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const FRAME_MAGIC: &[u8; 4] = b"PCFR";

#[derive(Clone, Debug)]
struct CaptureMode {
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    format_name: String,
    native_index: u32,
}

impl CaptureMode {
    fn fps(&self) -> f64 {
        if self.fps_den == 0 {
            30.0
        } else {
            self.fps_num as f64 / self.fps_den as f64
        }
    }

    fn label(&self) -> String {
        format!(
            "{}×{} @ {:.0} FPS ({})",
            self.width,
            self.height,
            self.fps().round(),
            self.format_name
        )
    }
}

#[derive(Clone, Debug)]
struct CaptureDevice {
    name: String,
    symlink: String,
    modes: Vec<CaptureMode>,
}

struct FrameSender {
    stream: TcpStream,
}

impl FrameSender {
    fn connect(addr: &str, token: &str) -> Result<Self> {
        let mut stream = TcpStream::connect(addr)?;
        let _ = stream.set_nodelay(true);
        stream.write_all(token.as_bytes())?;
        stream.flush()?;
        Ok(Self { stream })
    }

    fn send_rgba(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
        let mut header = [0u8; 20];
        header[0..4].copy_from_slice(FRAME_MAGIC);
        header[4..8].copy_from_slice(&width.to_le_bytes());
        header[8..12].copy_from_slice(&height.to_le_bytes());
        header[12] = 1; // RGBA8
        header[16..20].copy_from_slice(&(rgba.len() as u32).to_le_bytes());
        self.stream.write_all(&header)?;
        self.stream.write_all(rgba)?;
        Ok(())
    }
}

fn emit(out: &Arc<Mutex<io::Stdout>>, value: Value) {
    if let Ok(mut guard) = out.lock() {
        let _ = serde_json::to_writer(&mut *guard, &value);
        let _ = guard.write_all(b"\n");
        let _ = guard.flush();
    }
}

fn emit_patch(out: &Arc<Mutex<io::Stdout>>, patch: Value) {
    emit(out, json!({"type": "state_patch", "patch": patch}));
}

fn bgra_to_rgba_into(src: &[u8], width: usize, height: usize, stride: isize, dst: &mut Vec<u8>) {
    dst.resize(width * height * 4, 255);
    let abs_stride = stride.unsigned_abs();
    let bottom_up = stride < 0;
    for y in 0..height {
        let src_row_idx = if bottom_up { height - 1 - y } else { y };
        let src_start = src_row_idx * abs_stride;
        if src_start + width * 4 > src.len() {
            break;
        }
        let srow = &src[src_start..src_start + width * 4];
        let drow = &mut dst[y * width * 4..(y + 1) * width * 4];
        for x in 0..width {
            let si = x * 4;
            drow[si] = srow[si + 2]; // R
            drow[si + 1] = srow[si + 1]; // G
            drow[si + 2] = srow[si]; // B
            drow[si + 3] = 255; // A
        }
    }
}

#[cfg(windows)]
mod mf {
    use super::*;
    use windows::{
        core::{Interface, GUID, PWSTR},
        Win32::{
            Media::MediaFoundation::{
                IMF2DBuffer, IMFActivate, IMFAttributes, IMFMediaSource, IMFMediaType,
                IMFSourceReader, MFCreateAttributes, MFCreateSourceReaderFromMediaSource,
                MFEnumDeviceSources, MFShutdown, MFStartup, MFVideoFormat_MJPG,
                MFVideoFormat_NV12, MFVideoFormat_RGB24, MFVideoFormat_RGB32, MFVideoFormat_YUY2,
                MFMediaType_Video, MFSTARTUP_NOSOCKET, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_LOW_LATENCY,
                MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
                MF_MT_SUBTYPE, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
                MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
            },
            System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED},
        },
    };

    struct ComMfGuard;
    impl ComMfGuard {
        fn init() -> Result<Self> {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)?;
            }
            Ok(Self)
        }
    }
    impl Drop for ComMfGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
                CoUninitialize();
            }
        }
    }

    fn subtype_label(guid: GUID) -> String {
        if guid == MFVideoFormat_MJPG {
            "MJPEG".into()
        } else if guid == MFVideoFormat_NV12 {
            "NV12".into()
        } else if guid == MFVideoFormat_YUY2 {
            "YUY2".into()
        } else if guid == MFVideoFormat_RGB32 {
            "RGB32".into()
        } else if guid == MFVideoFormat_RGB24 {
            "RGB24".into()
        } else {
            format!("{:08X}", guid.data1)
        }
    }

    fn get_string_attr(attrs: &IMFAttributes, key: &GUID) -> Option<String> {
        unsafe {
            let mut ptr = PWSTR::null();
            let mut len = 0u32;
            if attrs.GetAllocatedString(key, &mut ptr, &mut len).is_ok() && !ptr.is_null() {
                let slice = std::slice::from_raw_parts(ptr.0, len as usize);
                let text = String::from_utf16_lossy(slice);
                CoTaskMemFree(Some(ptr.0 as *const _));
                return Some(text);
            }
        }
        None
    }

    fn enumerate_modes(source: &IMFMediaSource) -> Vec<CaptureMode> {
        let mut best: BTreeMap<(u32, u32, u32), CaptureMode> = BTreeMap::new();
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            if MFCreateAttributes(&mut attrs, 2).is_err() {
                return Vec::new();
            }
            let Some(attrs) = attrs else {
                return Vec::new();
            };
            let _ = attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
            let Ok(reader) = MFCreateSourceReaderFromMediaSource(source, &attrs) else {
                return Vec::new();
            };
            for idx in 0..256u32 {
                let Ok(mt) = reader.GetNativeMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, idx) else {
                    break;
                };
                let Ok(size) = mt.GetUINT64(&MF_MT_FRAME_SIZE) else {
                    continue;
                };
                let width = (size >> 32) as u32;
                let height = (size & 0xFFFF_FFFF) as u32;
                if width == 0 || height == 0 || width > 4096 || height > 2160 {
                    continue;
                }
                let rate = mt.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or((30u64 << 32) | 1);
                let fps_num = (rate >> 32) as u32;
                let fps_den = ((rate & 0xFFFF_FFFF) as u32).max(1);
                let fps_int = ((fps_num as f64) / (fps_den as f64)).round() as u32;
                if fps_int == 0 || fps_int > 240 {
                    continue;
                }
                let subtype = mt.GetGUID(&MF_MT_SUBTYPE).unwrap_or_default();
                let fmt = subtype_label(subtype);
                let score = match fmt.as_str() {
                    "MJPEG" => 4,
                    "NV12" => 3,
                    "YUY2" => 2,
                    "RGB32" => 1,
                    _ => 0,
                };
                let key = (width, height, fps_int);
                let replace = best.get(&key).map_or(true, |old| {
                    let old_score = match old.format_name.as_str() {
                        "MJPEG" => 4,
                        "NV12" => 3,
                        "YUY2" => 2,
                        "RGB32" => 1,
                        _ => 0,
                    };
                    score > old_score
                });
                if replace {
                    best.insert(
                        key,
                        CaptureMode {
                            width,
                            height,
                            fps_num,
                            fps_den,
                            format_name: fmt,
                            native_index: idx,
                        },
                    );
                }
            }
        }
        let mut modes: Vec<CaptureMode> = best.into_values().collect();
        modes.sort_by(|a, b| {
            let rank = |m: &CaptureMode| -> (u32, u64) {
                let fps = m.fps().round() as u32;
                let preferred = if m.width == 1920 && m.height == 1080 && fps >= 59 {
                    3
                } else if m.width == 1280 && m.height == 720 && fps >= 59 {
                    2
                } else if m.width == 1920 && m.height == 1080 {
                    1
                } else {
                    0
                };
                (preferred, (m.width as u64) * (m.height as u64) * (fps as u64))
            };
            rank(b).cmp(&rank(a))
        });
        modes
    }

    pub fn list_devices() -> Result<Vec<CaptureDevice>> {
        let _guard = ComMfGuard::init()?;
        let mut devices = Vec::new();
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 1)?;
            let attrs = attrs.ok_or_else(|| anyhow!("MFCreateAttributes failed"))?;
            attrs.SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )?;
            let mut activates_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0u32;
            MFEnumDeviceSources(&attrs, &mut activates_ptr, &mut count)?;
            if !activates_ptr.is_null() && count > 0 {
                let slice = std::slice::from_raw_parts(activates_ptr, count as usize);
                for act in slice.iter().flatten() {
                    let act_attrs: IMFAttributes = act.cast()?;
                    let name = get_string_attr(&act_attrs, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
                        .unwrap_or_else(|| "Video Capture Device".into());
                    let symlink = get_string_attr(
                        &act_attrs,
                        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                    )
                    .unwrap_or_default();
                    let modes = act
                        .ActivateObject::<IMFMediaSource>()
                        .ok()
                        .map(|src| {
                            let m = enumerate_modes(&src);
                            let _ = src.Shutdown();
                            m
                        })
                        .unwrap_or_default();
                    devices.push(CaptureDevice {
                        name,
                        symlink,
                        modes,
                    });
                }
                CoTaskMemFree(Some(activates_ptr as *const _));
            }
        }
        Ok(devices)
    }

    fn activate_by_symlink(symlink: &str) -> Result<IMFMediaSource> {
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 1)?;
            let attrs = attrs.ok_or_else(|| anyhow!("MFCreateAttributes failed"))?;
            attrs.SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )?;
            let mut activates_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0u32;
            MFEnumDeviceSources(&attrs, &mut activates_ptr, &mut count)?;
            let mut found = None;
            if !activates_ptr.is_null() && count > 0 {
                let slice = std::slice::from_raw_parts(activates_ptr, count as usize);
                for act in slice.iter().flatten() {
                    let act_attrs: IMFAttributes = act.cast()?;
                    let link = get_string_attr(
                        &act_attrs,
                        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                    )
                    .unwrap_or_default();
                    if link == symlink {
                        found = Some(act.ActivateObject::<IMFMediaSource>()?);
                        break;
                    }
                }
                CoTaskMemFree(Some(activates_ptr as *const _));
            }
            found.ok_or_else(|| anyhow!("未找到所选视频采集设备，请刷新设备列表。"))
        }
    }

    pub fn run_capture(
        symlink: String,
        mode: Option<CaptureMode>,
        frame_addr: String,
        frame_token: String,
        stop: Arc<AtomicBool>,
        out: Arc<Mutex<io::Stdout>>,
    ) -> Result<()> {
        let _guard = ComMfGuard::init()?;
        let source = activate_by_symlink(&symlink)?;
        let reader: IMFSourceReader = unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 3)?;
            let attrs = attrs.ok_or_else(|| anyhow!("MFCreateAttributes failed"))?;
            let _ = attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
            MFCreateSourceReaderFromMediaSource(&source, &attrs)?
        };

        let stream_idx = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let fmt_label = mode
            .as_ref()
            .map(|m| m.format_name.clone())
            .unwrap_or_else(|| "AUTO".into());

        unsafe {
            if let Some(m) = &mode {
                if let Ok(native) = reader.GetNativeMediaType(stream_idx, m.native_index) {
                    let _ = reader.SetCurrentMediaType(stream_idx, None, &native);
                }
            }
            let out_type: IMFMediaType =
                windows::Win32::Media::MediaFoundation::MFCreateMediaType()?;
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            if let Some(m) = &mode {
                let packed_size = ((m.width as u64) << 32) | (m.height as u64);
                let _ = out_type.SetUINT64(&MF_MT_FRAME_SIZE, packed_size);
                let packed_rate = ((m.fps_num as u64) << 32) | (m.fps_den as u64);
                let _ = out_type.SetUINT64(&MF_MT_FRAME_RATE, packed_rate);
            }
            reader.SetCurrentMediaType(stream_idx, None, &out_type)?;
        }

        let current_mt = unsafe { reader.GetCurrentMediaType(stream_idx)? };
        let packed_size = unsafe { current_mt.GetUINT64(&MF_MT_FRAME_SIZE)? };
        let mut width = (packed_size >> 32) as u32;
        let mut height = (packed_size & 0xFFFF_FFFF) as u32;
        let mut default_stride = unsafe {
            current_mt
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .map(|v| v as i32 as isize)
                .unwrap_or((width * 4) as isize)
        };

        let mut sender = FrameSender::connect(&frame_addr, &frame_token)?;
        emit_patch(
            &out,
            json!({
                "active": true,
                "loading": false,
                "status": "采集中",
                "statusPhase": 2,
                "resolutionText": format!("{width} × {height}"),
                "pixelFormatText": fmt_label,
                "modeSummary": format!("{width}×{height} · {fmt_label}"),
                "footerStatus": format!("实时采集中 · {width}×{height}")
            }),
        );

        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        let mut meter_start = Instant::now();
        let mut meter_frames = 0u32;

        while !stop.load(Ordering::Relaxed) {
            let mut flags = 0u32;
            let mut sample = None;
            let res = unsafe {
                reader.ReadSample(
                    stream_idx,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
            };
            if res.is_err() {
                break;
            }
            // Stream format changed dynamically
            if flags & 0x100 != 0 {
                if let Ok(mt) = unsafe { reader.GetCurrentMediaType(stream_idx) } {
                    if let Ok(ps) = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE) } {
                        width = (ps >> 32) as u32;
                        height = (ps & 0xFFFF_FFFF) as u32;
                    }
                    if let Ok(st) = unsafe { mt.GetUINT32(&MF_MT_DEFAULT_STRIDE) } {
                        default_stride = st as i32 as isize;
                    }
                }
            }
            let Some(sample) = sample else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            let Ok(buffer) = (unsafe { sample.ConvertToContiguousBuffer() }) else {
                continue;
            };

            let copied = if let Ok(buf2d) = buffer.cast::<IMF2DBuffer>() {
                let mut scan0: *mut u8 = std::ptr::null_mut();
                let mut pitch = 0i32;
                if unsafe { buf2d.Lock2D(&mut scan0, &mut pitch) }.is_ok() && !scan0.is_null() {
                    let abs_pitch = pitch.unsigned_abs() as usize;
                    let total = abs_pitch * (height as usize);
                    let src = if pitch >= 0 {
                        unsafe { std::slice::from_raw_parts(scan0, total) }
                    } else {
                        let start = unsafe { scan0.sub(abs_pitch * ((height as usize).saturating_sub(1))) };
                        unsafe { std::slice::from_raw_parts(start, total) }
                    };
                    bgra_to_rgba_into(src, width as usize, height as usize, pitch as isize, &mut rgba);
                    unsafe {
                        let _ = buf2d.Unlock2D();
                    }
                    true
                } else {
                    false
                }
            } else {
                let mut ptr: *mut u8 = std::ptr::null_mut();
                let mut cur_len = 0u32;
                if unsafe { buffer.Lock(&mut ptr, None, Some(&mut cur_len)) }.is_ok()
                    && !ptr.is_null()
                {
                    let src = unsafe { std::slice::from_raw_parts(ptr, cur_len as usize) };
                    bgra_to_rgba_into(
                        src,
                        width as usize,
                        height as usize,
                        default_stride,
                        &mut rgba,
                    );
                    unsafe {
                        let _ = buffer.Unlock();
                    }
                    true
                } else {
                    false
                }
            };

            if copied {
                if sender.send_rgba(width, height, &rgba).is_err() {
                    break;
                }
                meter_frames += 1;
                if meter_start.elapsed() >= Duration::from_millis(500) {
                    let fps = meter_frames as f64 / meter_start.elapsed().as_secs_f64();
                    meter_start = Instant::now();
                    meter_frames = 0;
                    emit_patch(
                        &out,
                        json!({
                            "resolutionText": format!("{width} × {height}"),
                            "fpsText": format!("{fps:.1} FPS"),
                            "modeSummary": format!("{width}×{height} · {fps:.1} FPS · {fmt_label}"),
                            "footerStatus": format!("实时采集中 · {width}×{height} @ {fps:.1} FPS")
                        }),
                    );
                }
            }
        }
        unsafe {
            let _ = source.Shutdown();
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod mf {
    use super::*;
    pub fn list_devices() -> Result<Vec<CaptureDevice>> {
        Ok(Vec::new())
    }
    pub fn run_capture(
        _symlink: String,
        _mode: Option<CaptureMode>,
        _frame_addr: String,
        _frame_token: String,
        _stop: Arc<AtomicBool>,
        _out: Arc<Mutex<io::Stdout>>,
    ) -> Result<()> {
        Err(anyhow!("当前平台暂不支持 Windows MediaFoundation 视频采集。"))
    }
}

fn transform_summary(rotation: i64, flip_h: bool, flip_v: bool) -> String {
    let mut parts = vec![format!("{rotation}°")];
    if flip_h {
        parts.push("水平翻转".into());
    }
    if flip_v {
        parts.push("垂直翻转".into());
    }
    parts.join(" · ")
}

fn main() {
    let out = Arc::new(Mutex::new(io::stdout()));
    let mut devices: Vec<CaptureDevice> = Vec::new();
    let mut device_index: usize = 0;
    let mut format_index: usize = 0;
    let mut rotation: i64 = 0;
    let mut flip_h = false;
    let mut flip_v = false;
    let mut frame_addr = String::new();
    let mut frame_token = String::new();
    let mut active_stop: Option<Arc<AtomicBool>> = None;
    let mut active_thread: Option<thread::JoinHandle<()>> = None;

    let stop_capture = |active_stop: &mut Option<Arc<AtomicBool>>,
                        active_thread: &mut Option<thread::JoinHandle<()>>,
                        out: &Arc<Mutex<io::Stdout>>| {
        if let Some(stop) = active_stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        if let Some(handle) = active_thread.take() {
            let _ = handle.join();
        }
        emit_patch(
            out,
            json!({
                "active": false,
                "loading": false,
                "status": "未采集",
                "statusPhase": 0,
                "fpsText": "—",
                "footerStatus": "采集已停止"
            }),
        );
    };

    let push_devices = |devices: &[CaptureDevice],
                        device_index: &mut usize,
                        format_index: &mut usize,
                        out: &Arc<Mutex<io::Stdout>>| {
        if *device_index >= devices.len() {
            *device_index = 0;
        }
        let dev_opts: Vec<Value> = devices
            .iter()
            .enumerate()
            .map(|(i, d)| json!({"label": d.name, "value": i}))
            .collect();
        let fmt_opts: Vec<Value> = devices
            .get(*device_index)
            .map(|d| {
                d.modes
                    .iter()
                    .enumerate()
                    .map(|(i, m)| json!({"label": m.label(), "value": i}))
                    .collect()
            })
            .unwrap_or_default();
        if *format_index >= fmt_opts.len() {
            *format_index = 0;
        }
        let footer = if devices.is_empty() {
            "未检测到视频采集卡或摄像头，请插入后点击刷新设备"
        } else {
            "已检测到采集设备，点击开启采集即可低延迟预览"
        };
        emit_patch(
            out,
            json!({
                "deviceOptions": dev_opts,
                "formatOptions": fmt_opts,
                "deviceIndex": *device_index,
                "formatIndex": *format_index,
                "footerStatus": footer
            }),
        );
    };

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match msg["type"].as_str().unwrap_or("") {
            "init" => {
                frame_addr = msg["frameAddr"].as_str().unwrap_or("").to_owned();
                frame_token = msg["frameToken"].as_str().unwrap_or("").to_owned();
                match mf::list_devices() {
                    Ok(list) => {
                        devices = list;
                        push_devices(&devices, &mut device_index, &mut format_index, &out);
                    }
                    Err(e) => {
                        emit(&out, json!({"type": "error", "message": e.to_string()}));
                    }
                }
            }
            "set_state" => {
                let key = msg["key"].as_str().unwrap_or("");
                match key {
                    "deviceIndex" => {
                        device_index = msg["value"].as_u64().unwrap_or(0) as usize;
                        format_index = 0;
                        push_devices(&devices, &mut device_index, &mut format_index, &out);
                    }
                    "formatIndex" => {
                        format_index = msg["value"].as_u64().unwrap_or(0) as usize;
                    }
                    _ => {}
                }
            }
            "action" => {
                let act = msg["action"].as_str().unwrap_or("");
                match act {
                    "refresh_devices" => {
                        if active_stop.is_none() {
                            match mf::list_devices() {
                                Ok(list) => {
                                    devices = list;
                                    push_devices(
                                        &devices,
                                        &mut device_index,
                                        &mut format_index,
                                        &out,
                                    );
                                }
                                Err(e) => {
                                    emit(&out, json!({"type": "error", "message": e.to_string()}));
                                }
                            }
                        }
                    }
                    "toggle_capture" => {
                        if active_stop.is_some() {
                            stop_capture(&mut active_stop, &mut active_thread, &out);
                        } else if let Some(dev) = devices.get(device_index).cloned() {
                            let mode = dev.modes.get(format_index).cloned();
                            let stop = Arc::new(AtomicBool::new(false));
                            let stop_c = stop.clone();
                            let out_c = out.clone();
                            let addr = frame_addr.clone();
                            let tok = frame_token.clone();
                            emit_patch(
                                &out,
                                json!({
                                    "active": true,
                                    "loading": true,
                                    "status": "正在开启采集…",
                                    "statusPhase": 1,
                                    "footerStatus": format!("正在打开 {} …", dev.name)
                                }),
                            );
                            let handle = thread::spawn(move || {
                                if let Err(err) =
                                    mf::run_capture(dev.symlink, mode, addr, tok, stop_c.clone(), out_c.clone())
                                {
                                    if !stop_c.load(Ordering::Relaxed) {
                                        emit(&out_c, json!({"type": "error", "message": err.to_string()}));
                                        emit_patch(
                                            &out_c,
                                            json!({
                                                "active": false,
                                                "loading": false,
                                                "status": "采集失败",
                                                "statusPhase": 3
                                            }),
                                        );
                                    }
                                }
                            });
                            active_stop = Some(stop);
                            active_thread = Some(handle);
                        } else {
                            emit(
                                &out,
                                json!({"type": "error", "message": "未检测到可用视频采集设备。"}),
                            );
                        }
                    }
                    "rotate_cw" => {
                        rotation = (rotation + 90) % 360;
                        emit_patch(
                            &out,
                            json!({
                                "rotation": rotation,
                                "transformText": transform_summary(rotation, flip_h, flip_v)
                            }),
                        );
                    }
                    "toggle_flip_h" => {
                        flip_h = !flip_h;
                        emit_patch(
                            &out,
                            json!({
                                "flipH": flip_h,
                                "transformText": transform_summary(rotation, flip_h, flip_v)
                            }),
                        );
                    }
                    "toggle_flip_v" => {
                        flip_v = !flip_v;
                        emit_patch(
                            &out,
                            json!({
                                "flipV": flip_v,
                                "transformText": transform_summary(rotation, flip_h, flip_v)
                            }),
                        );
                    }
                    "reset_transform" => {
                        rotation = 0;
                        flip_h = false;
                        flip_v = false;
                        emit_patch(
                            &out,
                            json!({
                                "fitMode": 0,
                                "rotation": 0,
                                "flipH": false,
                                "flipV": false,
                                "transformText": "0°"
                            }),
                        );
                    }
                    _ => {}
                }
            }
            "shutdown" => {
                stop_capture(&mut active_stop, &mut active_thread, &out);
                break;
            }
            _ => {}
        }
    }
    stop_capture(&mut active_stop, &mut active_thread, &out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_conversion_handles_top_down_and_bottom_up_strides() {
        // 2x2 image: row 0 = blue(B=255,G=0,R=0), green; row 1 = red, white
        let src = vec![
            255, 0, 0, 0, 0, 255, 0, 0, // row 0
            0, 0, 255, 0, 255, 255, 255, 0, // row 1
        ];
        let mut dst = Vec::new();
        bgra_to_rgba_into(&src, 2, 2, 8, &mut dst);
        // Pixel (0,0) RGBA should be (0, 0, 255, 255)
        assert_eq!(&dst[0..4], &[0, 0, 255, 255]);
        // Pixel (0,1) RGBA should be (255, 0, 0, 255)
        assert_eq!(&dst[8..12], &[255, 0, 0, 255]);

        // Bottom-up (negative stride) flips rows
        bgra_to_rgba_into(&src, 2, 2, -8, &mut dst);
        assert_eq!(&dst[0..4], &[255, 0, 0, 255]);
        assert_eq!(&dst[8..12], &[0, 0, 255, 255]);
    }
}

//! PortCanvas Video Capture Worker (`pc-worker-v1`)
//! Native low-latency Windows MediaFoundation capture card / webcam worker.
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
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
const ALL_FORMATS_LABEL: &str = "全部格式";

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

fn format_priority(fmt: &str) -> u32 {
    match fmt {
        "NV12" => 10,
        "YUY2" => 9,
        "RGB24" => 8,
        "RGB32" | "ARGB32" => 7,
        "UYVY" => 6,
        "I420" | "IYUV" => 5,
        "P010" => 4,
        "MJPEG" => 3,
        "H264" => 2,
        "HEVC" => 1,
        _ => 0,
    }
}

fn subtype_label_from_parts(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> String {
    const MF_VIDEO_TAIL: [u8; 8] = [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71];
    if data2 == 0x0000 && data3 == 0x0010 && data4 == MF_VIDEO_TAIL {
        match data1 {
            20 => return "RGB24".into(),
            21 => return "ARGB32".into(),
            22 => return "RGB32".into(),
            23 => return "RGB565".into(),
            24 => return "RGB555".into(),
            _ => {}
        }
        let bytes = data1.to_le_bytes();
        if bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b' ') {
            let fourcc = String::from_utf8_lossy(&bytes).trim().to_ascii_uppercase();
            return match fourcc.as_str() {
                "MJPG" => "MJPEG".into(),
                "IYUV" => "I420".into(),
                other if !other.is_empty() => other.to_owned(),
                _ => format!("{data1:08X}"),
            };
        }
    }
    format!("{data1:08X}")
}

fn distinct_formats(modes: &[CaptureMode]) -> Vec<String> {
    let mut unique: BTreeSet<String> = BTreeSet::new();
    for m in modes {
        unique.insert(m.format_name.clone());
    }
    let mut list: Vec<String> = unique.into_iter().collect();
    list.sort_by(|a, b| format_priority(b).cmp(&format_priority(a)).then_with(|| a.cmp(b)));
    list
}

fn filter_modes<'a>(modes: &'a [CaptureMode], pixel_format_filter: Option<&str>) -> Vec<&'a CaptureMode> {
    match pixel_format_filter {
        Some(fmt) => modes.iter().filter(|m| m.format_name == fmt).collect(),
        None => modes.iter().collect(),
    }
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

#[inline]
fn yuv_to_rgb(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let c = (y as i32) - 16;
    let d = (u as i32) - 128;
    let e = (v as i32) - 128;
    let r = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8;
    let g = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
    let b = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8;
    (r, g, b)
}

fn bgra_to_rgba_into(src: &[u8], width: usize, height: usize, stride: isize, dst: &mut Vec<u8>) {
    dst.resize(width * height * 4, 255);
    let abs_stride = stride.unsigned_abs().max(width * 4);
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
            drow[si] = srow[si + 2];
            drow[si + 1] = srow[si + 1];
            drow[si + 2] = srow[si];
            drow[si + 3] = 255;
        }
    }
}

fn bgr24_to_rgba_into(src: &[u8], width: usize, height: usize, stride: isize, dst: &mut Vec<u8>) {
    dst.resize(width * height * 4, 255);
    let abs_stride = stride.unsigned_abs().max(width * 3);
    let bottom_up = stride < 0;
    for y in 0..height {
        let src_row_idx = if bottom_up { height - 1 - y } else { y };
        let src_start = src_row_idx * abs_stride;
        if src_start + width * 3 > src.len() {
            break;
        }
        let srow = &src[src_start..src_start + width * 3];
        let drow = &mut dst[y * width * 4..(y + 1) * width * 4];
        for x in 0..width {
            let si = x * 3;
            let di = x * 4;
            drow[di] = srow[si + 2];
            drow[di + 1] = srow[si + 1];
            drow[di + 2] = srow[si];
            drow[di + 3] = 255;
        }
    }
}

fn yuy2_to_rgba_into(src: &[u8], width: usize, height: usize, stride: isize, dst: &mut Vec<u8>) {
    dst.resize(width * height * 4, 255);
    let abs_stride = stride.unsigned_abs().max(width * 2);
    for y in 0..height {
        let src_start = y * abs_stride;
        if src_start + width * 2 > src.len() {
            break;
        }
        let srow = &src[src_start..src_start + width * 2];
        let drow = &mut dst[y * width * 4..(y + 1) * width * 4];
        let pairs = width / 2;
        for p in 0..pairs {
            let si = p * 4;
            let y0 = srow[si];
            let u = srow[si + 1];
            let y1 = srow[si + 2];
            let v = srow[si + 3];
            let (r0, g0, b0) = yuv_to_rgb(y0, u, v);
            let (r1, g1, b1) = yuv_to_rgb(y1, u, v);
            let di = p * 8;
            drow[di] = r0;
            drow[di + 1] = g0;
            drow[di + 2] = b0;
            drow[di + 3] = 255;
            drow[di + 4] = r1;
            drow[di + 5] = g1;
            drow[di + 6] = b1;
            drow[di + 7] = 255;
        }
    }
}

fn uyvy_to_rgba_into(src: &[u8], width: usize, height: usize, stride: isize, dst: &mut Vec<u8>) {
    dst.resize(width * height * 4, 255);
    let abs_stride = stride.unsigned_abs().max(width * 2);
    for y in 0..height {
        let src_start = y * abs_stride;
        if src_start + width * 2 > src.len() {
            break;
        }
        let srow = &src[src_start..src_start + width * 2];
        let drow = &mut dst[y * width * 4..(y + 1) * width * 4];
        let pairs = width / 2;
        for p in 0..pairs {
            let si = p * 4;
            let u = srow[si];
            let y0 = srow[si + 1];
            let v = srow[si + 2];
            let y1 = srow[si + 3];
            let (r0, g0, b0) = yuv_to_rgb(y0, u, v);
            let (r1, g1, b1) = yuv_to_rgb(y1, u, v);
            let di = p * 8;
            drow[di] = r0;
            drow[di + 1] = g0;
            drow[di + 2] = b0;
            drow[di + 3] = 255;
            drow[di + 4] = r1;
            drow[di + 5] = g1;
            drow[di + 6] = b1;
            drow[di + 7] = 255;
        }
    }
}

fn nv12_to_rgba_into(src: &[u8], width: usize, height: usize, stride: isize, dst: &mut Vec<u8>) {
    dst.resize(width * height * 4, 255);
    let y_stride = stride.unsigned_abs().max(width);
    let uv_offset = y_stride * height;
    if uv_offset + y_stride * (height / 2) > src.len() {
        return;
    }
    for y in 0..height {
        let y_row = &src[y * y_stride..y * y_stride + width];
        let uv_row_start = uv_offset + (y / 2) * y_stride;
        let uv_row = &src[uv_row_start..uv_row_start + width];
        let drow = &mut dst[y * width * 4..(y + 1) * width * 4];
        for x in 0..width {
            let u = uv_row[(x & !1).min(width.saturating_sub(2))];
            let v = uv_row[((x & !1) + 1).min(width.saturating_sub(1))];
            let (r, g, b) = yuv_to_rgb(y_row[x], u, v);
            let di = x * 4;
            drow[di] = r;
            drow[di + 1] = g;
            drow[di + 2] = b;
            drow[di + 3] = 255;
        }
    }
}

fn i420_to_rgba_into(src: &[u8], width: usize, height: usize, stride: isize, dst: &mut Vec<u8>) {
    dst.resize(width * height * 4, 255);
    let y_stride = stride.unsigned_abs().max(width);
    let uv_stride = (y_stride / 2).max(width / 2);
    let u_offset = y_stride * height;
    let v_offset = u_offset + uv_stride * (height / 2);
    if v_offset + uv_stride * (height / 2) > src.len() {
        return;
    }
    for y in 0..height {
        let y_row = &src[y * y_stride..y * y_stride + width];
        let u_row = &src[u_offset + (y / 2) * uv_stride..];
        let v_row = &src[v_offset + (y / 2) * uv_stride..];
        let drow = &mut dst[y * width * 4..(y + 1) * width * 4];
        for x in 0..width {
            let cx = x / 2;
            let (r, g, b) = yuv_to_rgb(y_row[x], u_row[cx], v_row[cx]);
            let di = x * 4;
            drow[di] = r;
            drow[di + 1] = g;
            drow[di + 2] = b;
            drow[di + 3] = 255;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Bgra32,
    Bgr24,
    Yuy2,
    Uyvy,
    Nv12,
    I420,
}

fn convert_frame_to_rgba(
    fmt: OutputFormat,
    src: &[u8],
    width: usize,
    height: usize,
    stride: isize,
    dst: &mut Vec<u8>,
) {
    match fmt {
        OutputFormat::Bgra32 => bgra_to_rgba_into(src, width, height, stride, dst),
        OutputFormat::Bgr24 => bgr24_to_rgba_into(src, width, height, stride, dst),
        OutputFormat::Yuy2 => yuy2_to_rgba_into(src, width, height, stride, dst),
        OutputFormat::Uyvy => uyvy_to_rgba_into(src, width, height, stride, dst),
        OutputFormat::Nv12 => nv12_to_rgba_into(src, width, height, stride, dst),
        OutputFormat::I420 => i420_to_rgba_into(src, width, height, stride, dst),
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
                MFEnumDeviceSources, MFShutdown, MFStartup, MFVideoFormat_ARGB32,
                MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_I420, MFVideoFormat_IYUV,
                MFVideoFormat_MJPG, MFVideoFormat_NV12, MFVideoFormat_P010, MFVideoFormat_RGB24,
                MFVideoFormat_RGB32, MFVideoFormat_RGB555, MFVideoFormat_RGB565,
                MFVideoFormat_UYVY, MFVideoFormat_YUY2, MFVideoFormat_YV12, MFMediaType_Video,
                MFSTARTUP_NOSOCKET, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_LOW_LATENCY,
                MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
                MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
                MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
                MF_VERSION,
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
        } else if guid == MFVideoFormat_UYVY {
            "UYVY".into()
        } else if guid == MFVideoFormat_I420 || guid == MFVideoFormat_IYUV {
            "I420".into()
        } else if guid == MFVideoFormat_YV12 {
            "YV12".into()
        } else if guid == MFVideoFormat_RGB32 {
            "RGB32".into()
        } else if guid == MFVideoFormat_ARGB32 {
            "ARGB32".into()
        } else if guid == MFVideoFormat_RGB24 {
            "RGB24".into()
        } else if guid == MFVideoFormat_RGB565 {
            "RGB565".into()
        } else if guid == MFVideoFormat_RGB555 {
            "RGB555".into()
        } else if guid == MFVideoFormat_H264 {
            "H264".into()
        } else if guid == MFVideoFormat_HEVC {
            "HEVC".into()
        } else if guid == MFVideoFormat_P010 {
            "P010".into()
        } else {
            subtype_label_from_parts(guid.data1, guid.data2, guid.data3, guid.data4)
        }
    }

    fn output_format_for_guid(guid: GUID) -> Option<OutputFormat> {
        if guid == MFVideoFormat_RGB32 || guid == MFVideoFormat_ARGB32 {
            Some(OutputFormat::Bgra32)
        } else if guid == MFVideoFormat_RGB24 {
            Some(OutputFormat::Bgr24)
        } else if guid == MFVideoFormat_YUY2 {
            Some(OutputFormat::Yuy2)
        } else if guid == MFVideoFormat_UYVY {
            Some(OutputFormat::Uyvy)
        } else if guid == MFVideoFormat_NV12 {
            Some(OutputFormat::Nv12)
        } else if guid == MFVideoFormat_I420 || guid == MFVideoFormat_IYUV {
            Some(OutputFormat::I420)
        } else {
            None
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
        let mut seen: BTreeMap<(u32, u32, u32, String), CaptureMode> = BTreeMap::new();
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            if MFCreateAttributes(&mut attrs, 2).is_err() {
                return Vec::new();
            }
            let Some(attrs) = attrs else {
                return Vec::new();
            };
            let _ = attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
            let _ = attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
            let Ok(reader) = MFCreateSourceReaderFromMediaSource(source, &attrs) else {
                return Vec::new();
            };
            for idx in 0..512u32 {
                let Ok(mt) =
                    reader.GetNativeMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, idx)
                else {
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
                let key = (width, height, fps_int, fmt.clone());
                seen.entry(key).or_insert(CaptureMode {
                    width,
                    height,
                    fps_num,
                    fps_den,
                    format_name: fmt,
                    native_index: idx,
                });
            }
        }
        let mut modes: Vec<CaptureMode> = seen.into_values().collect();
        modes.sort_by(|a, b| {
            let rank = |m: &CaptureMode| -> (u32, u64, u32) {
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
                (
                    preferred,
                    (m.width as u64) * (m.height as u64) * (fps as u64),
                    format_priority(&m.format_name),
                )
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
            MFCreateAttributes(&mut attrs, 4)?;
            let attrs = attrs.ok_or_else(|| anyhow!("MFCreateAttributes failed"))?;
            let _ = attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
            let _ = attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
            MFCreateSourceReaderFromMediaSource(&source, &attrs)?
        };

        let stream_idx = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let fmt_label = mode
            .as_ref()
            .map(|m| m.format_name.clone())
            .unwrap_or_else(|| "AUTO".into());

        let mut output_fmt = OutputFormat::Bgra32;
        unsafe {
            let mut native_mt = None;
            if let Some(m) = &mode {
                if let Ok(native) = reader.GetNativeMediaType(stream_idx, m.native_index) {
                    let _ = reader.SetCurrentMediaType(stream_idx, None, &native);
                    native_mt = Some(native);
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
            if reader.SetCurrentMediaType(stream_idx, None, &out_type).is_err() {
                // Some MediaFoundation color converters reject explicit frame-rate overrides on output types.
                let simple_rgb: IMFMediaType =
                    windows::Win32::Media::MediaFoundation::MFCreateMediaType()?;
                simple_rgb.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                simple_rgb.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
                if reader.SetCurrentMediaType(stream_idx, None, &simple_rgb).is_err() {
                    if let Some(native) = &native_mt {
                        let native_sub = native.GetGUID(&MF_MT_SUBTYPE).unwrap_or_default();
                        if let Some(direct) = output_format_for_guid(native_sub) {
                            reader.SetCurrentMediaType(stream_idx, None, native)?;
                            output_fmt = direct;
                        } else {
                            return Err(anyhow!(
                                "设备格式 {fmt_label} 无法转换为 RGB32，请切换其他采集格式。"
                            ));
                        }
                    } else {
                        return Err(anyhow!("无法配置视频采集输出格式（RGB32）。"));
                    }
                }
            }
        }

        let current_mt = unsafe { reader.GetCurrentMediaType(stream_idx)? };
        let packed_size = unsafe { current_mt.GetUINT64(&MF_MT_FRAME_SIZE)? };
        let mut width = (packed_size >> 32) as u32;
        let mut height = (packed_size & 0xFFFF_FFFF) as u32;
        let fallback_stride = match output_fmt {
            OutputFormat::Bgra32 => (width * 4) as isize,
            OutputFormat::Bgr24 => (width * 3) as isize,
            OutputFormat::Yuy2 | OutputFormat::Uyvy => (width * 2) as isize,
            OutputFormat::Nv12 | OutputFormat::I420 => width as isize,
        };
        let mut default_stride = unsafe {
            current_mt
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .map(|v| v as i32 as isize)
                .unwrap_or(fallback_stride)
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
                "footerStatus": format!("实时采集中 · {width}×{height} ({fmt_label})")
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

            let copied = if matches!(output_fmt, OutputFormat::Bgra32 | OutputFormat::Bgr24) {
                if let Ok(buf2d) = buffer.cast::<IMF2DBuffer>() {
                    let mut scan0: *mut u8 = std::ptr::null_mut();
                    let mut pitch = 0i32;
                    if unsafe { buf2d.Lock2D(&mut scan0, &mut pitch) }.is_ok() && !scan0.is_null() {
                        let abs_pitch = pitch.unsigned_abs() as usize;
                        let total = abs_pitch * (height as usize);
                        let src = if pitch >= 0 {
                            unsafe { std::slice::from_raw_parts(scan0, total) }
                        } else {
                            let start = unsafe {
                                scan0.sub(abs_pitch * ((height as usize).saturating_sub(1)))
                            };
                            unsafe { std::slice::from_raw_parts(start, total) }
                        };
                        convert_frame_to_rgba(
                            output_fmt,
                            src,
                            width as usize,
                            height as usize,
                            pitch as isize,
                            &mut rgba,
                        );
                        unsafe {
                            let _ = buf2d.Unlock2D();
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    lock_and_convert(&buffer, output_fmt, width, height, default_stride, &mut rgba)
                }
            } else {
                lock_and_convert(&buffer, output_fmt, width, height, default_stride, &mut rgba)
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
                            "footerStatus": format!("实时采集中 · {width}×{height} @ {fps:.1} FPS ({fmt_label})")
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

    fn lock_and_convert(
        buffer: &windows::Win32::Media::MediaFoundation::IMFMediaBuffer,
        output_fmt: OutputFormat,
        width: u32,
        height: u32,
        stride: isize,
        rgba: &mut Vec<u8>,
    ) -> bool {
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut cur_len = 0u32;
        if unsafe { buffer.Lock(&mut ptr, None, Some(&mut cur_len)) }.is_ok() && !ptr.is_null() {
            let src = unsafe { std::slice::from_raw_parts(ptr, cur_len as usize) };
            convert_frame_to_rgba(
                output_fmt,
                src,
                width as usize,
                height as usize,
                stride,
                rgba,
            );
            unsafe {
                let _ = buffer.Unlock();
            }
            true
        } else {
            false
        }
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
    let mut pixel_format_index: usize = 0;
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
                        pixel_format_index: &mut usize,
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

        let (px_names, filtered_modes): (Vec<String>, Vec<CaptureMode>) = devices
            .get(*device_index)
            .map(|d| {
                let fmts = distinct_formats(&d.modes);
                if *pixel_format_index > fmts.len() {
                    *pixel_format_index = 0;
                }
                let active_filter = if *pixel_format_index == 0 {
                    None
                } else {
                    fmts.get(*pixel_format_index - 1).map(String::as_str)
                };
                let modes: Vec<CaptureMode> = filter_modes(&d.modes, active_filter)
                    .into_iter()
                    .cloned()
                    .collect();
                (fmts, modes)
            })
            .unwrap_or_default();

        let mut px_opts: Vec<Value> = vec![json!({"label": ALL_FORMATS_LABEL, "value": 0})];
        for (idx, name) in px_names.iter().enumerate() {
            px_opts.push(json!({"label": name, "value": idx + 1}));
        }
        let fmt_opts: Vec<Value> = filtered_modes
            .iter()
            .enumerate()
            .map(|(i, m)| json!({"label": m.label(), "value": i}))
            .collect();
        if *format_index >= fmt_opts.len() {
            *format_index = 0;
        }
        let footer = if devices.is_empty() {
            "未检测到视频采集卡或摄像头，请插入后点击刷新设备".to_owned()
        } else if !px_names.is_empty() {
            format!("已检测到采集设备（支持格式：{}），点击开启采集即可预览", px_names.join(" / "))
        } else {
            "已检测到采集设备，点击开启采集即可低延迟预览".to_owned()
        };
        emit_patch(
            out,
            json!({
                "deviceOptions": dev_opts,
                "pixelFormatOptions": px_opts,
                "formatOptions": fmt_opts,
                "deviceIndex": *device_index,
                "pixelFormatIndex": *pixel_format_index,
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
                        push_devices(
                            &devices,
                            &mut device_index,
                            &mut pixel_format_index,
                            &mut format_index,
                            &out,
                        );
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
                        pixel_format_index = 0;
                        format_index = 0;
                        push_devices(
                            &devices,
                            &mut device_index,
                            &mut pixel_format_index,
                            &mut format_index,
                            &out,
                        );
                    }
                    "pixelFormatIndex" => {
                        pixel_format_index = msg["value"].as_u64().unwrap_or(0) as usize;
                        format_index = 0;
                        push_devices(
                            &devices,
                            &mut device_index,
                            &mut pixel_format_index,
                            &mut format_index,
                            &out,
                        );
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
                                        &mut pixel_format_index,
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
                            let fmts = distinct_formats(&dev.modes);
                            let active_filter = if pixel_format_index == 0 {
                                None
                            } else {
                                fmts.get(pixel_format_index - 1).map(String::as_str)
                            };
                            let filtered = filter_modes(&dev.modes, active_filter);
                            let mode = filtered.get(format_index).copied().cloned();
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
                                if let Err(err) = mf::run_capture(
                                    dev.symlink,
                                    mode,
                                    addr,
                                    tok,
                                    stop_c.clone(),
                                    out_c.clone(),
                                ) {
                                    if !stop_c.load(Ordering::Relaxed) {
                                        emit(
                                            &out_c,
                                            json!({"type": "error", "message": err.to_string()}),
                                        );
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
        let src = vec![
            10, 20, 30, 0, 40, 50, 60, 0, // row 0
            70, 80, 90, 0, 100, 110, 120, 0, // row 1
        ];
        let mut dst = Vec::new();
        bgra_to_rgba_into(&src, 2, 2, 8, &mut dst);
        assert_eq!(
            dst,
            vec![30, 20, 10, 255, 60, 50, 40, 255, 90, 80, 70, 255, 120, 110, 100, 255]
        );
        bgra_to_rgba_into(&src, 2, 2, -8, &mut dst);
        assert_eq!(
            dst,
            vec![90, 80, 70, 255, 120, 110, 100, 255, 30, 20, 10, 255, 60, 50, 40, 255]
        );
    }

    #[test]
    fn fourcc_and_rgb_subtypes_decode_cleanly() {
        let tail = [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71];
        assert_eq!(
            subtype_label_from_parts(u32::from_le_bytes(*b"YUY2"), 0, 0x10, tail),
            "YUY2"
        );
        assert_eq!(
            subtype_label_from_parts(u32::from_le_bytes(*b"NV12"), 0, 0x10, tail),
            "NV12"
        );
        assert_eq!(
            subtype_label_from_parts(u32::from_le_bytes(*b"MJPG"), 0, 0x10, tail),
            "MJPEG"
        );
        assert_eq!(
            subtype_label_from_parts(u32::from_le_bytes(*b"IYUV"), 0, 0x10, tail),
            "I420"
        );
        assert_eq!(subtype_label_from_parts(20, 0, 0x10, tail), "RGB24");
        assert_eq!(subtype_label_from_parts(22, 0, 0x10, tail), "RGB32");
    }

    #[test]
    fn pixel_converters_output_expected_dimensions_and_neutral_gray() {
        let mut dst = Vec::new();
        // Neutral mid-gray: Y=128, U=128, V=128
        let yuy2 = vec![128, 128, 128, 128, 128, 128, 128, 128];
        yuy2_to_rgba_into(&yuy2, 2, 2, 4, &mut dst);
        assert_eq!(dst.len(), 16);
        assert!((dst[0] as i32 - 130).abs() <= 3 && dst[3] == 255);

        let uyvy = vec![128, 128, 128, 128, 128, 128, 128, 128];
        uyvy_to_rgba_into(&uyvy, 2, 2, 4, &mut dst);
        assert_eq!(dst.len(), 16);
        assert!((dst[0] as i32 - 130).abs() <= 3 && dst[3] == 255);

        let nv12 = vec![128, 128, 128, 128, 128, 128];
        nv12_to_rgba_into(&nv12, 2, 2, 2, &mut dst);
        assert_eq!(dst.len(), 16);
        assert!((dst[0] as i32 - 130).abs() <= 3 && dst[3] == 255);

        let i420 = vec![128, 128, 128, 128, 128, 128];
        i420_to_rgba_into(&i420, 2, 2, 2, &mut dst);
        assert_eq!(dst.len(), 16);
        assert!((dst[0] as i32 - 130).abs() <= 3 && dst[3] == 255);

        let bgr24 = vec![10, 20, 30, 40, 50, 60];
        bgr24_to_rgba_into(&bgr24, 2, 1, 6, &mut dst);
        assert_eq!(dst, vec![30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn distinct_formats_preserves_and_filters_all_formats() {
        let modes = vec![
            CaptureMode {
                width: 1920,
                height: 1080,
                fps_num: 60,
                fps_den: 1,
                format_name: "MJPEG".into(),
                native_index: 0,
            },
            CaptureMode {
                width: 1920,
                height: 1080,
                fps_num: 60,
                fps_den: 1,
                format_name: "YUY2".into(),
                native_index: 1,
            },
            CaptureMode {
                width: 1920,
                height: 1080,
                fps_num: 60,
                fps_den: 1,
                format_name: "NV12".into(),
                native_index: 2,
            },
        ];
        assert_eq!(distinct_formats(&modes), vec!["NV12", "YUY2", "MJPEG"]);
        assert_eq!(filter_modes(&modes, None).len(), 3);
        assert_eq!(filter_modes(&modes, Some("YUY2")).len(), 1);
    }
}

//! Windows Media Foundation feasibility probe; uses system codecs.
use anyhow::{ensure, Result};
use std::path::Path;
use windows::{
    core::PCWSTR,
    Win32::{Media::MediaFoundation::*, System::Com::*},
};

struct Environment;
impl Environment {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                CoUninitialize();
                return Err(error.into());
            }
        }
        Ok(Self)
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub timestamp_100ns: i64,
    pub rgba: Vec<u8>,
}
pub struct Reader {
    reader: IMFSourceReader,
    format: Format,
    _environment: Environment,
}
struct Format {
    width: u32,
    height: u32,
    coded_height: u32,
    stride: i32,
    x: u32,
    y: u32,
}
unsafe fn dimensions(reader: &IMFSourceReader) -> Result<Format> {
    let actual = reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?;
    ensure!(
        actual.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_RGB32,
        "Source reader changed away from RGB32"
    );
    let size = actual.GetUINT64(&MF_MT_FRAME_SIZE)?;
    let (width, height) = ((size >> 32) as u32, size as u32);
    ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) * 4 <= 128 * 1024 * 1024,
        "Video dimensions exceed probe budget"
    );
    let stride = actual
        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
        .map(|n| n as i32)
        .unwrap_or((width * 4) as i32);
    let mut format = Format {
        width,
        height,
        coded_height: height,
        stride,
        x: 0,
        y: 0,
    };
    for key in [MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_GEOMETRIC_APERTURE] {
        if let Ok(size) = actual.GetBlobSize(&key) {
            ensure!(
                size as usize == std::mem::size_of::<MFVideoArea>(),
                "Invalid video display aperture"
            );
            let mut bytes = vec![0u8; size as usize];
            actual.GetBlob(&key, &mut bytes, None)?;
            let area = std::ptr::read_unaligned(bytes.as_ptr() as *const MFVideoArea);
            ensure!(
                area.OffsetX.value >= 0
                    && area.OffsetY.value >= 0
                    && area.OffsetX.fract == 0
                    && area.OffsetY.fract == 0
                    && area.Area.cx > 0
                    && area.Area.cy > 0,
                "Unsupported video display aperture"
            );
            let (x, y, w, h) = (
                area.OffsetX.value as u32,
                area.OffsetY.value as u32,
                area.Area.cx as u32,
                area.Area.cy as u32,
            );
            ensure!(
                x + w <= width && y + h <= height,
                "Video aperture outside decoded frame"
            );
            format.x = x;
            format.y = y;
            format.width = w;
            format.height = h;
            break;
        }
    }
    Ok(format)
}
impl Reader {
    pub fn open(path: &Path) -> Result<Self> {
        let environment = Environment::new()?;
        unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 1)?;
            let attributes = attributes.unwrap();
            attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
            let path = wide(path);
            let reader = MFCreateSourceReaderFromURL(PCWSTR(path.as_ptr()), &attributes)?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
            let media_type = MFCreateMediaType()?;
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                None,
                &media_type,
            )?;
            let format = dimensions(&reader)?;
            Ok(Self {
                reader,
                format,
                _environment: environment,
            })
        }
    }
    pub fn next_frame(&mut self) -> Result<Option<VideoFrame>> {
        unsafe {
            loop {
                let mut flags = 0;
                let mut timestamp = 0;
                let mut sample = None;
                self.reader.ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut timestamp),
                    Some(&mut sample),
                )?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    return Ok(None);
                }
                if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                    self.format = dimensions(&self.reader)?;
                }
                let Some(sample) = sample else { continue };
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut pointer = std::ptr::null_mut();
                let mut length = 0;
                buffer.Lock(&mut pointer, None, Some(&mut length))?;
                let result = (|| {
                    let format = &self.format;
                    let stride = format.stride.unsigned_abs() as usize;
                    ensure!(
                        stride >= (format.x + format.width) as usize * 4
                            && length as usize >= stride * format.coded_height as usize,
                        "Decoded video buffer is incomplete"
                    );
                    let bytes = std::slice::from_raw_parts(pointer, length as usize);
                    let mut rgba = vec![0; format.width as usize * format.height as usize * 4];
                    for row in 0..format.height as usize {
                        let line = row + format.y as usize;
                        let source_row = if format.stride < 0 {
                            format.coded_height as usize - 1 - line
                        } else {
                            line
                        };
                        for column in 0..format.width as usize {
                            let input = source_row * stride + (column + format.x as usize) * 4;
                            let output = (row * format.width as usize + column) * 4;
                            rgba[output..output + 4].copy_from_slice(&[
                                bytes[input + 2],
                                bytes[input + 1],
                                bytes[input],
                                255,
                            ]);
                        }
                    }
                    Ok(VideoFrame {
                        width: format.width,
                        height: format.height,
                        timestamp_100ns: timestamp,
                        rgba,
                    })
                })();
                buffer.Unlock()?;
                return result.map(Some);
            }
        }
    }
}

/// Generate non-private moving pixels using the Windows H.264 encoder, not a downloaded fixture.
pub fn write_fixture(path: &Path) -> Result<()> {
    ensure!(!path.exists(), "Fixture output already exists");
    let _environment = Environment::new()?;
    unsafe {
        let path = wide(path);
        let writer = MFCreateSinkWriterFromURL(PCWSTR(path.as_ptr()), None, None)?;
        let output = MFCreateMediaType()?;
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        output.SetUINT32(&MF_MT_AVG_BITRATE, 800_000)?;
        output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        output.SetUINT64(&MF_MT_FRAME_SIZE, (320u64 << 32) | 180)?;
        output.SetUINT64(&MF_MT_FRAME_RATE, (30u64 << 32) | 1)?;
        output.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
        let stream = writer.AddStream(&output)?;
        let input = MFCreateMediaType()?;
        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        input.SetUINT64(&MF_MT_FRAME_SIZE, (320u64 << 32) | 180)?;
        input.SetUINT64(&MF_MT_FRAME_RATE, (30u64 << 32) | 1)?;
        input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
        writer.SetInputMediaType(stream, &input, None)?;
        writer.BeginWriting()?;
        for frame in 0..60 {
            let buffer = MFCreateMemoryBuffer(320 * 180 * 4)?;
            let mut pointer = std::ptr::null_mut();
            buffer.Lock(&mut pointer, None, None)?;
            let bytes = std::slice::from_raw_parts_mut(pointer, 320 * 180 * 4);
            for (index, pixel) in bytes.chunks_exact_mut(4).enumerate() {
                let x = index % 320;
                let red = frame < 30;
                let moving = (x as i32 - frame * 4).abs() < 12;
                pixel.copy_from_slice(&[
                    if red { 20 } else { 210 },
                    if moving { 220 } else { 40 },
                    if red { 210 } else { 20 },
                    255,
                ]);
            }
            buffer.Unlock()?;
            buffer.SetCurrentLength(320 * 180 * 4)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(i64::from(frame) * 10_000_000 / 30)?;
            sample.SetSampleDuration(10_000_000 / 30)?;
            writer.WriteSample(stream, &sample)?;
        }
        writer.Finalize()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_h264_encode_decode_and_eof_release() -> Result<()> {
        let directory = std::path::PathBuf::from(
            std::env::var_os("WCS_TEST_ROOT").expect("isolated WCS_TEST_ROOT"),
        )
        .join(format!("media-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let path = directory.join("synthetic.mp4");
        write_fixture(&path)?;
        let mut frames = Vec::new();
        {
            let mut reader = Reader::open(&path)?;
            while let Some(frame) = reader.next_frame()? {
                assert_eq!((frame.width, frame.height), (320, 180));
                frames.push((frame.timestamp_100ns, frame.rgba[0], frame.rgba[2]));
            }
        }
        assert_eq!(frames.len(), 60);
        assert!(frames.windows(2).all(|p| p[0].0 < p[1].0));
        assert!(frames[0].1 > 180 && frames[0].2 < 60);
        assert!(frames[59].2 > 180 && frames[59].1 < 60);
        std::fs::remove_file(path)?;
        std::fs::remove_dir(directory)?;
        Ok(())
    }
}

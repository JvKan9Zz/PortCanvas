//! AVFoundation video reader. One owned RGBA frame; files are decoded incrementally.
use anyhow::{ensure,Result};
use std::{ffi::{c_char,c_void,CStr,CString},path::Path,ptr::NonNull};
extern "C" {
    fn pc_video_open(path:*const c_char,error:*mut c_char)->*mut c_void;
    fn pc_video_audio(reader:*mut c_void)->i32;
    fn pc_video_rewind(reader:*mut c_void,error:*mut c_char)->i32;
    fn pc_video_next(reader:*mut c_void,pixels:*mut *mut u8,width:*mut u32,height:*mut u32,timestamp:*mut i64,duration:*mut i64,error:*mut c_char)->i32;
    fn pc_video_close(reader:*mut c_void);
    fn pc_media_fixture(path:*const c_char,audio:i32,error:*mut c_char)->i32;
}
pub(crate) fn path_string(path:&Path)->Result<CString>{use std::os::unix::ffi::OsStrExt;Ok(CString::new(path.as_os_str().as_bytes())?)}
pub(crate) fn failure(error:&[c_char;1024])->anyhow::Error {anyhow::anyhow!("{}",unsafe{CStr::from_ptr(error.as_ptr())}.to_string_lossy())}
pub struct VideoFrame {pub width:u32,pub height:u32,pub timestamp_100ns:i64,pub duration_100ns:Option<i64>,pub rgba:Vec<u8>}
pub struct Reader(NonNull<c_void>);
impl Reader {
    pub fn open(path:&Path)->Result<Self>{let path=path_string(path)?;let mut error=[0;1024];NonNull::new(unsafe{pc_video_open(path.as_ptr(),error.as_mut_ptr())}).map(Self).ok_or_else(||failure(&error))}
    pub fn has_audio(&self)->bool{unsafe{pc_video_audio(self.0.as_ptr())!=0}}
    pub fn rewind(&mut self)->Result<()>{let mut error=[0;1024];ensure!(unsafe{pc_video_rewind(self.0.as_ptr(),error.as_mut_ptr())}!=0,"{}",failure(&error));Ok(())}
    pub fn next_frame(&mut self)->Result<Option<VideoFrame>>{
        let (mut data,mut width,mut height,mut timestamp,mut duration)=(std::ptr::null_mut(),0,0,0,0);let mut error=[0;1024];
        match unsafe{pc_video_next(self.0.as_ptr(),&mut data,&mut width,&mut height,&mut timestamp,&mut duration,error.as_mut_ptr())}{
            0=>Ok(None),1=>{let rgba=unsafe{let bytes=std::slice::from_raw_parts(data,width as usize*height as usize*4).to_vec();libc::free(data.cast());bytes};Ok(Some(VideoFrame{width,height,timestamp_100ns:timestamp,duration_100ns:(duration>0).then_some(duration),rgba}))},_=>Err(failure(&error))
        }
    }
}
impl Drop for Reader{fn drop(&mut self){unsafe{pc_video_close(self.0.as_ptr())}}}
pub fn write_fixture(path:&Path)->Result<()>{fixture(path,false)}
pub fn write_av_fixture(path:&Path)->Result<()>{fixture(path,true)}
fn fixture(path:&Path,audio:bool)->Result<()>{ensure!(!path.exists(),"Fixture output already exists");let path=path_string(path)?;let mut error=[0;1024];ensure!(unsafe{pc_media_fixture(path.as_ptr(),audio as i32,error.as_mut_ptr())}!=0,"{}",failure(&error));Ok(())}

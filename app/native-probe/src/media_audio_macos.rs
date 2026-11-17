//! File-backed AVAudioPlayer output with a per-player volume, no application mixer.
use crate::media::{path_string,failure};
use anyhow::{ensure,Result};
use std::{cell::Cell,ffi::{c_void,c_char},path::Path,ptr::NonNull,sync::{Arc,atomic::{AtomicBool,Ordering}}};
extern "C" {
    fn pc_audio_open(path:*const c_char,position:i64,volume:f32,error:*mut c_char)->*mut c_void;
    fn pc_audio_close(value:*mut c_void);fn pc_audio_time(value:*mut c_void)->i64;fn pc_audio_ended(value:*mut c_void)->i32;
    fn pc_audio_pause(value:*mut c_void);fn pc_audio_play(value:*mut c_void)->i32;fn pc_audio_seek(value:*mut c_void,position:i64)->i32;
    fn pc_audio_volume(value:*mut c_void,volume:f32);fn pc_audio_get_volume(value:*mut c_void)->f32;
}
pub struct Audio{handle:NonNull<c_void>,cancel:Arc<AtomicBool>,ended:Cell<bool>,paused:Cell<bool>}
impl Audio {
    pub fn open(path:&Path,position:i64,level:u8,cancel:Arc<AtomicBool>)->Result<Option<Self>>{
        ensure!(!cancel.load(Ordering::SeqCst),"音频播放已取消。");
        if !crate::media::Reader::open(path)?.has_audio(){return Ok(None);}
        let path=path_string(path)?;let mut error=[0;1024];let handle=NonNull::new(unsafe{pc_audio_open(path.as_ptr(),position,level.min(100) as f32/100.,error.as_mut_ptr())}).ok_or_else(||failure(&error))?;
        Ok(Some(Self{handle,cancel,ended:Cell::new(false),paused:Cell::new(false)}))
    }
    pub fn pump(&self)->Result<bool>{ensure!(!self.cancel.load(Ordering::SeqCst),"音频播放已取消。");let ended=!self.paused.get()&&unsafe{pc_audio_ended(self.handle.as_ptr())}!=0;Ok(ended&&!self.ended.replace(ended))}
    pub fn ended(&self)->bool{self.ended.get()}
    pub fn time(&self)->Result<i64>{Ok(unsafe{pc_audio_time(self.handle.as_ptr())})}
    pub fn pause(&self)->Result<()>{if !self.ended(){unsafe{pc_audio_pause(self.handle.as_ptr())};self.paused.set(true);}Ok(())}
    pub fn resume(&self)->Result<()>{if !self.ended(){ensure!(unsafe{pc_audio_play(self.handle.as_ptr())}!=0,"音频输出无法恢复。");self.paused.set(false);}Ok(())}
    pub fn seek(&self,position:i64)->Result<()>{ensure!(unsafe{pc_audio_seek(self.handle.as_ptr(),position.max(0))}!=0,"音频定位失败。");self.ended.set(false);self.paused.set(false);Ok(())}
    pub fn set_volume(&self,level:u8)->Result<()>{unsafe{pc_audio_volume(self.handle.as_ptr(),level.min(100) as f32/100.)};Ok(())}
    pub fn volume(&self)->Result<f32>{Ok(unsafe{pc_audio_get_volume(self.handle.as_ptr())})}
}
impl Drop for Audio{fn drop(&mut self){unsafe{pc_audio_close(self.handle.as_ptr())}}}

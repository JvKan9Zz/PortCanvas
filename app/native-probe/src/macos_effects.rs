//! Accessibility and power notifications. AppKit callbacks run on the main queue.
use crate::effects_policy::SystemEffects;
use std::{ffi::c_void,ptr::NonNull};
extern "C" {fn pc_effects_read()->u32;fn pc_effects_open(context:*mut c_void,callback:extern "C" fn(*mut c_void))->*mut c_void;fn pc_effects_close(value:*mut c_void);}
pub struct MacEffects{handle:Option<NonNull<c_void>>,callback:Option<Box<Box<dyn Fn()>>>}
extern "C" fn wake(context:*mut c_void){if !context.is_null(){let callback=unsafe{&*(context as *const Box<dyn Fn()>)};callback();}}
impl MacEffects {
    pub fn new(callback:impl Fn()+Send+Sync+'static)->Self{let mut callback:Box<Box<dyn Fn()>>=Box::new(Box::new(callback));let context=(&mut *callback as *mut Box<dyn Fn()>).cast();let handle=NonNull::new(unsafe{pc_effects_open(context,wake)});Self{handle,callback:Some(callback)}}
    pub fn read(&self)->SystemEffects{let flags=unsafe{pc_effects_read()};SystemEffects{high_contrast:flags&1!=0,transparency_off:flags&2!=0,animations_off:flags&4!=0,energy_saver:flags&8!=0,..Default::default()}}
    pub fn report(&self)->serde_json::Value{serde_json::json!({"platform":"macos","snapshot":self.read(),"subscriptions":if self.handle.is_some(){2}else{0}})}
    pub fn close(&mut self)->Vec<String>{if let Some(handle)=self.handle.take(){unsafe{pc_effects_close(handle.as_ptr())};}self.callback.take();vec![]}
}
impl Drop for MacEffects{fn drop(&mut self){self.close();}}

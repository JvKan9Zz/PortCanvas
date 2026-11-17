//! Main-thread AppKit adapter for the existing winit renderer.
use raw_window_handle::{HasWindowHandle,RawWindowHandle};
use slint::winit_030::winit::window::Window;
use std::{ffi::c_void,ptr::NonNull};
extern "C" {
    fn pc_window_open(view:*mut c_void,context:*mut c_void,callback:extern "C" fn(*mut c_void,i32))->*mut c_void;
    fn pc_window_material(value:*mut c_void,rects:*const f32,count:usize,quality:i32,opacity:f32,dark:i32,enabled:i32)->i32;
    fn pc_window_background(value:*mut c_void,pixels:*const u8,width:u32,height:u32,crop:*const f32);
    fn pc_window_color(value:*mut c_void,red:f32,green:f32,blue:f32);
    fn pc_window_show(value:*mut c_void);
    fn pc_window_hide(value:*mut c_void);fn pc_window_close(value:*mut c_void);
    fn pc_window_cursor(view:*mut c_void,x:*mut f32,y:*mut f32)->i32;
}
fn view(window:&Window)->Option<*mut c_void>{match window.window_handle().ok()?.as_raw(){RawWindowHandle::AppKit(handle)=>Some(handle.ns_view.as_ptr()),_=>None}}
extern "C" fn action(context:*mut c_void,value:i32){let callback=unsafe{&*(context as *const Box<dyn Fn(i32)>)};callback(value);}
pub struct Chrome{handle:NonNull<c_void>,_callback:Box<Box<dyn Fn(i32)>>,style:i32}
impl Chrome {
    pub fn new(window:&Window,callback:impl Fn(i32)+'static)->Option<Self>{let mut callback:Box<Box<dyn Fn(i32)>>=Box::new(Box::new(callback));let context=(&mut *callback as *mut Box<dyn Fn(i32)>).cast();let handle=NonNull::new(unsafe{pc_window_open(view(window)?,context,action)})?;Some(Self{handle,_callback:callback,style:0})}
    pub fn update(&mut self,rects:&[[f32;6]],quality:i32,opacity:f32,dark:bool,enabled:bool)->bool{self.style=unsafe{pc_window_material(self.handle.as_ptr(),rects.as_ptr().cast(),rects.len(),quality,opacity,dark as i32,enabled as i32)};self.style>0}
    pub fn background(&self,pixels:Option<(&[u8],u32,u32)>,crop:[f32;4]){let(data,w,h)=pixels.map(|(data,w,h)|(data.as_ptr(),w,h)).unwrap_or((std::ptr::null(),0,0));unsafe{pc_window_background(self.handle.as_ptr(),data,w,h,crop.as_ptr())}}
    pub fn color(&self,color:slint::Color){unsafe{pc_window_color(self.handle.as_ptr(),color.red() as f32/255.,color.green() as f32/255.,color.blue() as f32/255.)}}
    pub fn show(&self){unsafe{pc_window_show(self.handle.as_ptr())}}
    pub fn hide(&self){unsafe{pc_window_hide(self.handle.as_ptr())}}
    pub fn report(&self)->serde_json::Value{serde_json::json!({"nativeSystemMaterial":true,"style":match self.style{2=>"NSGlassEffectView",1=>"NSVisualEffectView",_=>"solid"},"geometryUnits":"AppKit points"})}
}
impl Drop for Chrome{fn drop(&mut self){unsafe{pc_window_close(self.handle.as_ptr())}}}
pub fn cursor(window:&Window)->Option<(f32,f32)>{let(mut x,mut y)=(0.,0.);(unsafe{pc_window_cursor(view(window)?,&mut x,&mut y)}!=0).then_some((x,y))}

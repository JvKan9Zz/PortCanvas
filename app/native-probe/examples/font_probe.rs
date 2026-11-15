//! Explicit GPU regression probe for text translated by fractions of a physical pixel.
use anyhow::{Context,Result};
use slint::ComponentHandle;
use std::{path::PathBuf,cell::Cell,rc::Rc,time::Duration};
use glow::HasContext;
slint::slint! {
    import "../assets/ui-regular.ttf";
    import "../assets/ui-medium.ttf";
    import "../assets/ui-semibold.ttf";
    export component FontProbe inherits Window {
        title:"PortCanvas font raster probe";width:680px;height:270px;
        background:#ffffff;default-font-family:"PortCanvas UI";
        for offset[index] in [0.0,0.25,0.5,0.75]:Text {
            x:40px+offset*1phx;y:20px+index*60px;
            width:620px;height:48px;text:"设备连接与管理 1024 MiB";
            color:#182333;font-size:31phx;font-weight:600;vertical-alignment:top;
        }
    }
}
fn main()->Result<()> {
    let proof=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").context("owned WCS_TEST_ROOT required")?).canonicalize()?;
    let name=std::env::var("WCS_FONT_CASE").context("explicit WCS_FONT_CASE required")?;
    anyhow::ensure!(name.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'),"case name must be simple");
    slint::BackendSelector::new().backend_name("winit".into()).renderer_name("femtovg".into()).select()?;
    let ui=FontProbe::new()?;let weak=ui.as_weak();let timer=slint::Timer::default();
    let report_path=proof.join(format!("{name}.json"));let captured=Rc::new(Cell::new(false));
    ui.window().set_rendering_notifier(move|state,api|{
        if !matches!(state,slint::RenderingState::AfterRendering)||captured.replace(true){return;}
        let ui=weak.upgrade().unwrap();
        let slint::GraphicsAPI::NativeOpenGL{get_proc_address}=api else {panic!("native OpenGL required")};
        let size=ui.window().size();assert!(size.width>0&&size.height>0);
        let mut raw=vec![0u8;size.width as usize*size.height as usize*4];
        unsafe {
            let gl=glow::Context::from_loader_function(|n|get_proc_address(&std::ffi::CString::new(n).unwrap()));
            let framebuffer=gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
            let read_buffer=gl.get_parameter_i32(glow::READ_BUFFER) as u32;
            let pack=gl.get_parameter_buffer(glow::PIXEL_PACK_BUFFER_BINDING);
            let stores=[glow::PACK_ALIGNMENT,glow::PACK_ROW_LENGTH,glow::PACK_SKIP_ROWS,glow::PACK_SKIP_PIXELS].map(|p|(p,gl.get_parameter_i32(p)));
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER,None);gl.bind_framebuffer(glow::READ_FRAMEBUFFER,None);gl.read_buffer(glow::BACK);
            for (p,_) in stores{gl.pixel_store_i32(p,if p==glow::PACK_ALIGNMENT{1}else{0});}
            gl.read_pixels(0,0,size.width as i32,size.height as i32,glow::RGBA,glow::UNSIGNED_BYTE,glow::PixelPackData::Slice(Some(&mut raw)));
            assert_eq!(gl.get_error(),glow::NO_ERROR);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER,framebuffer);gl.read_buffer(read_buffer);gl.bind_buffer(glow::PIXEL_PACK_BUFFER,pack);
            for(p,value)in stores{gl.pixel_store_i32(p,value);}
        }
        let mut pixels=image::RgbaImage::from_raw(size.width,size.height,raw).unwrap();image::imageops::flip_vertical_in_place(&mut pixels);
        pixels.save(proof.join(format!("{name}.png"))).expect("PNG proof");
        let scale=ui.window().scale_factor();let stride=pixels.width() as usize*4;
        let raw=pixels.as_raw();let x=(40.0*scale).round() as usize;let top=(20.0*scale).round() as usize;let step=(60.0*scale).round() as usize;
        let width=(500.0*scale).round() as usize;let mut differences=Vec::new();
        for row in 1..4 {
            let best=(-2i32..=2).map(|shift|{
                let mut changed=0;
                for y in 0..45 {for dx in 0..width {
                    let first=(top+y)*stride+(x+dx)*4;
                    let second=(top+row*step+y)*stride+((x+dx) as i32+shift) as usize*4;
                    if raw[first..first+3]!=raw[second..second+3] {changed+=1;}
                }}changed
            }).min().unwrap();differences.push(best);
        }
        assert!(raw.chunks_exact(4).filter(|p|p[0]<200).count()>100,"probe must contain rendered text");
        let report=serde_json::json!({"case":name,"scale":scale,"dimensions":[pixels.width(),pixels.height()],"physicalOffsets":[0,0.25,0.5,0.75],"bestIntegerAlignedChangedPixels":differences});
        std::fs::write(proof.join(format!("{name}.json")),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!("{report}");slint::quit_event_loop().unwrap();
    }).map_err(|e|anyhow::anyhow!("{e:?}"))?;
    timer.start(slint::TimerMode::SingleShot,Duration::from_secs(5),||{let _=slint::quit_event_loop();});
    ui.run()?;anyhow::ensure!(report_path.is_file(),"font snapshot unavailable");Ok(())
}

//! Isolated Windows GPU gate. No services, settings, network or production data are opened.
mod gpu;
use anyhow::{ensure, Context, Result};
use portcanvas_native_probe::{background_playback::Playback, media::VideoFrame};
use slint::{winit_030::WinitWindowAccessor, ComponentHandle};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Instant,
};
slint::slint! {
    import {Button,Slider,VerticalBox,HorizontalBox,Palette} from "std-widgets.slint";
    export component Probe inherits Window {
        init=>{Palette.color-scheme=ColorScheme.light;}
        title:"PortCanvas · Glass GPU probe (isolated)";
        preferred-width:1000px;preferred-height:680px;
        in property <image> source-image;in property <image> material-image;
        in-out property <int> quality:2;in-out property <float> material-opacity:50;
        in property <bool> video:false;in property <bool> native-hidden:false;in property <bool> benchmarking:false;
        in property <string> notice;in-out property <bool> feedback:false;
        out property <float> view-width:root.width/1px;out property <float> view-height:root.height/1px;
        callback controls();callback poll();callback benchmark();
        Timer {interval:16ms;running:root.video&&!root.native-hidden;triggered=>{root.poll();}}
        Timer {interval:16ms;running:root.benchmarking&&!root.native-hidden;triggered=>{root.benchmark();}}
        Image {source:root.source-image;width:parent.width;height:parent.height;image-fit:fill;}
        Rectangle {x:32px;y:24px;width:parent.width - 64px;height:88px;background:#f0f4f9f0;border-radius:12px;
            VerticalBox {padding:12px;spacing:8px;
                Text {text:"Native GPU material probe · 应用内背景";color:#152338;font-size:18px;font-weight:600;}
                HorizontalBox {padding:0px;
                    Button {text:root.quality==0?"Performance · selected":"Performance";clicked=>{root.quality=0;root.controls();}}
                    Button {text:root.quality==1?"Balanced · selected":"Balanced";clicked=>{root.quality=1;root.controls();}}
                    Button {text:root.quality==2?"Full · selected":"Full";clicked=>{root.quality=2;root.controls();}}
                }
            }
        }
        Image {x:48px;y:148px;width:parent.width - 96px;height:260px;source:root.material-image;image-fit:fill;}
        VerticalBox {x:76px;y:176px;width:parent.width - 152px;height:204px;padding:0px;spacing:16px;
            Text {text:"PortCanvas";font-size:34px;font-weight:600;color:#122c46;}
            Text {text:"清晰内容层 · Native controls stay sharp";font-size:19px;color:#122c46;}
            HorizontalBox {padding:0px;
                Button {text:root.feedback?"Input response B":"Input response A";clicked=>{root.feedback=!root.feedback;root.controls();}}
                Text {text:"Opacity "+round(root.material-opacity)+"%";color:#122c46;vertical-alignment:center;}
                Slider {value<=>root.material-opacity;minimum:0;maximum:100;step:1;changed(_)=>{root.controls();}}
            }
        }
        Rectangle {x:32px;y:440px;width:parent.width - 64px;height:150px;background:#f0f4f9f0;border-radius:12px;
            VerticalBox {padding:20px;Text {text:root.notice;wrap:word-wrap;color:#152338;}
                Text {text:"One source upload; local GPU sampling; no screen capture or pixel readback.\nStatic manual mode redraws only on changes. Video uses the existing native player.";wrap:word-wrap;color:#152338;}
            }
        }
    }
}
struct State {
    gpu: RefCell<Option<gpu::Gpu>>,
    frame: RefCell<Option<Arc<VideoFrame>>>,
    player: RefCell<Option<Playback>>,
    sequence: Cell<u64>,
    uploaded_sequence: Cell<Option<u64>>,
    source_dimensions: Cell<Option<(u32, u32)>>,
    images_bound: Cell<bool>,
    dirty: Cell<bool>,
    key: Cell<Option<(i32, i32, i32, i32)>>,
    input: Cell<Option<Instant>>,
    input_us: RefCell<Vec<f64>>,
    render_started: Cell<Option<(i32, Instant)>>,
    frame_us: RefCell<Vec<(i32, f64)>>,
    report: RefCell<serde_json::Value>,
    error: RefCell<String>,
    hidden: Cell<bool>,
    hides: Cell<u64>,
    visibility: RefCell<Vec<serde_json::Value>>,
}
fn summary(samples: &[(i32, f64)]) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    for (mode, name) in [(0, "performance"), (1, "balanced"), (2, "full")] {
        let mut values = samples
            .iter()
            .filter(|(m, _)| *m == mode)
            .map(|(_, n)| *n)
            .collect::<Vec<_>>();
        values.sort_by(f64::total_cmp);
        if !values.is_empty() {
            let p95 = ((values.len() - 1) as f64 * 0.95).ceil() as usize;
            result.insert(name.into(),serde_json::json!({"samples":values.len(),"p50":values[values.len()/2],"p95":values[p95],"max":values[values.len()-1]}));
        }
    }
    result.into()
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(args.next().context("image/video path required")?);
    let output = PathBuf::from(args.next().context("report path required")?);
    let seconds = args
        .next()
        .map(|s| s.to_string_lossy().parse::<u64>())
        .transpose()?
        .unwrap_or(0);
    ensure!(
        source.is_absolute() && output.is_absolute(),
        "absolute isolated paths required"
    );
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("femtovg".into())
        .select()?;
    let video = matches!(
        source
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "mp4" | "m4v" | "webm"
    );
    let frame = if video {
        None
    } else {
        let size = image::ImageReader::open(&source)?.into_dimensions()?;
        ensure!(
            u64::from(size.0) * u64::from(size.1) * 4 <= 128 * 1024 * 1024,
            "probe image pixel budget exceeded"
        );
        let image = image::open(&source)?.thumbnail(1920, 1080).into_rgba8();
        Some(Arc::new(VideoFrame {
            width: image.width(),
            height: image.height(),
            timestamp_100ns: 0,
            duration_100ns: None,
            rgba: image.into_raw(),
        }))
    };
    let ui = Probe::new()?;
    ui.set_video(video);
    ui.set_benchmarking(seconds > 0);
    ui.set_notice(if seconds>0{"Controlled redraw workload; modes rotate every third of the run."}else{"Performance: tint/edge. Balanced: 9-tap blur. Full: blur, local edge refraction and highlight."}.into());
    let state = Rc::new(State {
        gpu: RefCell::new(None),
        frame: RefCell::new(frame),
        player: RefCell::new(video.then(|| Playback::open(source, 0, false))),
        sequence: Cell::new(0),
        uploaded_sequence: Cell::new(None),
        source_dimensions: Cell::new(None),
        images_bound: Cell::new(false),
        dirty: Cell::new(true),
        key: Cell::new(None),
        input: Cell::new(None),
        input_us: RefCell::new(Vec::new()),
        render_started: Cell::new(None),
        frame_us: RefCell::new(Vec::new()),
        report: RefCell::new(serde_json::Value::Null),
        error: RefCell::new(String::new()),
        hidden: Cell::new(false),
        hides: Cell::new(0),
        visibility: RefCell::new(Vec::new()),
    });
    ui.on_controls({
        let s = state.clone();
        let weak = ui.as_weak();
        move || {
            s.input.set(Some(Instant::now()));
            s.dirty.set(true);
            if let Some(ui) = weak.upgrade() {
                ui.window().request_redraw();
            }
        }
    });
    ui.on_poll({
        let s = state.clone();
        let weak = ui.as_weak();
        move || {
            if let Some(player) = s.player.borrow().as_ref() {
                let (sequence, frame, error) =
                    player.read(|p| (p.sequence, p.frame.clone(), p.error.clone()));
                if let Some(error) = error {
                    *s.error.borrow_mut() = error;
                }
                if sequence != s.sequence.get() {
                    s.sequence.set(sequence);
                    *s.frame.borrow_mut() = frame;
                    s.dirty.set(true);
                    if let Some(ui) = weak.upgrade() {
                        ui.window().request_redraw();
                    }
                }
            }
        }
    });
    let started = Instant::now();
    ui.on_benchmark({
        let s = state.clone();
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                let elapsed = started.elapsed().as_secs_f64();
                if elapsed >= seconds as f64 {
                    let _ = ui.hide();
                    let _ = slint::quit_event_loop();
                    return;
                }
                let quality = ((elapsed / (seconds as f64 / 3.0)) as i32).min(2);
                if ui.get_quality() != quality {
                    ui.set_quality(quality);
                }
                s.dirty.set(true);
                ui.window().request_redraw();
            }
        }
    });
    ui.window().on_winit_window_event({
        let s = state.clone();
        let weak = ui.as_weak();
        move |window, event| {
            use slint::winit_030::{winit::event::WindowEvent, EventResult};
            if let WindowEvent::Occluded(hidden) = event {
                s.hidden.set(*hidden);
            }
            if matches!(event, WindowEvent::Occluded(_) | WindowEvent::Resized(_)) {
                let hidden = s.hidden.get()
                    || window
                        .with_winit_window(|w| w.is_minimized() == Some(true))
                        .unwrap_or(false);
                if let Some(ui) = weak.upgrade() {
                    if hidden != ui.get_native_hidden() {
                        ui.set_native_hidden(hidden);
                        s.hides.set(s.hides.get() + 1);
                        if let Some(gpu)=s.gpu.borrow().as_ref(){s.visibility.borrow_mut().push(serde_json::json!({"hidden":hidden,"draws":gpu.draws,"uploads":gpu.uploads,"atSeconds":started.elapsed().as_secs_f64()}));}
                        if let Some(p) = s.player.borrow().as_ref() {
                            p.paused(hidden);
                        }
                    }
                }
            }
            EventResult::Propagate
        }
    });
    ui.window().set_rendering_notifier({let s=state.clone();let weak=ui.as_weak();move|phase,api|unsafe{
        match phase{
            slint::RenderingState::RenderingSetup=>{
                let result=match api{slint::GraphicsAPI::NativeOpenGL{get_proc_address}=>gpu::Gpu::new(glow::Context::from_loader_function_cstr(|name|get_proc_address(name))),_=>Err(anyhow::anyhow!("OpenGL renderer required"))};
                match result{Ok(gpu)=>{
                    *s.gpu.borrow_mut()=Some(gpu);s.uploaded_sequence.set(None);s.source_dimensions.set(None);s.images_bound.set(false);s.key.set(None);s.dirty.set(true);
                },Err(e)=>*s.error.borrow_mut()=format!("{e:#}")}
            }
            slint::RenderingState::BeforeRendering=>{
                let Some(ui)=weak.upgrade()else{return};
                let key=(ui.get_quality(),ui.get_material_opacity().round() as i32,ui.get_view_width().round() as i32,ui.get_view_height().round() as i32);
                s.render_started.set(Some((key.0,Instant::now())));
                if s.dirty.get()||s.key.get()!=Some(key){
                    if let(Some(gpu),Some(frame))=(s.gpu.borrow_mut().as_mut(),s.frame.borrow().as_ref()){
                        let changed=s.uploaded_sequence.get()!=Some(s.sequence.get());
                        let upload=if changed{Some((frame.width,frame.height,frame.rgba.as_slice()))}else{None};
                        let crop=[48.0/key.2 as f32,148.0/key.3 as f32,(key.2 as f32-96.0)/key.2 as f32,260.0/key.3 as f32];
                        match gpu.draw(upload,key.0,key.1 as f32,crop){Ok((background,material))=>{
                            // Texture IDs are stable; update Slint bindings only when needed.
                            if s.source_dimensions.get()!=Some((frame.width,frame.height)){ui.set_source_image(background);s.source_dimensions.set(Some((frame.width,frame.height)));}
                            if !s.images_bound.replace(true){ui.set_material_image(material);}
                            s.uploaded_sequence.set(Some(s.sequence.get()));s.dirty.set(false);s.key.set(Some(key));
                        },Err(e)=>*s.error.borrow_mut()=format!("{e:#}")}
                    }
                }
            }
            slint::RenderingState::AfterRendering=>{
                if let Some((mode,time))=s.render_started.take(){s.frame_us.borrow_mut().push((mode,time.elapsed().as_secs_f64()*1e6));}
                if let Some(time)=s.input.take(){s.input_us.borrow_mut().push(time.elapsed().as_secs_f64()*1e6);}
            }
            slint::RenderingState::RenderingTeardown=>{
                if let Some(gpu)=s.gpu.borrow_mut().take(){
                    *s.report.borrow_mut()=serde_json::json!({"driver":gpu.driver,"sourceUploads":gpu.uploads,"materialDraws":gpu.draws,"gpuTimingSupported":gpu.timer_queries_supported(),"shaderGpuUs":summary(&gpu.gpu_samples),"cpuSubmitUs":summary(&gpu.submit_samples),"textureSize":[1120,280]});
                    gpu.destroy();
                }
            }
            _=>{}
        }
    }}).map_err(|e|anyhow::anyhow!("render notifier: {e:?}"))?;
    let result = ui.run();
    if let Some(mut player) = state.player.borrow_mut().take() {
        player.close()?;
    }
    drop(ui);
    let mut report = state.report.borrow().clone();
    report["error"] = serde_json::json!(state.error.borrow().as_str());
    report["released"] = serde_json::json!(state.gpu.borrow().is_none());
    report["uiCpuFrameUs"] = summary(&state.frame_us.borrow());
    report["inputToAfterRenderUs"] = serde_json::json!(*state.input_us.borrow());
    report["hideTransitions"] = serde_json::json!(state.hides.get());
    report["visibility"] = serde_json::json!(*state.visibility.borrow());
    report["lastMaterial"] = serde_json::json!(state.key.get());
    report["seconds"] = serde_json::json!(started.elapsed().as_secs_f64());
    report["video"] = serde_json::json!(video);
    report["forcedRedraw"] = serde_json::json!(seconds > 0);
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    result?;
    ensure!(state.error.borrow().is_empty(), "{}", state.error.borrow());
    ensure!(state.gpu.borrow().is_none(), "GPU teardown missing");
    Ok(())
}

use crate::{
    material_gpu::{Gpu, Surface},
    workbench_ui::Workbench,
};
use portcanvas_native_probe::{
    effects_policy::{Responsiveness, SystemEffects},
    glass_model::{project, Rect},
    windows_effects::WindowsEffects,
};
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

// Only debug builds accept deterministic policy/heartbeat inputs for isolated GUI tests.
#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Fixture {
    policy: SystemEffects,
    active: bool,
    late_ms: u64,
}
fn fixture() -> Option<Fixture> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("PORTCANVAS_EFFECTS_FIXTURE") {
        return Some(
            std::fs::read(path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or(Fixture {
                    policy: SystemEffects {
                        unavailable: true,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        );
    }
    None
}

#[derive(PartialEq)]
struct Key {
    revision: u64,
    window: (f32, f32),
    crop: [f32; 4],
    rects: Vec<Rect>,
    radii: Vec<f32>,
    quality: i32,
    opacity: f32,
    tint: [u8; 3],
}
pub struct Controller {
    ui: slint::Weak<Workbench>,
    pixels: RefCell<Option<SharedPixelBuffer<Rgba8Pixel>>>,
    revision: Cell<u64>,
    uploaded: Cell<u64>,
    gpu: RefCell<Option<Gpu>>,
    key: RefCell<Option<Key>>,
    bindings: RefCell<Option<((u32, u32), Vec<(u32, u32)>, u8)>>,
    failed: Cell<bool>,
    uploads: Cell<u64>,
    passes: Cell<u64>,
    quality_passes: [Cell<u64>; 3],
    visibility: RefCell<Vec<serde_json::Value>>,
    released: Cell<bool>,
    driver: RefCell<String>,
    error: RefCell<String>,
    os: RefCell<Option<WindowsEffects>>,
    os_report: serde_json::Value,
    policy: Cell<SystemEffects>,
    synthetic: Cell<bool>,
    fixture_active: Cell<bool>,
    fixture_late: Cell<u64>,
    heartbeat: slint::Timer,
    monitor: RefCell<Responsiveness>,
    epoch: Instant,
    last_tick: Cell<Instant>,
    last_render: Cell<Option<Instant>>,
    prompted: Cell<bool>,
    ticks: Cell<u64>,
    unsubscribe_errors: RefCell<Vec<String>>,
    transitions: RefCell<Vec<serde_json::Value>>,
    record_geometry: bool,
    last_geometry: Cell<Option<(bool,[f32;4],[f32;14])>>,
    geometry: RefCell<Vec<serde_json::Value>>,
    backdrop_events: RefCell<Vec<serde_json::Value>>,
}
impl Controller {
    pub fn install(ui: &Workbench, pending_draw: Arc<AtomicU64>) -> anyhow::Result<Rc<Self>> {
        let weak = ui.as_weak();
        let os = WindowsEffects::new(move || {
            let _ = weak.upgrade_in_event_loop(|ui| ui.invoke_material_refresh());
        });
        let os_report = os.report();
        let c = Rc::new(Self {
            ui: ui.as_weak(),
            pixels: RefCell::new(None),
            revision: Cell::new(0),
            uploaded: Cell::new(0),
            gpu: RefCell::new(None),
            key: RefCell::new(None),
            bindings: RefCell::new(None),
            failed: Cell::new(false),
            uploads: Cell::new(0),
            passes: Cell::new(0),
            quality_passes: Default::default(),
            visibility: RefCell::new(Vec::new()),
            released: Cell::new(true),
            driver: RefCell::new(String::new()),
            error: RefCell::new(String::new()),
            os: RefCell::new(Some(os)),
            os_report,
            policy: Cell::new(Default::default()),
            synthetic: Cell::new(false),
            fixture_active: Cell::new(false),
            fixture_late: Cell::new(0),
            heartbeat: Default::default(),
            monitor: RefCell::new(Default::default()),
            epoch: Instant::now(),
            last_tick: Cell::new(Instant::now()),
            last_render: Cell::new(None),
            prompted: Cell::new(false),
            ticks: Cell::new(0),
            unsubscribe_errors: Default::default(),
            transitions: Default::default(),
            record_geometry: std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_some(),
            last_geometry: Cell::new(None),
            geometry: RefCell::new(Vec::new()),
            backdrop_events: RefCell::new(Vec::new()),
        });
        ui.set_observe_motion(c.record_geometry);
        ui.on_settings_backdrop({let c=c.clone();move |capture|c.backdrop(capture)});
        ui.on_material_refresh({
            let c = c.clone();
            move || c.refresh()
        });
        ui.on_material_controls({
            let c = c.clone();
            move || {
                c.reset_monitor();
                if let Some(ui)=c.ui.upgrade() { c.visibility(ui.get_native_hidden()); }
            }
        });
        ui.on_material_response({
            let c = c.clone();
            move |accepted| {
                let Some(ui) = c.ui.upgrade() else { return };
                if ui.get_dialog_kind() != 12 {
                    return;
                }
                ui.set_dialog_kind(0);
                if accepted
                    && !c.policy.get().solid()
                    && c.policy.get().effective(ui.get_glass_quality()) == ui.get_glass_quality()
                {
                    ui.set_glass_quality(ui.get_material_suggestion_target());
                    ui.invoke_glass_controls(true);
                }
            }
        });
        c.refresh();
        let render = c.clone();
        let weak = ui.as_weak();
        // One owner for material setup/passes/teardown AND existing desktop frame acknowledgement.
        ui.window()
            .set_rendering_notifier(move |state, api| match state {
                slint::RenderingState::RenderingSetup => {
                    render.failed.set(false);
                    render.uploaded.set(0);
                    render.key.borrow_mut().take();
                    render.bindings.borrow_mut().take();
                }
                slint::RenderingState::BeforeRendering => render.draw(api),
                slint::RenderingState::AfterRendering => {
                    render.last_render.set(Some(Instant::now()));
                    render.watch();
                    let ticket = pending_draw.swap(0, Ordering::SeqCst);
                    if ticket != 0 {
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.invoke_desktop_drawn(ticket.to_string().into())
                        });
                    }
                }
                slint::RenderingState::RenderingTeardown => render.release(),
                _ => {}
            })
            .map_err(|e| anyhow::anyhow!("原生绘制通知不可用：{e:?}"))?;
        Ok(c)
    }
    fn reset_monitor(&self) {
        self.heartbeat.stop();
        self.monitor.borrow_mut().reset();
        self.last_tick.set(Instant::now());
    }
    fn refresh(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let synthetic = fixture();
        let policy = synthetic.as_ref().map(|f| f.policy).unwrap_or_else(|| {
            self.os
                .borrow()
                .as_ref()
                .map(|o| o.read())
                .unwrap_or_default()
        });
        self.synthetic.set(synthetic.is_some());
        self.fixture_active
            .set(synthetic.as_ref().is_some_and(|f| f.active));
        self.fixture_late
            .set(synthetic.map(|f| f.late_ms.min(10000)).unwrap_or(0));
        let changed = self.policy.replace(policy) != policy;
        if std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_some()
            && (changed || self.transitions.borrow().is_empty())
        {
            self.transitions.borrow_mut().push(serde_json::json!({"policy":policy,"preferred":ui.get_glass_quality(),"opacity":ui.get_bg_material(),"beforePasses":self.passes.get(),"beforeUploads":self.uploads.get()}));
        }
        self.reset_monitor();
        ui.set_high_contrast(policy.high_contrast);ui.set_material_solid(policy.solid());
        ui.set_material_status(
            if policy.notice().is_empty() && self.failed.get() {
                "GPU 玻璃不可用，当前显示基础材质。"
            } else {
                policy.notice()
            }
            .into(),
        );
        if changed && ui.get_dialog_kind() == 12 {
            ui.set_dialog_kind(0);
        }
        ui.window().request_redraw();
    }
    fn can_monitor(&self, ui: &Workbench) -> bool {
        !self.prompted.get()
            && !ui.get_native_hidden()
            && !ui.get_desktop_expanded()
            && !ui.get_settings_open()
            && !ui.get_update_busy()
            && ui.get_native_focused()
            && ui.get_dialog_kind() == 0
            && !ui.get_system_busy()
            && !ui.get_migration_busy()
            && !ui.get_busy()
            && !ui.get_bg_busy()
            && !ui.get_file_mutating()
            && !ui.get_terminal_busy()
            && ui.get_glass_loaded()
            && ui.get_glass_quality() > 0
            && ui.get_material_active()
            && self.policy.get() == SystemEffects::default()
    }
    fn watch(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        if !self.can_monitor(&ui) {
            self.reset_monitor();
            return;
        }
        if !self.heartbeat.running() {
            self.last_tick.set(Instant::now());
            let weak = Rc::downgrade(self);
            self.heartbeat.start(
                slint::TimerMode::Repeated,
                Duration::from_millis(250),
                move || {
                    if let Some(c) = weak.upgrade() {
                        c.tick();
                    }
                },
            );
        }
    }
    fn tick(&self) {
        let Some(ui) = self.ui.upgrade() else {
            self.reset_monitor();
            return;
        };
        if !self.can_monitor(&ui)
            || (!self.fixture_active.get()
                && self
                    .last_render
                    .get()
                    .is_none_or(|t| t.elapsed() > Duration::from_millis(500)))
        {
            self.reset_monitor();
            return;
        }
        let now = Instant::now();
        let previous = self.last_tick.replace(now);
        let late = if self.synthetic.get() {
            self.fixture_late.get()
        } else {
            now.duration_since(previous).as_millis().saturating_sub(250) as u64
        };
        self.ticks.set(self.ticks.get() + 1);
        if self
            .monitor
            .borrow_mut()
            .sample(self.epoch.elapsed().as_millis() as u64, late)
        {
            self.prompted.set(true);
            self.heartbeat.stop();
            let target = (ui.get_glass_quality() - 1).max(0);
            ui.set_material_suggestion_target(target);
            ui.set_dialog_title("降低玻璃效果？".into());
            ui.set_dialog_message(
                crate::workbench::text(
                    if target == 1 {
                        "界面持续响应较慢，是否将玻璃效果切换到均衡？"
                    } else {
                        "界面持续响应较慢，是否将玻璃效果切换到轻量？"
                    },
                    ui.get_english(),
                )
                .into(),
            );
            ui.set_dialog_kind(12);
        }
    }
    /// Share the same CPU buffer used by the ordinary Slint fallback; never decode twice.
    pub fn publish(&self, pixels: SharedPixelBuffer<Rgba8Pixel>) {
        if let Some(ui) = self.ui.upgrade() {
            if !ui.get_material_active() {
                ui.set_bg_frame(slint::Image::from_rgba8(pixels.clone()));
            }
            *self.pixels.borrow_mut() = Some(pixels);
            self.revision.set(self.revision.get() + 1);
            ui.window().request_redraw();
        }
    }
    pub fn clear(&self) {
        self.pixels.borrow_mut().take();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_material_active(false);
            ui.set_bg_frame(Default::default());
            ui.set_material_nav(Default::default());
            ui.set_material_header(Default::default());
            ui.set_material_terminal(Default::default());
            ui.set_material_work(Default::default());
            ui.window().request_redraw();
        }
    }
    fn release(&self) {
        if let Some(gpu) = self.gpu.borrow_mut().take() {
            unsafe {
                gpu.destroy();
            }
        }
        self.uploaded.set(0);
        self.bindings.borrow_mut().take();
        self.key.borrow_mut().take();
        self.released.set(true);
    }
    fn draw(&self, api: &slint::GraphicsAPI) {
        let Some(ui) = self.ui.upgrade() else { return };
        if self.record_geometry {
            let r=ui.get_desktop_work_rect();
            let c=ui.get_chrome_layout();
            let state=(ui.get_desktop_expanded(),[r.x,r.y,r.width,r.height],[c.nav,c.drawer,c.drawer_x,c.settings_y,c.settings_opacity,c.settings_tab,c.page,c.dialog_y,c.dialog_opacity,c.advanced,c.connection_height,c.busy_phase,c.busy_wave,c.busy]);
            if self.last_geometry.replace(Some(state))!=Some(state) && self.geometry.borrow().len()<2048 {
                self.geometry.borrow_mut().push(serde_json::json!({"ms":self.epoch.elapsed().as_millis(),"expanded":state.0,"rect":state.1,"chrome":state.2}));
            }
        }
        if ui.get_native_hidden() { return; }
        let pixels = self.pixels.borrow();
        let Some(pixels) = pixels.as_ref() else {
            self.release();
            return;
        };
        if self.policy.get().solid() {
            if ui.get_material_active() {
                ui.set_bg_frame(slint::Image::from_rgba8(pixels.clone()));
            }
            ui.set_material_active(false);
            ui.set_material_nav(Default::default());
            ui.set_material_header(Default::default());
            ui.set_material_terminal(Default::default());
            ui.set_material_work(Default::default());
            self.release();
            return;
        }
        if self.failed.get() {
            return;
        }
        let expanded=ui.get_desktop_expanded();
        let mut panels=if expanded {vec![ui.get_material_header_rect()]} else {
            vec![ui.get_material_nav_rect(),ui.get_material_header_rect(),ui.get_desktop_work_rect()]
        };
        if !expanded && ui.get_terminal_visible() {
            panels.push(ui.get_material_terminal_rect());
        }
        let rects=panels.iter().map(|r|Rect{x:r.x,y:r.y,width:r.width,height:r.height}).collect::<Vec<_>>();
        let radii=panels.iter().map(|r|r.radius).collect::<Vec<_>>();
        let terminal_index=(!expanded&&ui.get_terminal_visible()).then_some(3usize);
        if rects.iter().any(|r| r.width <= 0. || r.height <= 0.) {
            return;
        }
        let tint = ui.get_material_tint();
        let size = ui.window().size().to_logical(ui.window().scale_factor());
        let key = Key {
            revision: self.revision.get(),
            window: (size.width, size.height),
            rects,
            radii,
            crop: [
                ui.get_bg_saved_crop_x(),
                ui.get_bg_saved_crop_y(),
                ui.get_bg_saved_crop_width(),
                ui.get_bg_saved_crop_height(),
            ],
            quality: self.policy.get().effective(ui.get_glass_quality()),
            opacity: ui.get_bg_material(),
            tint: [tint.red(), tint.green(), tint.blue()],
        };
        if self.key.borrow().as_ref() == Some(&key) {
            return;
        }
        let source = (pixels.width(), pixels.height());
        let surfaces = key
            .rects
            .iter()
            .zip(&key.radii)
            .map(|(r,radius)| Surface {
                // Material carries no text; logical-size targets bound fill cost at high DPI.
                size: (r.width.ceil() as u32, r.height.ceil() as u32),
                crop: project(source, key.crop, key.window, *r),
                radius:*radius,
            })
            .collect::<Vec<_>>();
        let result = (|| unsafe {
            let mut slot = self.gpu.borrow_mut();
            if slot.is_none() {
                let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
                    anyhow::bail!("Native OpenGL unavailable")
                };
                let gl = glow::Context::from_loader_function(|name| {
                    get_proc_address(&std::ffi::CString::new(name).unwrap())
                });
                let gpu = Gpu::new(gl)?;
                *self.driver.borrow_mut() = gpu.driver.clone();
                self.released.set(false);
                *slot = Some(gpu);
            }
            let gpu = slot.as_mut().unwrap();
            let frame = (self.uploaded.get() != key.revision)
                .then(|| (source.0, source.1, pixels.as_bytes()));
            let previous_passes=gpu.passes;
            let images = gpu.draw(
                frame,
                &surfaces,
                key.quality,
                key.opacity,
                key.tint.map(|v| v as f32 / 255.),
            )?;
            self.uploads
                .set(self.uploads.get() + u64::from(frame.is_some()));
            let executed=gpu.passes - previous_passes;
            self.passes.set(self.passes.get() + executed);
            self.quality_passes[key.quality.clamp(0, 2) as usize].set(
                self.quality_passes[key.quality.clamp(0, 2) as usize].get() + executed,
            );
            self.uploaded.set(key.revision);
            Ok::<_, anyhow::Error>(images)
        })();
        match result {
            Ok((background, images)) => {
                let binding = (source, surfaces.iter().map(|s| s.size).collect::<Vec<_>>(),u8::from(expanded)|(u8::from(ui.get_terminal_visible())<<1));
                if self.bindings.borrow().as_ref() != Some(&binding) {
                    ui.set_bg_frame(background);
                    ui.set_material_nav(if expanded {Default::default()} else {images[0].clone()});
                    ui.set_material_header(images[if expanded {0} else {1}].clone());
                    ui.set_material_work(if expanded {Default::default()} else {images[2].clone()});
                    ui.set_material_terminal(terminal_index.and_then(|i|images.get(i)).cloned().unwrap_or_default());
                    *self.bindings.borrow_mut() = Some(binding);
                }
                ui.set_material_active(true);
                if ui.get_material_status() == "GPU 玻璃不可用，当前显示基础材质。" {
                    ui.set_material_status(self.policy.get().notice().into());
                }
                *self.key.borrow_mut() = Some(key);
            }
            Err(error) => {
                eprintln!("Workbench material fallback: {error:#}");
                *self.error.borrow_mut() = error.to_string();
                self.failed.set(true);
                ui.set_material_status("GPU 玻璃不可用，当前显示基础材质。".into());
                ui.set_material_active(false);
                ui.set_bg_frame(slint::Image::from_rgba8(pixels.clone()));
                ui.set_material_nav(Default::default());
                ui.set_material_header(Default::default());
                ui.set_material_terminal(Default::default());
            ui.set_material_work(Default::default());
                self.release();
            }
        }
    }
    fn backdrop(&self, capture: bool) {
        let Some(ui)=self.ui.upgrade() else{return};
        ui.set_settings_backdrop_active(false);
        ui.set_settings_backdrop_frame(Default::default());
        if !capture || ui.get_material_solid() || ui.get_native_hidden() {
            if self.record_geometry && self.backdrop_events.borrow().len()<64 {
                self.backdrop_events.borrow_mut().push(serde_json::json!({"capture":false,"released":true}));
            }
            return;
        }
        // FemtoVG reads its last rendered front buffer once, before the modal is
        // painted. Keep only the bounded blurred image; no timer or live capture.
        let started=Instant::now();
        let result=(||->anyhow::Result<(slint::Image,u32,u32)>{
            let pixels=ui.window().take_snapshot().map_err(|e|anyhow::anyhow!("{e}"))?;
            let (width,height)=(pixels.width(),pixels.height());
            anyhow::ensure!(width>0&&height>0,"empty window snapshot");
            let limit=match ui.get_glass_quality(){2=>1024.0,1=>640.0,_=>320.0};
            let ratio=(limit/width as f32).min(640.0/height as f32).min(1.0);
            let (w,h)=((width as f32*ratio).round().max(1.0) as u32,(height as f32*ratio).round().max(1.0) as u32);
            let source=image::RgbaImage::from_raw(width,height,pixels.as_bytes().to_vec()).ok_or_else(||anyhow::anyhow!("invalid window pixels"))?;
            let small=image::imageops::resize(&source,w,h,image::imageops::FilterType::Nearest);
            let blurred=image::imageops::fast_blur(&small,(6.0*ui.window().scale_factor()*ratio).max(1.0));
            let buffer=SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(blurred.as_raw(),w,h);
            Ok((slint::Image::from_rgba8(buffer),w,h))
        })();
        let mut event=serde_json::json!({"capture":true,"ms":started.elapsed().as_secs_f64()*1000.0});
        match result {
            Ok((image,w,h))=>{ui.set_settings_backdrop_frame(image);ui.set_settings_backdrop_active(true);event["size"]=serde_json::json!([w,h]);}
            Err(error)=>{event["error"]=error.to_string().into();}
        }
        if self.record_geometry && self.backdrop_events.borrow().len()<64 {self.backdrop_events.borrow_mut().push(event);}
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"sourceUploads":self.uploads.get(),"materialPasses":self.passes.get(),
            "qualityPasses":self.quality_passes.each_ref().map(Cell::get),"visibility":*self.visibility.borrow(),
            "released":self.released.get(),"driver":*self.driver.borrow(),"error":*self.error.borrow(),
            "policy":self.policy.get(),"os":self.os_report,"subscriptionsReleased":self.os.borrow().is_none()&&self.unsubscribe_errors.borrow().is_empty(),"unsubscribeErrors":*self.unsubscribe_errors.borrow(),
            "synthetic":self.synthetic.get(),"heartbeatTicks":self.ticks.get(),"prompted":self.prompted.get(),"transitions":*self.transitions.borrow(),"backdrop":*self.backdrop_events.borrow(),"chromeFields":["navY","drawerWidth","drawerX","settingsY","settingsOpacity","settingsTabY","pagePhase","dialogY","dialogOpacity","advancedHeight","connectionHeight","busyPhase","busyWave","busy"],"geometry":*self.geometry.borrow()})
    }
    pub fn visibility(&self, hidden: bool) {
        if hidden {
            self.reset_monitor();
        }
        if std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_some() {
            self.visibility.borrow_mut().push(serde_json::json!({"hidden":hidden,"desktopExpanded":self.ui.upgrade().is_some_and(|ui|ui.get_desktop_expanded()),"sourceUploads":self.uploads.get(),"materialPasses":self.passes.get(),"heartbeatTicks":self.ticks.get()}));
        }
    }
    pub fn shutdown(&self) {
        self.backdrop(false);
        self.reset_monitor();
        if let Some(mut os) = self.os.borrow_mut().take() {
            *self.unsubscribe_errors.borrow_mut() = os.close();
        }
    }
}

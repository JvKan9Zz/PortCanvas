use crate::{workbench::text, workbench_ui::Workbench};
use portcanvas_core::{plugins::ARENA_ID, services::Services};
use portcanvas_native_probe::{
    arena_model::pose_live,
    background_image,
    camera_client::{self, Frame},
    file_browser::bytes,
};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
struct Camera {
    cancel: CancellationToken,
    frames: watch::Receiver<Option<Arc<Frame>>>,
    sequence: u64,
}
pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    epoch: Cell<u64>,
    generation: Cell<u64>,
    visible: Cell<bool>,
    cancel: RefCell<CancellationToken>,
    pose_id: Cell<Option<u64>>,
    pose: RefCell<(Value, Instant)>,
    map_visible: Cell<bool>,
    map_epoch: Arc<AtomicU64>,
    devices: RefCell<Vec<String>>,
    listing: Cell<u64>,
    camera_epoch: Cell<u64>,
    camera: RefCell<Option<Camera>>,
    timer: slint::Timer,
    camera_timer: slint::Timer,
    uploads: Cell<u64>,
    visibility: RefCell<Vec<Value>>,
}
impl Controller {
    pub fn install(
        ui: &Workbench,
        services: Arc<Services>,
        runtime: Arc<Runtime>,
        jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) -> Rc<Self> {
        let c = Rc::new(Self {
            ui: ui.as_weak(),
            services,
            runtime,
            jobs,
            epoch: Cell::new(0),
            generation: Cell::new(u64::MAX),
            visible: Cell::new(false),
            cancel: RefCell::new(CancellationToken::new()),
            pose_id: Cell::new(None),
            pose: RefCell::new((json!({"phase":"unavailable"}), Instant::now())),
            map_visible: Cell::new(false),
            map_epoch: Arc::new(AtomicU64::new(0)),
            devices: RefCell::new(Vec::new()),
            listing: Cell::new(0),
            camera_epoch: Cell::new(0),
            camera: RefCell::new(None),
            timer: slint::Timer::default(),
            camera_timer: slint::Timer::default(),
            uploads: Cell::new(0),
            visibility: RefCell::new(Vec::new()),
        });
        ui.on_arena_sync({
            let c = c.clone();
            move || c.sync()
        });
        ui.on_arena_pose_started({
            let c = c.clone();
            move |ticket, id| {
                if ticket.parse::<u64>().ok() == Some(c.epoch.get()) && c.visible.get() {
                    c.pose_id.set(id.parse().ok());
                    c.pose_packet(c.services.pose.snapshot());
                }
            }
        });
        ui.on_arena_pose_packet({
            let c = c.clone();
            move |packet| {
                if let Ok(v) = serde_json::from_str(&packet) {
                    c.pose_packet(v);
                }
            }
        });
        ui.on_arena_cameras_result({
            let c = c.clone();
            move |ticket, serial, packet| {
                if ticket.parse::<u64>().ok() != Some(c.epoch.get())
                    || serial.parse::<u64>().ok() != Some(c.listing.get())
                    || !c.visible.get()
                {
                    return;
                }
                let Some(ui) = c.ui.upgrade() else { return };
                ui.set_arena_camera_loading(false);
                let v = serde_json::from_str::<Value>(&packet).unwrap_or_default();
                let items = v
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|v| v["accessible"] != false)
                    .collect::<Vec<_>>();
                let selected = c
                    .devices
                    .borrow()
                    .get(ui.get_arena_camera_index().max(0) as usize)
                    .cloned();
                *c.devices.borrow_mut() = items
                    .iter()
                    .filter_map(|v| v["path"].as_str().map(str::to_owned))
                    .collect();
                ui.set_arena_cameras(ModelRc::new(VecModel::from(
                    items
                        .iter()
                        .map(|v| {
                            format!(
                                "{} · {}",
                                v["name"].as_str().unwrap_or(""),
                                v["path"].as_str().unwrap_or("")
                            )
                            .into()
                        })
                        .collect::<Vec<slint::SharedString>>(),
                )));
                let index = selected
                    .and_then(|path| c.devices.borrow().iter().position(|p| *p == path))
                    .unwrap_or(0);
                ui.set_arena_camera_index(index as i32);
                let message = if let Some(e) = v["error"].as_str() {
                    e
                } else if items.is_empty() && v.as_array().is_some_and(|v| !v.is_empty()) {
                    "摄像头访问权限不足，请联系设备管理员。"
                } else if items.is_empty() {
                    "没有可用摄像头"
                } else {
                    "摄像头已关闭"
                };
                ui.set_arena_camera_status(text(message, ui.get_english()).into());
            }
        });
        ui.on_arena_camera_finished({
            let c = c.clone();
            move |ticket, error| {
                if ticket.parse::<u64>().ok() == Some(c.camera_epoch.get()) {
                    c.stop_camera();
                    if let Some(ui) = c.ui.upgrade() {
                        ui.set_arena_camera_status(text(&error, ui.get_english()).into());
                    }
                }
            }
        });
        ui.on_arena_toggle_camera({
            let c = c.clone();
            move || {
                if c.camera.borrow().is_some() {
                    c.stop_camera();
                } else {
                    c.start_camera();
                }
            }
        });
        ui.on_arena_refresh_cameras({
            let c = c.clone();
            move || c.list_cameras()
        });
        ui.on_arena_environment({
            let c = c.clone();
            move |packet| {
                if c.camera.borrow().is_none() {
                    return;
                }
                if let (Some(ui), Ok(v)) = (c.ui.upgrade(), serde_json::from_str::<Value>(&packet))
                {
                    if v["feature"] == "camera" {
                        let phase = v["phase"].as_str().unwrap_or("");
                        let label = match phase {
                            "downloading" => "正在下载组件…",
                            "uploading" => "正在传输组件…",
                            _ => "正在准备远端运行环境…",
                        };
                        let mut message = text(label, ui.get_english());
                        if let (Some(done), Some(total)) = (v["done"].as_u64(), v["total"].as_u64())
                        {
                            message += &format!(" {} / {}", bytes(done), bytes(total));
                        }
                        ui.set_arena_camera_status(message.into());
                    }
                }
            }
        });
        c.sync();
        c
    }
    fn spawn(&self, f: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|j| !j.is_finished());
        jobs.push(self.runtime.spawn(f));
    }
    pub fn sync(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = self.services.devices.generation();
        let selected = ui.get_page() == 9 && ui.get_plugin_active_id().as_str() == ARENA_ID;
        let shown =
            selected && !ui.get_native_hidden() && !self.services.quitting.load(Ordering::SeqCst);
        self.sync_map(&ui, shown);
        if !shown && !self.visible.get() && generation == self.generation.get() {
            return;
        }
        let active = shown && ui.get_connected();
        if active == self.visible.get() && generation == self.generation.get() {
            self.render_pose();
            return;
        }
        self.cancel.borrow().cancel();
        self.stop_camera();
        self.timer.stop();
        self.pose_id.set(None);
        self.epoch.set(self.epoch.get() + 1);
        let device_changed = self.generation.replace(generation) != generation;
        self.visible.set(active);
        *self.pose.borrow_mut() = (json!({"phase":"unavailable"}), Instant::now());
        if device_changed {
            self.devices.borrow_mut().clear();
            ui.set_arena_cameras(ModelRc::default());
            ui.set_arena_camera_index(0);
        }
        ui.set_arena_camera_loading(false);
        if std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_some() {
            self.visibility
                .borrow_mut()
                .push(json!({"active":active,"uploads":self.uploads.get()}));
        }
        self.render_pose();
        if !active {
            return;
        }
        let cancel = CancellationToken::new();
        *self.cancel.borrow_mut() = cancel.clone();
        self.list_cameras();
        let ticket = self.epoch.get();
        let s = self.services.clone();
        let weak = self.ui.clone();
        self.spawn(async move {
            if let Ok(v) = s
                .pose
                .start_for_generation(generation, cancel.clone())
                .await
            {
                let Some(id) = v["id"].as_u64() else { return };
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.invoke_arena_pose_started(ticket.to_string().into(), id.to_string().into())
                });
                cancel.cancelled().await;
                s.pose.stop(Some(id)).await;
            }
        });
        let weak = Rc::downgrade(self);
        self.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(250),
            move || {
                if let Some(c) = weak.upgrade() {
                    if c.pose_id.get().is_some() {
                        c.services.pose.touch();
                    }
                    c.render_pose();
                }
            },
        );
    }
    fn sync_map(&self, ui: &Workbench, visible: bool) {
        if visible == self.map_visible.replace(visible) {
            return;
        }
        let ticket = self.map_epoch.fetch_add(1, Ordering::Relaxed) + 1;
        ui.set_arena_has_platform_image(false);
        ui.set_arena_platform_image(Default::default());
        if !visible {
            return;
        }
        let plugins = self.services.plugins.clone();
        let weak = self.ui.clone();
        let epoch = self.map_epoch.clone();
        self.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let path = plugins.asset_path(ARENA_ID, "platformImage")?;
                background_image::load(&path, None, 2048)
            })
            .await;
            if let Ok(Ok(picture)) = result {
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    if epoch.load(Ordering::Relaxed) == ticket
                        && ui.get_page() == 9
                        && ui.get_plugin_active_id().as_str() == ARENA_ID
                        && !ui.get_native_hidden()
                    {
                        let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                            &picture.rgba,
                            picture.width,
                            picture.height,
                        );
                        ui.set_arena_platform_image(slint::Image::from_rgba8(pixels));
                        ui.set_arena_has_platform_image(true);
                    }
                });
            }
        });
    }
    fn list_cameras(self: &Rc<Self>) {
        if !self.visible.get() || self.camera.borrow().is_some() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_arena_camera_loading() {
            return;
        }
        ui.set_arena_camera_loading(true);
        ui.set_arena_camera_status(text("正在读取摄像头列表…", ui.get_english()).into());
        self.listing.set(self.listing.get() + 1);
        let serial = self.listing.get();
        let ticket = self.epoch.get();
        let generation = self.generation.get();
        let cancel = self.cancel.borrow().clone();
        let s = self.services.clone();
        let weak = self.ui.clone();
        self.spawn(async move {
            let v = s
                .arena
                .cameras_for_generation(generation, &cancel)
                .await
                .unwrap_or_else(|e| json!({"error":e.to_string()}));
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_arena_cameras_result(
                    ticket.to_string().into(),
                    serial.to_string().into(),
                    v.to_string().into(),
                )
            });
        });
    }
    fn pose_packet(&self, v: Value) {
        if self.visible.get()
            && v["generation"].as_u64() == Some(self.generation.get())
            && v["id"].as_u64() == self.pose_id.get()
        {
            *self.pose.borrow_mut() = (v, Instant::now());
            self.render_pose();
        }
    }
    fn render_pose(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let (v, received) = &*self.pose.borrow();
        let live = self.visible.get() && pose_live(v, received.elapsed().as_secs_f64() * 1000.);
        ui.set_arena_pose_live(live);
        ui.set_arena_coordinates(
            if live {
                format!(
                    "X {:.2} m / Y {:.2} m",
                    v["x"].as_f64().unwrap(),
                    v["y"].as_f64().unwrap()
                )
            } else {
                "X — m / Y — m".into()
            }
            .into(),
        );
        let phase = if v["phase"] == "live" && !live {
            "stale"
        } else {
            v["phase"].as_str().unwrap_or("unavailable")
        };
        ui.set_arena_pose_status(
            if !ui.get_connected() {
                "设备未连接"
            } else {
                match phase {
                    "live" => "定位已接入",
                    "invalid" => "定位数据无效",
                    "stale" => "定位数据已过期",
                    _ => "定位未接入",
                }
            }
            .into(),
        );
        let heading = live.then(|| v["heading"].as_f64()).flatten();
        let confidence = live.then(|| v["confidence"].as_f64()).flatten();
        let age = live
            .then(|| v["ageMs"].as_f64())
            .flatten()
            .map(|age| age + received.elapsed().as_secs_f64() * 1000.);
        ui.set_arena_heading_text(
            heading
                .map(|n| format!("{n:.0}°"))
                .unwrap_or_else(|| "—".into())
                .into(),
        );
        ui.set_arena_confidence_text(
            confidence
                .map(|n| format!("{:.0}%", n * 100.))
                .unwrap_or_else(|| "—".into())
                .into(),
        );
        ui.set_arena_pose_age_text(
            age.map(|n| format!("{n:.0} ms"))
                .unwrap_or_else(|| "—".into())
                .into(),
        );
        ui.set_arena_has_heading(heading.is_some());
        if live {
            ui.set_arena_world_x(v["x"].as_f64().unwrap() as f32);
            ui.set_arena_world_y(v["y"].as_f64().unwrap() as f32);
            ui.set_arena_heading(heading.unwrap_or(0.) as f32);
        }
    }
    fn start_camera(self: &Rc<Self>) {
        if !self.visible.get() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(path) = self
            .devices
            .borrow()
            .get(ui.get_arena_camera_index().max(0) as usize)
            .cloned()
        else {
            return;
        };
        self.camera_epoch.set(self.camera_epoch.get() + 1);
        let ticket = self.camera_epoch.get();
        let generation = self.generation.get();
        let fps = [8, 15, 20, 30][ui.get_arena_rate().clamp(0, 3) as usize];
        let cancel = CancellationToken::new();
        let (frames, receiver) = watch::channel(None);
        *self.camera.borrow_mut() = Some(Camera {
            cancel: cancel.clone(),
            frames: receiver,
            sequence: 0,
        });
        ui.set_arena_camera_active(true);
        ui.set_arena_camera_status(text("正在开启摄像头…", ui.get_english()).into());
        let s = self.services.clone();
        let weak = self.ui.clone();
        self.spawn(async move {
            let result = camera_client::run(s, generation, path, fps, frames, cancel.clone()).await;
            let error = if cancel.is_cancelled() {
                String::new()
            } else {
                result.err().map(|e| e.to_string()).unwrap_or_default()
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_arena_camera_finished(ticket.to_string().into(), error.into())
            });
        });
        let weak = Rc::downgrade(self);
        self.camera_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(33),
            move || {
                if let Some(c) = weak.upgrade() {
                    c.draw_camera();
                }
            },
        );
    }
    fn draw_camera(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut camera = self.camera.borrow_mut();
        let Some(c) = camera.as_mut() else { return };
        let frame = c.frames.borrow().clone();
        if let Some(f) = frame {
            if f.sequence != c.sequence {
                c.sequence = f.sequence;
                ui.set_arena_frame(slint::Image::from_rgb8(
                    SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&f.pixels, f.width, f.height),
                ));
                ui.set_arena_has_frame(true);
                self.uploads.set(self.uploads.get() + 1);
                let status = self.services.arena.status();
                let fps = status["streamFps"]
                    .as_f64()
                    .map(|n| format!(" · {n:.1} FPS"))
                    .unwrap_or_default();
                ui.set_arena_camera_status((text("摄像头已开启", ui.get_english()) + &fps).into());
            }
        }
    }
    fn stop_camera(&self) {
        self.camera_epoch.set(self.camera_epoch.get() + 1);
        if let Some(c) = self.camera.borrow_mut().take() {
            c.cancel.cancel();
        }
        self.camera_timer.stop();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_arena_camera_active(false);
            ui.set_arena_has_frame(false);
            ui.set_arena_frame(Default::default());
            ui.set_arena_camera_status(text("摄像头已关闭", ui.get_english()).into());
        }
    }
    pub fn shutdown(&self) {
        if let Some(ui) = self.ui.upgrade() {
            self.sync_map(&ui, false);
        }
        self.cancel.borrow().cancel();
        self.stop_camera();
        self.timer.stop();
        self.visible.set(false);
    }
    pub fn report(&self) -> Value {
        json!({"uploads":self.uploads.get(),"active":self.visible.get(),"cameraActive":self.camera.borrow().is_some(),"visibility":*self.visibility.borrow()})
    }
}

use crate::{
    workbench::text,
    workbench_ui::{VisionFact, VisionHolder, Workbench},
};
use anyhow::{ensure, Result};
use portcanvas_core::{plugins::VISION_ID, services::Services};
use portcanvas_native_probe::camera_client::{decode, Frame};
use serde_json::{json, Value};
use slint::{
    winit_030::WinitWindowAccessor, ComponentHandle, ModelRc, Rgb8Pixel, SharedPixelBuffer,
    VecModel,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const SNIPPET: &str = "import cv2, os\nif cv2.imwrite('/dev/shm/.portcanvas_cam-%d.jpg' % os.getpid(), frame): os.replace('/dev/shm/.portcanvas_cam-%d.jpg' % os.getpid(), '/dev/shm/portcanvas_cam.jpg')";
const PRESETS: [[i32; 6]; 4] = [
    [0, 10, 50, 255, 50, 255],
    [100, 124, 80, 255, 50, 255],
    [35, 85, 60, 255, 50, 255],
    [18, 34, 80, 255, 80, 255],
];

struct Sample {
    rgb: Frame,
    jpeg: Arc<Vec<u8>>,
}

struct Running {
    cancel: CancellationToken,
    config: watch::Sender<Value>,
    frame: watch::Receiver<Option<Arc<Sample>>>,
    status: watch::Receiver<Value>,
}

pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    epoch: Cell<u64>,
    generation: Cell<u64>,
    active: Cell<bool>,
    cancel: RefCell<CancellationToken>,
    inspecting: Cell<bool>,
    inspection_timer: slint::Timer,
    render_timer: slint::Timer,
    cameras: RefCell<Vec<String>>,
    windows: RefCell<Vec<String>>,
    run_id: Cell<u64>,
    running: RefCell<Option<Running>>,
    inspections: Cell<u64>,
    frames: Cell<u64>,
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
            generation: Cell::new(0),
            active: Cell::new(false),
            cancel: RefCell::new(CancellationToken::new()),
            inspecting: Cell::new(false),
            inspection_timer: slint::Timer::default(),
            render_timer: slint::Timer::default(),
            cameras: RefCell::new(Vec::new()),
            windows: RefCell::new(Vec::new()),
            run_id: Cell::new(0),
            running: RefCell::new(None),
            inspections: Cell::new(0),
            frames: Cell::new(0),
        });
        ui.on_vision_sync({
            let c = c.clone();
            move || c.sync()
        });
        ui.on_vision_toggle({
            let c = c.clone();
            move || {
                if c.running.borrow().is_some() {
                    c.stop_stream();
                } else {
                    c.start_stream();
                }
            }
        });
        ui.on_vision_refresh({
            let c = c.clone();
            move || c.inspect()
        });
        ui.on_vision_config_changed({
            let c = c.clone();
            move || c.update_config()
        });
        ui.on_vision_preset_changed({
            let c = c.clone();
            move |preset| c.preset(preset)
        });
        ui.on_vision_copy_snippet({
            let c = c.clone();
            move || c.copy_snippet()
        });
        ui.on_vision_snapshot({
            let c = c.clone();
            move || c.snapshot()
        });
        ui.on_vision_environment({
            let c = c.clone();
            move |packet| c.environment(&packet)
        });
        ui.on_vision_inspect_result({
            let c = c.clone();
            move |epoch, generation, value| {
                c.receive_inspection(&epoch, &generation, &value);
            }
        });
        ui.on_vision_stream_finished({
            let c = c.clone();
            move |ticket, error| {
                if ticket.parse().ok() == Some(c.run_id.get()) {
                    c.stop_stream();
                    if let Some(ui) = c.ui.upgrade() {
                        if !error.is_empty() {
                            ui.set_vision_status("读取失败".into());
                            ui.set_vision_status_phase(3);
                            ui.set_vision_error(text(&error, ui.get_english()).into());
                        }
                    }
                }
            }
        });
        c.sync();
        c
    }

    fn spawn(&self, task: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|job| !job.is_finished());
        jobs.push(self.runtime.spawn(task));
    }

    fn sync(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let active = ui.get_page() == 9
            && ui.get_plugin_active_id().as_str() == VISION_ID
            && ui.get_connected()
            && !ui.get_native_hidden()
            && !self
                .services
                .quitting
                .load(std::sync::atomic::Ordering::Relaxed);
        let generation = self.services.devices.generation();
        if self.active.get() == active && (!active || self.generation.get() == generation) {
            return;
        }
        self.shutdown();
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.generation.set(generation);
        self.active.set(active);
        self.inspecting.set(false);
        self.cameras.borrow_mut().clear();
        self.windows.borrow_mut().clear();
        ui.set_vision_cameras(ModelRc::default());
        ui.set_vision_windows(ModelRc::default());
        ui.set_vision_holders(ModelRc::default());
        ui.set_vision_board_busy(false);
        ui.set_vision_board_summary("".into());
        ui.set_vision_capabilities("".into());
        ui.set_vision_shared_available(false);
        ui.set_vision_error("".into());
        ui.set_vision_status(
            if active {
                "正在读取…"
            } else {
                "设备未连接"
            }
            .into(),
        );
        ui.set_vision_status_phase(if active { 1 } else { 0 });
        if !active {
            return;
        }
        *self.cancel.borrow_mut() = CancellationToken::new();
        self.inspect();
        let weak = Rc::downgrade(self);
        self.inspection_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(2500),
            move || {
                if let Some(c) = weak.upgrade() {
                    c.inspect();
                }
            },
        );
    }

    fn inspect(&self) {
        if !self.active.get() || self.inspecting.replace(true) {
            return;
        }
        self.inspections.set(self.inspections.get() + 1);
        let epoch = self.epoch.get();
        let generation = self.generation.get();
        let cancel = self.cancel.borrow().clone();
        let services = self.services.clone();
        let weak = self.ui.clone();
        self.spawn(async move {
            let packet = services
                .vision
                .inspect_for_generation(generation, &cancel)
                .await
                .unwrap_or_else(|error| json!({"error":error.to_string()}));
            if !cancel.is_cancelled() {
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.invoke_vision_inspect_result(
                        epoch.to_string().into(),
                        generation.to_string().into(),
                        packet.to_string().into(),
                    );
                });
            }
        });
    }

    fn receive_inspection(&self, epoch: &str, generation: &str, payload: &str) {
        if !self.active.get()
            || epoch.parse().ok() != Some(self.epoch.get())
            || generation.parse().ok() != Some(self.services.devices.generation())
        {
            return;
        }
        self.inspecting.set(false);
        let Some(ui) = self.ui.upgrade() else { return };
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        let english = ui.get_english();
        if let Some(error) = value["error"].as_str() {
            ui.set_vision_board_summary(text(error, english).into());
            if self.running.borrow().is_none() {
                ui.set_vision_status("读取失败".into());
                ui.set_vision_status_phase(3);
            }
            return;
        }
        let previous = self.config(&ui);
        let old_camera = selected(&self.cameras.borrow(), ui.get_vision_camera_index()).to_owned();
        let old_window = selected(&self.windows.borrow(), ui.get_vision_window_index()).to_owned();
        let mut cameras = Vec::new();
        let mut camera_names = Vec::new();
        for camera in rows(&value["cameras"]) {
            if let Some(path) = camera["path"]
                .as_str()
                .filter(|_| camera["accessible"] == true)
            {
                cameras.push(path.to_owned());
                camera_names.push(format!(
                    "{} · {path}",
                    camera["name"].as_str().unwrap_or(path)
                ));
            }
        }
        let mut windows = vec![String::new()];
        let mut window_names = vec![text("自动选择窗口", english)];
        for window in rows(&value["windows"]) {
            if let Some(id) = window["id"].as_str() {
                windows.push(id.to_owned());
                let title = if id == "screen" {
                    let description = field(window, "title");
                    let display = description
                        .split_once('(')
                        .map_or("", |(_, display)| display.trim_end_matches(')'));
                    format!("{} ({display})", text("整屏", english))
                } else {
                    field(window, "title").into_owned()
                };
                window_names.push(format!(
                    "{title} · {}×{}",
                    field(window, "width"),
                    field(window, "height")
                ));
            }
        }
        if *self.cameras.borrow() != cameras {
            ui.set_vision_camera_index(index_of(&cameras, &old_camera));
            *self.cameras.borrow_mut() = cameras;
        }
        if *self.windows.borrow() != windows {
            ui.set_vision_window_index(index_of(&windows, &old_window));
            *self.windows.borrow_mut() = windows;
        }
        ui.set_vision_cameras(model(strings(camera_names)));
        ui.set_vision_windows(model(strings(window_names)));
        let holders: Vec<_> = rows(&value["holders"])
            .map(|holder| VisionHolder {
                pid: field(holder, "pid").as_ref().into(),
                name: field(holder, "name").as_ref().into(),
                device: field(holder, "device").as_ref().into(),
                command: field(holder, "cmdline").as_ref().into(),
                stats: holder_stats(holder).into(),
            })
            .collect();
        ui.set_vision_board_busy(!holders.is_empty());
        ui.set_vision_board_summary(
            text(
                if holders.is_empty() {
                    "未发现摄像头占用"
                } else {
                    "板端程序正在使用摄像头"
                },
                english,
            )
            .into(),
        );
        ui.set_vision_holders(model(holders));
        ui.set_vision_shared_available(value["sharedFrame"]["available"] == true);
        let capabilities = [
            ("cv2", "OpenCV"),
            ("apriltag", "AprilTag"),
            ("haar", "Haar"),
            ("ffmpeg", "FFmpeg"),
            ("x11", "X11"),
        ]
        .into_iter()
        .filter_map(|(key, label)| (value["capabilities"][key] == true).then_some(label))
        .collect::<Vec<_>>()
        .join(" · ");
        ui.set_vision_capabilities(if capabilities.is_empty() {
            "—".into()
        } else {
            capabilities.into()
        });
        if self.running.borrow().is_none() {
            ui.set_vision_status("就绪".into());
            ui.set_vision_status_phase(0);
        } else if previous != self.config(&ui) {
            self.update_config();
        }
    }

    fn config(&self, ui: &Workbench) -> Value {
        json!({
            "source": (["auto","camera","watch"][ui.get_vision_source_mode().clamp(0,2) as usize]),
            "path": selected(&self.cameras.borrow(), ui.get_vision_camera_index()),
            "windowId": selected(&self.windows.borrow(), ui.get_vision_window_index()),
            "fps": ([8,15,20,30][ui.get_vision_rate().clamp(0,3) as usize]),
            "algo": ui.get_vision_algo(), "applyOnWatch": ui.get_vision_apply_on_watch(),
            "flipH": ui.get_vision_flip_h(), "flipV": ui.get_vision_flip_v(), "crosshair": ui.get_vision_crosshair(),
            "binaryMode": ui.get_vision_binary_mode(), "thresh": ui.get_vision_thresh(), "blur": ui.get_vision_blur(),
            "featureMode": ui.get_vision_feature_mode(), "cannyLow": ui.get_vision_canny_low(), "cannyHigh": ui.get_vision_canny_high(),
            "hsvPreset": ui.get_vision_hsv_preset(), "hsvView": ui.get_vision_hsv_view(),
            "hLow": ui.get_vision_h_low(), "hHigh": ui.get_vision_h_high(), "sLow": ui.get_vision_s_low(),
            "sHigh": ui.get_vision_s_high(), "vLow": ui.get_vision_v_low(), "vHigh": ui.get_vision_v_high(),
            "morph": ui.get_vision_morph(), "lineRoi": ui.get_vision_line_roi(),
            "houghParam2": ui.get_vision_hough_param2(), "minRadius": ui.get_vision_min_radius(),
            "drawContours": ui.get_vision_draw_contours(), "tagFamily": ui.get_vision_tag_family(),
            "tagGuide": ui.get_vision_tag_guide(), "faceNeighbors": ui.get_vision_face_neighbors(),
            "faceMinSize": ui.get_vision_face_min_size(),
        })
    }

    fn update_config(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if let Some(run) = self.running.borrow().as_ref() {
            run.config.send_if_modified(|previous| {
                let config = self.config(&ui);
                if *previous == config {
                    return false;
                }
                *previous = config;
                true
            });
        }
    }

    fn preset(&self, preset: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        if let Some(&[hl, hh, sl, sh, vl, vh]) =
            usize::try_from(preset).ok().and_then(|i| PRESETS.get(i))
        {
            ui.set_vision_h_low(hl);
            ui.set_vision_h_high(hh);
            ui.set_vision_s_low(sl);
            ui.set_vision_s_high(sh);
            ui.set_vision_v_low(vl);
            ui.set_vision_v_high(vh);
        }
        self.update_config();
    }

    fn start_stream(self: &Rc<Self>) {
        if !self.active.get() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else { return };
        let initial = self.config(&ui);
        if initial["source"] == "camera" && initial["path"] == "" {
            ui.set_vision_error(text("请选择有效的视频设备。", ui.get_english()).into());
            return;
        }
        let ticket = self.run_id.get().wrapping_add(1);
        self.run_id.set(ticket);
        let cancel = self.cancel.borrow().child_token();
        let generation = self.generation.get();
        let (config, input) = watch::channel(initial);
        let (frames, frame) = watch::channel(None);
        let (status, diagnostics) = watch::channel(Value::Null);
        *self.running.borrow_mut() = Some(Running {
            cancel: cancel.clone(),
            config,
            frame,
            status: diagnostics,
        });
        ui.set_vision_error("".into());
        ui.set_vision_active(true);
        ui.set_vision_loading(true);
        ui.set_vision_status("正在开启画面…".into());
        ui.set_vision_status_phase(1);
        let services = self.services.clone();
        let weak = self.ui.clone();
        self.spawn(async move {
            let result =
                run_stream(services, generation, input, frames, status, cancel.clone()).await;
            if !cancel.is_cancelled() {
                let error = result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.invoke_vision_stream_finished(ticket.to_string().into(), error.into())
                });
            }
        });
        let weak = Rc::downgrade(self);
        self.render_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(33),
            move || {
                if let Some(c) = weak.upgrade() {
                    c.draw();
                }
            },
        );
    }

    fn draw(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut running = self.running.borrow_mut();
        let Some(run) = running.as_mut() else { return };
        if run.frame.has_changed().unwrap_or(false) {
            if let Some(sample) = run.frame.borrow_and_update().as_ref() {
                let image = &sample.rgb;
                ui.set_vision_frame(slint::Image::from_rgb8(
                    SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
                        &image.pixels,
                        image.width,
                        image.height,
                    ),
                ));
                ui.set_vision_has_frame(true);
                self.frames.set(self.frames.get() + 1);
            } else {
                ui.set_vision_has_frame(false);
                ui.set_vision_frame(Default::default());
            }
        }
        if run.status.has_changed().unwrap_or(false) {
            let status = run.status.borrow_and_update();
            let telemetry = &status["telemetry"];
            let live = status["phase"] == "connected";
            let waiting = status["phase"] == "waiting";
            ui.set_vision_loading(!live && !waiting);
            ui.set_vision_status_phase(if live { 2 } else { 1 });
            let english = ui.get_english();
            ui.set_vision_status(
                if live {
                    "实时"
                } else if waiting {
                    "等待板端画面"
                } else {
                    "正在开启画面…"
                }
                .into(),
            );
            ui.set_vision_source_badge(text(&field(telemetry, "sourceLabel"), english).into());
            ui.set_vision_telemetry_summary(text(&field(telemetry, "summary"), english).into());
            let mut facts = Vec::new();
            if live {
                if let (Some(width), Some(height)) =
                    (telemetry["width"].as_u64(), telemetry["height"].as_u64())
                {
                    if width > 0 && height > 0 {
                        facts.push(VisionFact {
                            label: text("分辨率", english).into(),
                            value: format!("{width}×{height}").into(),
                        });
                    }
                }
                for (key, label, unit) in [("fps", "帧率", "FPS"), ("procMs", "处理耗时", "ms")]
                {
                    if let Some(number) = telemetry[key].as_f64() {
                        facts.push(VisionFact {
                            label: text(label, english).into(),
                            value: format!("{number:.1} {unit}").into(),
                        });
                    }
                }
            }
            facts.extend(rows(&telemetry["details"]).take(24).map(|row| VisionFact {
                label: text(&field(row, "label"), english).into(),
                value: text(&field(row, "value"), english).into(),
            }));
            ui.set_vision_facts(model(facts));
        }
    }

    fn stop_stream(&self) {
        self.run_id.set(self.run_id.get().wrapping_add(1));
        if let Some(run) = self.running.borrow_mut().take() {
            run.cancel.cancel();
        }
        self.render_timer.stop();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_vision_active(false);
            ui.set_vision_loading(false);
            ui.set_vision_has_frame(false);
            ui.set_vision_frame(Default::default());
            ui.set_vision_source_badge("".into());
            ui.set_vision_telemetry_summary("".into());
            ui.set_vision_facts(ModelRc::default());
            ui.set_vision_status("就绪".into());
            ui.set_vision_status_phase(0);
        }
    }

    fn environment(&self, packet: &str) {
        if self.running.borrow().is_none() {
            return;
        }
        if let (Some(ui), Ok(value)) = (self.ui.upgrade(), serde_json::from_str::<Value>(packet)) {
            if value["feature"] == "vision" {
                ui.set_vision_status("正在准备远端运行环境…".into());
            }
        }
    }

    fn copy_snippet(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(SNIPPET)) {
            Ok(()) => ui.invoke_notify("共享帧代码已复制".into()),
            Err(error) => ui.set_vision_error(error.to_string().into()),
        }
    }

    fn snapshot(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let sample = self
            .running
            .borrow()
            .as_ref()
            .and_then(|run| run.frame.borrow().clone());
        let Some(sample) = sample else { return };
        let path = ui
            .window()
            .with_winit_window(|window| {
                rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(text("保存视觉快照", ui.get_english()))
                    .add_filter("JPEG", &["jpg", "jpeg"])
                    .set_file_name("vision-snapshot.jpg")
                    .save_file()
            })
            .flatten();
        let Some(path) = path else { return };
        let weak = self.ui.clone();
        self.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                portcanvas_core::profiles::atomic_bytes(&path, &sample.jpeg)
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result);
            let _ = weak.upgrade_in_event_loop(move |ui| match result {
                Ok(()) => ui.invoke_notify("视觉快照已保存".into()),
                Err(error) => ui.set_vision_error(error.to_string().into()),
            });
        });
    }

    pub fn shutdown(&self) {
        self.active.set(false);
        self.inspection_timer.stop();
        self.cancel.borrow().cancel();
        self.stop_stream();
    }

    pub fn report(&self) -> Value {
        json!({"active":self.active.get(),"streaming":self.running.borrow().is_some(),
               "inspections":self.inspections.get(),"frames":self.frames.get()})
    }
}

async fn run_stream(
    services: Arc<Services>,
    generation: u64,
    mut config: watch::Receiver<Value>,
    frames: watch::Sender<Option<Arc<Sample>>>,
    status: watch::Sender<Value>,
    cancel: CancellationToken,
) -> Result<()> {
    let initial = config.borrow_and_update().clone();
    let id = services
        .vision
        .start(initial, generation, cancel.clone())
        .await?;
    let result = async {
        let mut tick = tokio::time::interval(Duration::from_millis(33));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut sequence = 0;
        let mut poll_status = Instant::now();
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                _ = tick.tick() => {}
            }
            if config.has_changed()? {
                services
                    .vision
                    .update_config(config.borrow_and_update().clone())?;
            }
            if poll_status <= Instant::now() {
                let value = services.vision.status();
                ensure!(
                    value["id"].as_u64() == Some(id)
                        && value["generation"].as_u64() == Some(generation),
                    "设备连接已变化。"
                );
                ensure!(
                    !matches!(value["phase"].as_str(), Some("idle" | "error")),
                    "{}",
                    value["message"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .unwrap_or("视觉采集已结束。")
                );
                if value["phase"] == "waiting" && frames.borrow().is_some() {
                    frames.send_replace(None);
                }
                status.send_if_modified(|old| {
                    if old["phase"] == value["phase"] && old["telemetry"] == value["telemetry"] {
                        return false;
                    }
                    *old = value;
                    true
                });
                poll_status = Instant::now() + Duration::from_millis(180);
            }
            if let Some(frame) = services.vision.native_frame(id, sequence, generation)? {
                sequence = frame.sequence;
                let sample = tokio::task::spawn_blocking(move || -> Result<_> {
                    let rgb = decode(frame.sequence, &frame.jpeg)?;
                    Ok(Sample {
                        rgb,
                        jpeg: frame.jpeg,
                    })
                })
                .await??;
                if !cancel.is_cancelled() {
                    frames.send_replace(Some(Arc::new(sample)));
                }
            }
        }
    }
    .await;
    services.vision.stop(Some(id)).await;
    result
}

fn rows(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

fn field<'a>(value: &'a Value, key: &str) -> std::borrow::Cow<'a, str> {
    let value = &value[key];
    if let Some(text) = value.as_str() {
        text.into()
    } else if value.is_number() {
        value.to_string().into()
    } else {
        "".into()
    }
}

fn selected(values: &[String], index: i32) -> &str {
    usize::try_from(index)
        .ok()
        .and_then(|index| values.get(index))
        .map_or("", String::as_str)
}

fn index_of(values: &[String], selected: &str) -> i32 {
    values
        .iter()
        .position(|value| value == selected)
        .unwrap_or(0) as i32
}

fn model<T: Clone + 'static>(values: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(values))
}

fn strings(values: Vec<String>) -> Vec<slint::SharedString> {
    values.into_iter().map(Into::into).collect()
}

fn holder_stats(holder: &Value) -> String {
    let mut values = Vec::new();
    if let Some(cpu) = holder["cpuPercent"].as_f64() {
        values.push(format!("CPU {cpu:.1}%"));
    }
    if let Some(memory) = holder["memoryMb"].as_f64() {
        values.push(format!("{memory:.1} MiB"));
    }
    if let Some(uptime) = holder["uptimeSec"].as_f64() {
        values.push(format!("{uptime:.0} s"));
    }
    if values.is_empty() {
        "—".into()
    } else {
        values.join(" · ")
    }
}

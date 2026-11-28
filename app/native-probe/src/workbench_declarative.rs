//! Generic declarative native plugin runtime controller (`declarative-v1`).
//! Drives both `local-worker` (`pc-worker-v1`) and `ssh` plugins purely from `manifest.json` + `ui.json`.
use crate::{
    workbench::text,
    workbench_ui::{
        DeclBlock, DeclButton, DeclControl, DeclKpi, DeclTask, DeclTaskAction, Workbench,
    },
};
use anyhow::{anyhow, Result};
use portcanvas_core::{
    plugin_runtime::{
        validate_open_path, BlockSpec, Condition, ControlSpec, FrameFormat, LocalWorkerHandle,
        OptionSpec, UiSpec, WorkerEvent, WorkerInit, WorkerVideoFrame,
    },
    plugins::{Manifest, PluginRuntimeConfig},
    services::Services,
};
use portcanvas_native_probe::{
    arena_model::pose_live,
    camera_client::{self, decode, Frame},
    debug_log,
    file_browser::bytes,
};
use serde_json::{json, Map, Value};
use slint::{
    winit_030::WinitWindowAccessor, ComponentHandle, Model, ModelRc, Rgb8Pixel, Rgba8Pixel,
    SharedPixelBuffer, SharedString, VecModel,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    runtime::Runtime,
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default)]
pub struct BackgroundBadge {
    pub active: bool,
    pub summary: String,
}

enum RuntimeKind {
    LocalWorker {
        handle: Arc<LocalWorkerHandle>,
        events_rx: RefCell<mpsc::UnboundedReceiver<WorkerEvent>>,
        frame_rx: watch::Receiver<Option<Arc<WorkerVideoFrame>>>,
        last_seq: u64,
    },
    SshArena {
        cancel: CancellationToken,
        pose_id: Option<u64>,
        pose_packet: (Value, Instant),
        cameras: Vec<String>,
        camera_epoch: u64,
        camera_cancel: Option<CancellationToken>,
        camera_rx: Option<watch::Receiver<Option<Arc<Frame>>>>,
        last_seq: u64,
        platform_image: Option<slint::Image>,
    },
    SshVision {
        cancel: CancellationToken,
        inspecting: bool,
        last_inspect: Instant,
        cameras: Vec<String>,
        windows: Vec<String>,
        run_id: u64,
        stream_cancel: Option<CancellationToken>,
        config_tx: Option<watch::Sender<Value>>,
        frame_rx: Option<watch::Receiver<Option<Arc<VisionSample>>>>,
        status_rx: Option<watch::Receiver<Value>>,
        last_seq: u64,
    },
    SshGeneric {
        cancel: CancellationToken,
    },
}

struct VisionSample {
    rgb: Frame,
    jpeg: Arc<Vec<u8>>,
}

struct PluginSession {
    id: String,
    manifest: Manifest,
    spec: UiSpec,
    state: Map<String, Value>,
    error: String,
    runtime: RuntimeKind,
    latest_rgba: Option<(u32, u32, Vec<u8>)>,
    latest_jpeg: Option<Arc<Vec<u8>>>,
    has_frame: bool,
    last_frame_image: Option<slint::Image>,
    generation: u64,
    background_badge: BackgroundBadge,
}

pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    sessions: RefCell<BTreeMap<String, PluginSession>>,
    active_id: RefCell<String>,
    pump_timer: slint::Timer,
    background_badges: Arc<Mutex<BTreeMap<String, BackgroundBadge>>>,
    on_badges_changed: RefCell<Option<Rc<dyn Fn()>>>,
    epoch: Cell<u64>,
    primary_model: Rc<VecModel<DeclBlock>>,
    secondary_model: Rc<VecModel<DeclBlock>>,
}

impl Controller {
    pub fn install(
        ui: &Workbench,
        services: Arc<Services>,
        runtime: Arc<Runtime>,
        jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) -> Rc<Self> {
        let primary_model = Rc::new(VecModel::<DeclBlock>::default());
        let secondary_model = Rc::new(VecModel::<DeclBlock>::default());
        ui.set_decl_primary_blocks(ModelRc::from(primary_model.clone()));
        ui.set_decl_secondary_blocks(ModelRc::from(secondary_model.clone()));
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            services,
            runtime,
            jobs,
            sessions: RefCell::new(BTreeMap::new()),
            active_id: RefCell::new(String::new()),
            pump_timer: slint::Timer::default(),
            background_badges: Arc::new(Mutex::new(BTreeMap::new())),
            on_badges_changed: RefCell::new(None),
            epoch: Cell::new(0),
            primary_model,
            secondary_model,
        });

        ui.on_decl_sync({
            let c = controller.clone();
            move || c.sync()
        });
        ui.on_decl_set_int({
            let c = controller.clone();
            move |key, val| c.on_state_change(&key, json!(val))
        });
        ui.on_decl_set_float({
            let c = controller.clone();
            move |key, val| c.on_state_change(&key, json!(val))
        });
        ui.on_decl_set_bool({
            let c = controller.clone();
            move |key, val| c.on_state_change(&key, json!(val))
        });
        ui.on_decl_set_string({
            let c = controller.clone();
            move |key, val| c.on_state_change(&key, json!(val.as_str()))
        });
        ui.on_decl_trigger_action({
            let c = controller.clone();
            move |action, item_id, payload| c.on_action(&action, &item_id, &payload)
        });
        ui.on_decl_pick_directory({
            let c = controller.clone();
            move |state_key, action| c.on_pick_directory(&state_key, &action)
        });
        ui.on_decl_copy_text({
            let c = controller.clone();
            move |content| c.on_copy_text(&content)
        });
        ui.on_decl_ssh_event({
            let c = controller.clone();
            move |plugin_id, event_kind, payload| {
                c.on_ssh_event(&plugin_id, &event_kind, &payload);
            }
        });

        let weak_ctrl = Rc::downgrade(&controller);
        controller.pump_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || {
                if let Some(c) = weak_ctrl.upgrade() {
                    c.tick();
                }
            },
        );

        controller
    }

    pub fn set_badge_listener(&self, listener: Rc<dyn Fn()>) {
        *self.on_badges_changed.borrow_mut() = Some(listener);
    }

    pub fn background_badges(&self) -> BTreeMap<String, BackgroundBadge> {
        self.background_badges.lock().unwrap().clone()
    }

    pub fn stop_plugin(&self, id: &str) {
        let removed = self.sessions.borrow_mut().remove(id);
        if let Some(session) = removed {
            self.teardown_session(session);
        }
        {
            let mut badges = self.background_badges.lock().unwrap();
            badges.remove(id);
        }
        self.notify_badges_changed();
        if self.active_id.borrow().as_str() == id {
            self.active_id.borrow_mut().clear();
            self.clear_ui();
        }
    }

    pub fn shutdown(&self) {
        self.pump_timer.stop();
        let sessions: Vec<PluginSession> = {
            let mut map = self.sessions.borrow_mut();
            let keys: Vec<String> = map.keys().cloned().collect();
            keys.into_iter().filter_map(|k| map.remove(&k)).collect()
        };
        for session in sessions {
            self.teardown_session(session);
        }
        self.background_badges.lock().unwrap().clear();
    }

    pub fn report(&self) -> Value {
        let sessions = self.sessions.borrow();
        json!({
            "activePlugin": self.active_id.borrow().as_str(),
            "runningSessions": sessions.keys().cloned().collect::<Vec<_>>(),
            "backgroundBadges": self.background_badges.lock().unwrap().iter().map(|(k, v)| (k.clone(), json!({"active": v.active, "summary": v.summary}))).collect::<Map<String, Value>>(),
        })
    }

    fn spawn_job(&self, fut: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|j| !j.is_finished());
        jobs.push(self.runtime.spawn(fut));
    }

    fn notify_badges_changed(&self) {
        if let Some(cb) = self.on_badges_changed.borrow().as_ref() {
            cb();
        }
    }

    fn teardown_session(&self, session: PluginSession) {
        match session.runtime {
            RuntimeKind::LocalWorker { handle, .. } => {
                self.spawn_job(async move {
                    handle.shutdown().await;
                });
            }
            RuntimeKind::SshArena {
                cancel,
                pose_id,
                camera_cancel,
                ..
            } => {
                cancel.cancel();
                if let Some(c) = camera_cancel {
                    c.cancel();
                }
                let services = self.services.clone();
                self.spawn_job(async move {
                    if let Some(pid) = pose_id {
                        services.pose.stop(Some(pid)).await;
                    }
                    services.arena.stop(None).await;
                });
            }
            RuntimeKind::SshVision {
                cancel,
                stream_cancel,
                ..
            } => {
                cancel.cancel();
                if let Some(c) = stream_cancel {
                    c.cancel();
                }
                let services = self.services.clone();
                self.spawn_job(async move {
                    services.vision.stop(None).await;
                });
            }
            RuntimeKind::SshGeneric { cancel } => {
                cancel.cancel();
            }
        }
    }

    pub fn sync(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let on_plugin_page = ui.get_page() == 9 && !ui.get_plugin_active_id().is_empty();
        let hidden = ui.get_native_hidden()
            || self
                .services
                .quitting
                .load(std::sync::atomic::Ordering::SeqCst);
        let desired_id = if on_plugin_page && !hidden {
            ui.get_plugin_active_id().to_string()
        } else {
            String::new()
        };

        let prev_id = self.active_id.borrow().clone();
        if prev_id != desired_id {
            if !prev_id.is_empty() {
                let should_keep = self
                    .sessions
                    .borrow()
                    .get(&prev_id)
                    .is_some_and(|s| s.manifest.is_background());
                if should_keep {
                    if let Some(session) = self.sessions.borrow().get(&prev_id) {
                        if let RuntimeKind::LocalWorker { handle, .. } = &session.runtime {
                            let h = handle.clone();
                            self.spawn_job(async move {
                                let _ = h.set_visibility(false).await;
                            });
                        }
                    }
                } else if let Some(session) = self.sessions.borrow_mut().remove(&prev_id) {
                    self.teardown_session(session);
                }
            }
            *self.active_id.borrow_mut() = desired_id.clone();
        }

        if desired_id.is_empty() {
            self.clear_ui();
            return;
        }

        // Check if device connection changed for SSH plugins
        let gen = self.services.devices.generation();
        let connected = ui.get_connected();
        let need_restart_ssh = self.sessions.borrow().get(&desired_id).is_some_and(|s| {
            s.manifest.requires_device() && (s.generation != gen || !connected)
        });
        if need_restart_ssh {
            if let Some(old) = self.sessions.borrow_mut().remove(&desired_id) {
                self.teardown_session(old);
            }
        }

        if !self.sessions.borrow().contains_key(&desired_id) {
            if let Err(err) = self.open_session(&desired_id, &ui) {
                debug_log::error("declarative", &format!("open {desired_id} failed: {err:#}"));
                ui.set_decl_error(text(&err.to_string(), ui.get_english()).into());
                return;
            }
        } else if let Some(session) = self.sessions.borrow().get(&desired_id) {
            if let RuntimeKind::LocalWorker { handle, .. } = &session.runtime {
                let h = handle.clone();
                self.spawn_job(async move {
                    let _ = h.set_visibility(true).await;
                });
            }
        }

        self.render_active();
    }

    fn open_session(&self, id: &str, ui: &Workbench) -> Result<()> {
        let manifest = self.services.plugins.active_manifest(id)?;
        let spec_text = self.services.plugins.ui_spec_text(id)?;
        let spec = UiSpec::parse(&spec_text)?;
        let mut state = Map::new();
        for (k, v) in &spec.initial_state {
            state.insert(k.clone(), v.clone());
        }
        let gen = self.services.devices.generation();
        let connected = ui.get_connected();

        let runtime = match &manifest.runtime {
            Some(PluginRuntimeConfig::LocalWorker { .. }) => {
                let (exe, args, frame_channel, plugin_cancel) =
                    self.services.plugins.worker_executable(id)?;
                let work_dir = self.services.plugins.plugin_dir(id)?;
                let data_dir = self.services.plugins.data_dir(id)?;
                let default_download_dir = default_downloads_dir();
                let language = if ui.get_english() { "en" } else { "zh" }.to_owned();
                let (tx, rx) = mpsc::unbounded_channel();
                let init = WorkerInit {
                    plugin_id: id.to_owned(),
                    executable: exe,
                    args,
                    working_dir: work_dir,
                    data_dir,
                    default_download_dir,
                    language,
                    frame_channel,
                };
                let handle = self
                    .runtime
                    .block_on(LocalWorkerHandle::spawn(init, plugin_cancel, tx))?;
                let frame_rx = handle.frames();
                RuntimeKind::LocalWorker {
                    handle: Arc::new(handle),
                    events_rx: RefCell::new(rx),
                    frame_rx,
                    last_seq: 0,
                }
            }
            Some(PluginRuntimeConfig::Ssh { adapter }) => match adapter.as_str() {
                "arena" => {
                    let platform_image = self
                        .services
                        .plugins
                        .asset_path(id, "platformImage")
                        .ok()
                        .and_then(|p| image::open(&p).ok())
                        .map(|img| {
                            let rgba = img.to_rgba8();
                            let (w, h) = rgba.dimensions();
                            let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                rgba.as_raw(), w, h,
                            );
                            slint::Image::from_rgba8(buf)
                        });
                    let cancel = CancellationToken::new();
                    if connected {
                        self.start_arena_background(id, gen, &cancel);
                    }
                    RuntimeKind::SshArena {
                        cancel,
                        pose_id: None,
                        pose_packet: (json!({"phase": "unavailable"}), Instant::now()),
                        cameras: Vec::new(),
                        camera_epoch: 0,
                        camera_cancel: None,
                        camera_rx: None,
                        last_seq: 0,
                        platform_image,
                    }
                }
                "vision" => {
                    let cancel = CancellationToken::new();
                    if connected {
                        self.trigger_vision_inspect(id, gen, &cancel);
                    }
                    RuntimeKind::SshVision {
                        cancel,
                        inspecting: connected,
                        last_inspect: Instant::now(),
                        cameras: Vec::new(),
                        windows: Vec::new(),
                        run_id: 0,
                        stream_cancel: None,
                        config_tx: None,
                        frame_rx: None,
                        status_rx: None,
                        last_seq: 0,
                    }
                }
                _ => RuntimeKind::SshGeneric {
                    cancel: CancellationToken::new(),
                },
            },
            None => return Err(anyhow!("插件未声明 runtime 配置。")),
        };

        let session = PluginSession {
            id: id.to_owned(),
            manifest,
            spec,
            state,
            error: String::new(),
            runtime,
            latest_rgba: None,
            latest_jpeg: None,
            has_frame: false,
            last_frame_image: None,
            generation: gen,
            background_badge: BackgroundBadge::default(),
        };
        self.sessions.borrow_mut().insert(id.to_owned(), session);
        Ok(())
    }

    fn start_arena_background(&self, plugin_id: &str, gen: u64, cancel: &CancellationToken) {
        let services = self.services.clone();
        let weak = self.ui.clone();
        let pid = plugin_id.to_owned();
        let c1 = cancel.clone();
        self.spawn_job(async move {
            let result = services
                .arena
                .cameras_for_generation(gen, &c1)
                .await
                .unwrap_or_else(|e| json!({"error": e.to_string()}));
            if !c1.is_cancelled() {
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.invoke_decl_ssh_event(
                        pid.into(),
                        "arena-cameras".into(),
                        result.to_string().into(),
                    );
                });
            }
        });

        let services = self.services.clone();
        let weak = self.ui.clone();
        let pid = plugin_id.to_owned();
        let c2 = cancel.clone();
        self.spawn_job(async move {
            if let Ok(snap) = services.pose.start_for_generation(gen, c2.clone()).await {
                if !c2.is_cancelled() {
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.invoke_decl_ssh_event(
                            pid.into(),
                            "arena-pose-started".into(),
                            snap.to_string().into(),
                        );
                    });
                }
            }
        });
    }

    fn trigger_vision_inspect(&self, plugin_id: &str, gen: u64, cancel: &CancellationToken) {
        let services = self.services.clone();
        let weak = self.ui.clone();
        let pid = plugin_id.to_owned();
        let c = cancel.clone();
        self.spawn_job(async move {
            let packet = services
                .vision
                .inspect_for_generation(gen, &c)
                .await
                .unwrap_or_else(|e| json!({"error": e.to_string()}));
            if !c.is_cancelled() {
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.invoke_decl_ssh_event(
                        pid.into(),
                        "vision-inspect".into(),
                        packet.to_string().into(),
                    );
                });
            }
        });
    }

    pub fn forward_core_event(&self, event_name: &str, payload: &Value) {
        let mut need_render = false;
        {
            let mut sessions = self.sessions.borrow_mut();
            for session in sessions.values_mut() {
                match (&mut session.runtime, event_name) {
                    (RuntimeKind::SshArena { pose_packet, .. }, "arena-pose") => {
                        *pose_packet = (payload.clone(), Instant::now());
                        apply_arena_pose_to_state(&mut session.state, payload);
                        if self.active_id.borrow().as_str() == session.id {
                            need_render = true;
                        }
                    }
                    (RuntimeKind::SshArena { .. }, "environment-progress")
                        if payload["feature"] == "camera" =>
                    {
                        let phase = payload["phase"].as_str().unwrap_or("");
                        let label = match phase {
                            "downloading" => "正在下载组件…",
                            "uploading" => "正在传输组件…",
                            _ => "正在准备远端运行环境…",
                        };
                        let mut msg = label.to_owned();
                        if let (Some(done), Some(total)) =
                            (payload["done"].as_u64(), payload["total"].as_u64())
                        {
                            msg += &format!(" {} / {}", bytes(done), bytes(total));
                        }
                        session
                            .state
                            .insert("cameraStatus".into(), json!(msg));
                        if self.active_id.borrow().as_str() == session.id {
                            need_render = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        if need_render {
            self.render_active();
        }
    }

    fn on_ssh_event(&self, plugin_id: &str, kind: &str, payload: &str) {
        let Ok(val) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        let mut need_render = false;
        {
            let mut sessions = self.sessions.borrow_mut();
            if let Some(session) = sessions.get_mut(plugin_id) {
                match (&mut session.runtime, kind) {
                    (
                        RuntimeKind::SshArena {
                            cameras,
                            pose_id,
                            pose_packet,
                            ..
                        },
                        "arena-cameras",
                    ) => {
                        session.state.insert("cameraLoading".into(), json!(false));
                        let items: Vec<&Value> = val
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|v| v["accessible"] != false)
                            .collect();
                        *cameras = items
                            .iter()
                            .filter_map(|v| v["path"].as_str().map(str::to_owned))
                            .collect();
                        let options: Vec<Value> = items
                            .iter()
                            .map(|v| {
                                json!({
                                    "label": format!(
                                        "{} · {}",
                                        v["name"].as_str().unwrap_or(""),
                                        v["path"].as_str().unwrap_or("")
                                    ),
                                    "value": v["path"].as_str().unwrap_or("")
                                })
                            })
                            .collect();
                        session.state.insert("cameraOptions".into(), Value::Array(options));
                        let msg = if let Some(e) = val["error"].as_str() {
                            e
                        } else if items.is_empty()
                            && val.as_array().is_some_and(|a| !a.is_empty())
                        {
                            "摄像头访问权限不足，请联系设备管理员。"
                        } else if items.is_empty() {
                            "没有可用摄像头"
                        } else {
                            "摄像头已关闭"
                        };
                        session.state.insert("cameraStatus".into(), json!(msg));
                        let _ = (pose_id, pose_packet);
                        need_render = true;
                    }
                    (
                        RuntimeKind::SshArena {
                            pose_id,
                            pose_packet,
                            ..
                        },
                        "arena-pose-started",
                    ) => {
                        *pose_id = val["id"].as_u64();
                        let snap = self.services.pose.snapshot();
                        *pose_packet = (snap.clone(), Instant::now());
                        apply_arena_pose_to_state(&mut session.state, &snap);
                        need_render = true;
                    }
                    (
                        RuntimeKind::SshArena {
                            camera_cancel,
                            camera_rx,
                            ..
                        },
                        "arena-camera-finished",
                    ) => {
                        if let Some(c) = camera_cancel.take() {
                            c.cancel();
                        }
                        *camera_rx = None;
                        session.state.insert("cameraActive".into(), json!(false));
                        session.state.insert("cameraLoading".into(), json!(false));
                        if let Some(err) = val["error"].as_str().filter(|s| !s.is_empty()) {
                            session.state.insert("cameraStatus".into(), json!(err));
                        }
                        session.has_frame = false;
                        need_render = true;
                    }
                    (
                        RuntimeKind::SshVision {
                            inspecting,
                            cameras,
                            windows,
                            ..
                        },
                        "vision-inspect",
                    ) => {
                        *inspecting = false;
                        if let Some(err) = val["error"].as_str() {
                            session.error = err.to_owned();
                            session.state.insert("status".into(), json!("巡检失败"));
                            session.state.insert("statusPhase".into(), json!(3));
                        } else {
                            session.error.clear();
                            let cam_items: Vec<&Value> = val["cameras"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter(|v| v["accessible"] != false)
                                .collect();
                            *cameras = cam_items
                                .iter()
                                .filter_map(|v| v["path"].as_str().map(str::to_owned))
                                .collect();
                            let cam_opts: Vec<Value> = cam_items
                                .iter()
                                .map(|v| {
                                    let holder = v["heldBy"].as_str().unwrap_or("");
                                    let label = if holder.is_empty() {
                                        format!(
                                            "{} · {}",
                                            v["name"].as_str().unwrap_or(""),
                                            v["path"].as_str().unwrap_or("")
                                        )
                                    } else {
                                        format!(
                                            "{} · {} ({})",
                                            v["name"].as_str().unwrap_or(""),
                                            v["path"].as_str().unwrap_or(""),
                                            holder
                                        )
                                    };
                                    json!({"label": label, "value": v["path"].as_str().unwrap_or("")})
                                })
                                .collect();
                            session
                                .state
                                .insert("cameraOptions".into(), Value::Array(cam_opts));

                            let win_items: Vec<&Value> = val["windows"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .collect();
                            *windows = win_items
                                .iter()
                                .filter_map(|v| v["id"].as_str().map(str::to_owned))
                                .collect();
                            let win_opts: Vec<Value> = win_items
                                .iter()
                                .map(|v| {
                                    json!({
                                        "label": format!(
                                            "{} ({})",
                                            v["title"].as_str().unwrap_or("Window"),
                                            v["size"].as_str().unwrap_or("")
                                        ),
                                        "value": v["id"].as_str().unwrap_or("")
                                    })
                                })
                                .collect();
                            session
                                .state
                                .insert("windowOptions".into(), Value::Array(win_opts));

                            let holders: Vec<Value> = val["holders"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(|h| {
                                    let pid = h["pid"].as_u64().unwrap_or(0);
                                    let cmd = h["command"].as_str().unwrap_or("");
                                    let dev = h["device"].as_str().unwrap_or("");
                                    json!({
                                        "id": pid.to_string(),
                                        "title": format!("PID {pid} · {cmd}"),
                                        "subtitle": dev,
                                        "badge": "占用中",
                                        "badgeTone": "accent",
                                        "showProgress": false,
                                        "actions": []
                                    })
                                })
                                .collect();
                            let busy = !holders.is_empty();
                            session.state.insert("boardBusy".into(), json!(busy));
                            session.state.insert(
                                "boardSummary".into(),
                                json!(if busy {
                                    format!("{} 个进程正在使用摄像头", holders.len())
                                } else {
                                    "摄像头空闲，可直接打开".into()
                                }),
                            );
                            session.state.insert("holders".into(), Value::Array(holders));

                            let mut caps = Vec::new();
                            if val["capabilities"]["opencv"] == true {
                                caps.push("OpenCV");
                            }
                            if val["capabilities"]["apriltag"] == true {
                                caps.push("AprilTag");
                            }
                            if val["capabilities"]["haar"] == true {
                                caps.push("Haar");
                            }
                            if val["sharedFrame"]["available"] == true {
                                caps.push("共享帧");
                            }
                            session
                                .state
                                .insert("capabilities".into(), json!(caps.join(" · ")));

                            if session.state.get("active").and_then(Value::as_bool) != Some(true) {
                                session.state.insert("status".into(), json!("就绪"));
                                session.state.insert("statusPhase".into(), json!(2));
                            }
                        }
                        need_render = true;
                    }
                    (
                        RuntimeKind::SshVision {
                            stream_cancel,
                            config_tx,
                            frame_rx,
                            status_rx,
                            ..
                        },
                        "vision-stream-finished",
                    ) => {
                        if let Some(c) = stream_cancel.take() {
                            c.cancel();
                        }
                        *config_tx = None;
                        *frame_rx = None;
                        *status_rx = None;
                        session.state.insert("active".into(), json!(false));
                        session.state.insert("loading".into(), json!(false));
                        if let Some(err) = val["error"].as_str().filter(|s| !s.is_empty()) {
                            session.error = err.to_owned();
                            session.state.insert("status".into(), json!("采集失败"));
                            session.state.insert("statusPhase".into(), json!(3));
                        } else {
                            session.state.insert("status".into(), json!("已停止"));
                            session.state.insert("statusPhase".into(), json!(0));
                        }
                        session.has_frame = false;
                        need_render = true;
                    }
                    _ => {}
                }
            }
        }
        if need_render && self.active_id.borrow().as_str() == plugin_id {
            self.render_active();
        }
    }

    fn tick(&self) {
        let active_id = self.active_id.borrow().clone();
        let mut need_render = false;
        let mut badges_dirty = false;
        let mut toasts = Vec::new();
        let mut copies = Vec::new();
        let mut opens = Vec::new();
        let mut new_frame_for_active: Option<slint::Image> = None;
        let mut pending_vision_inspect: Option<(String, u64, CancellationToken)> = None;

        {
            let mut sessions = self.sessions.borrow_mut();
            for session in sessions.values_mut() {
                let is_active = session.id == active_id;
                match &mut session.runtime {
                    RuntimeKind::LocalWorker {
                        events_rx,
                        frame_rx,
                        last_seq,
                        ..
                    } => {
                        let mut rx = events_rx.borrow_mut();
                        while let Ok(ev) = rx.try_recv() {
                            match ev {
                                WorkerEvent::StatePatch(patch) => {
                                    for (k, v) in patch {
                                        if k == "backgroundActive" {
                                            let active = v.as_bool().unwrap_or(false);
                                            if session.background_badge.active != active {
                                                session.background_badge.active = active;
                                                badges_dirty = true;
                                            }
                                        } else if k == "backgroundSummary" {
                                            let sum = v.as_str().unwrap_or("").to_owned();
                                            if session.background_badge.summary != sum {
                                                session.background_badge.summary = sum;
                                                badges_dirty = true;
                                            }
                                        }
                                        session.state.insert(k, v);
                                    }
                                    if is_active {
                                        need_render = true;
                                    }
                                }
                                WorkerEvent::Notify(msg) => toasts.push(msg),
                                WorkerEvent::Error(err) => {
                                    session.error = err;
                                    if is_active {
                                        need_render = true;
                                    }
                                }
                                WorkerEvent::CopyText(txt) => copies.push(txt),
                                WorkerEvent::OpenPath(path) => opens.push(path),
                                WorkerEvent::Exited(msg) => {
                                    if let Some(m) = msg {
                                        session.error = m;
                                    }
                                    if session.background_badge.active {
                                        session.background_badge.active = false;
                                        session.background_badge.summary.clear();
                                        badges_dirty = true;
                                    }
                                    if is_active {
                                        need_render = true;
                                    }
                                }
                            }
                        }
                        if is_active {
                            let latest = frame_rx.borrow().clone();
                            if let Some(frame) = latest {
                                if frame.sequence > *last_seq {
                                    *last_seq = frame.sequence;
                                    if let Some((img, rgba, jpeg)) =
                                        present_worker_frame(&frame, &session.state)
                                    {
                                        session.has_frame = true;
                                        session.latest_rgba = rgba;
                                        session.latest_jpeg = jpeg;
                                        session.last_frame_image = Some(img.clone());
                                        new_frame_for_active = Some(img);
                                    }
                                }
                            }
                        }
                    }
                    RuntimeKind::SshArena {
                        pose_packet,
                        camera_rx,
                        last_seq,
                        ..
                    } => {
                        if is_active {
                            if pose_packet.1.elapsed() >= Duration::from_secs(1) {
                                self.services.pose.touch();
                                if pose_live(&pose_packet.0, pose_packet.1.elapsed().as_secs_f64() * 1000.0) {
                                    apply_arena_pose_to_state(&mut session.state, &pose_packet.0);
                                }
                            }
                            if let Some(rx) = camera_rx {
                                if let Some(frame) = rx.borrow().clone() {
                                    if frame.sequence > *last_seq {
                                        *last_seq = frame.sequence;
                                        let buf = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
                                            &frame.pixels,
                                            frame.width,
                                            frame.height,
                                        );
                                        let img = slint::Image::from_rgb8(buf);
                                        session.has_frame = true;
                                        session.last_frame_image = Some(img.clone());
                                        new_frame_for_active = Some(img);
                                        let status = self.services.arena.status();
                                        if let Some(fps) = status["streamFps"].as_f64() {
                                            session.state.insert(
                                                "cameraStatus".into(),
                                                json!(format!("实时画面 · {:.1} FPS", fps)),
                                            );
                                            need_render = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    RuntimeKind::SshVision {
                        cancel,
                        inspecting,
                        last_inspect,
                        frame_rx,
                        status_rx,
                        last_seq,
                        ..
                    } => {
                        if is_active {
                            if !*inspecting
                                && last_inspect.elapsed() >= Duration::from_millis(2500)
                            {
                                *inspecting = true;
                                *last_inspect = Instant::now();
                                pending_vision_inspect =
                                    Some((session.id.clone(), session.generation, cancel.clone()));
                            }
                            if let Some(rx) = frame_rx {
                                if let Some(sample) = rx.borrow().clone() {
                                    if sample.rgb.sequence > *last_seq {
                                        *last_seq = sample.rgb.sequence;
                                        let buf = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
                                            &sample.rgb.pixels,
                                            sample.rgb.width,
                                            sample.rgb.height,
                                        );
                                        let img = slint::Image::from_rgb8(buf);
                                        session.has_frame = true;
                                        session.latest_jpeg = Some(sample.jpeg.clone());
                                        session.last_frame_image = Some(img.clone());
                                        new_frame_for_active = Some(img);
                                    }
                                }
                            }
                            if let Some(srx) = status_rx {
                                if srx.has_changed().unwrap_or(false) {
                                    let status = srx.borrow_and_update().clone();
                                    apply_vision_status_to_state(&mut session.state, &status);
                                    need_render = true;
                                }
                            }
                        }
                    }
                    RuntimeKind::SshGeneric { .. } => {}
                }
            }
            if badges_dirty {
                let mut map = self.background_badges.lock().unwrap();
                map.clear();
                for (id, s) in sessions.iter() {
                    if s.manifest.is_background() && s.background_badge.active {
                        map.insert(id.clone(), s.background_badge.clone());
                    }
                }
            }
        }

        if let Some((pid, gen, c)) = pending_vision_inspect {
            self.trigger_vision_inspect(&pid, gen, &c);
        }
        if badges_dirty {
            self.notify_badges_changed();
        }
        if let Some(ui) = self.ui.upgrade() {
            if let Some(img) = new_frame_for_active {
                ui.set_decl_video_frame(img);
                ui.set_decl_has_video_frame(true);
            }
            for msg in toasts {
                ui.invoke_notify(text(&msg, ui.get_english()).into());
            }
        }
        for txt in copies {
            self.on_copy_text(&txt);
        }
        for path in opens {
            if let Ok(valid) = validate_open_path(&path) {
                let _ = open_in_os_explorer(&valid);
            }
        }
        if need_render {
            self.render_active();
        }
    }

    fn on_state_change(&self, key: &str, value: Value) {
        let active_id = self.active_id.borrow().clone();
        if active_id.is_empty() {
            return;
        }
        let mut forward_worker: Option<(Arc<LocalWorkerHandle>, Vec<(String, Value)>)> = None;
        let mut push_vision_cfg: Option<(watch::Sender<Value>, Value)> = None;
        {
            let mut sessions = self.sessions.borrow_mut();
            let Some(session) = sessions.get_mut(&active_id) else {
                return;
            };
            let mut changes = vec![(key.to_owned(), value.clone())];
            session.state.insert(key.to_owned(), value.clone());

            // Apply declarative presets if configured in ui.json
            let val_key = match &value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => String::new(),
            };
            if let Some(preset_map) = session
                .spec
                .presets
                .get(key)
                .and_then(|by_val| by_val.get(&val_key))
                .cloned()
            {
                for (pk, pv) in preset_map {
                    session.state.insert(pk.clone(), pv.clone());
                    changes.push((pk, pv));
                }
            }

            match &session.runtime {
                RuntimeKind::LocalWorker { handle, .. } => {
                    forward_worker = Some((handle.clone(), changes));
                }
                RuntimeKind::SshVision {
                    config_tx: Some(tx),
                    cameras,
                    windows,
                    ..
                } => {
                    let cfg = build_vision_config(&session.state, cameras, windows);
                    push_vision_cfg = Some((tx.clone(), cfg));
                }
                _ => {}
            }
        }

        if let Some((handle, changes)) = forward_worker {
            self.spawn_job(async move {
                for (k, v) in changes {
                    let _ = handle.set_state(&k, v).await;
                }
            });
        }
        if let Some((tx, cfg)) = push_vision_cfg {
            tx.send_replace(cfg);
        }
        self.render_active();
    }

    fn on_action(&self, action: &str, item_id: &str, payload: &str) {
        let active_id = self.active_id.borrow().clone();
        if active_id.is_empty() {
            return;
        }
        if action == "snapshot" {
            self.save_snapshot(&active_id);
            return;
        }

        let mut worker_call: Option<(Arc<LocalWorkerHandle>, String, Option<String>, Option<Value>)> =
            None;
        let mut need_render = false;

        let parsed_payload = if payload.is_empty() {
            None
        } else {
            Some(serde_json::from_str::<Value>(payload).unwrap_or_else(|_| json!(payload)))
        };

        {
            let mut sessions = self.sessions.borrow_mut();
            let Some(session) = sessions.get_mut(&active_id) else {
                return;
            };
            match &mut session.runtime {
                RuntimeKind::LocalWorker { handle, .. } => {
                    // Built-in client-side transform actions work even before worker echoes back
                    if action == "rotate_cw" {
                        let cur = session
                            .state
                            .get("rotation")
                            .and_then(Value::as_i64)
                            .unwrap_or(0);
                        let next = (cur + 90) % 360;
                        session.state.insert("rotation".into(), json!(next));
                        need_render = true;
                    } else if action == "toggle_flip_h" {
                        let cur = session
                            .state
                            .get("flipH")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        session.state.insert("flipH".into(), json!(!cur));
                        need_render = true;
                    } else if action == "toggle_flip_v" {
                        let cur = session
                            .state
                            .get("flipV")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        session.state.insert("flipV".into(), json!(!cur));
                        need_render = true;
                    } else if action == "open_dir" {
                        if let Some(path) = parsed_payload.as_ref().and_then(Value::as_str) {
                            if let Ok(valid) = validate_open_path(path) {
                                let _ = open_in_os_explorer(&valid);
                            }
                        }
                    }
                    worker_call = Some((
                        handle.clone(),
                        action.to_owned(),
                        (!item_id.is_empty()).then(|| item_id.to_owned()),
                        parsed_payload,
                    ));
                }
                RuntimeKind::SshArena {
                    cancel,
                    cameras,
                    camera_epoch,
                    camera_cancel,
                    camera_rx,
                    last_seq,
                    ..
                } => match action {
                    "refresh_cameras" => {
                        session.state.insert("cameraLoading".into(), json!(true));
                        let gen = session.generation;
                        let c = cancel.clone();
                        let services = self.services.clone();
                        let weak = self.ui.clone();
                        let pid = active_id.clone();
                        self.spawn_job(async move {
                            let res = services
                                .arena
                                .cameras_for_generation(gen, &c)
                                .await
                                .unwrap_or_else(|e| json!({"error": e.to_string()}));
                            if !c.is_cancelled() {
                                let _ = weak.upgrade_in_event_loop(move |ui| {
                                    ui.invoke_decl_ssh_event(
                                        pid.into(),
                                        "arena-cameras".into(),
                                        res.to_string().into(),
                                    );
                                });
                            }
                        });
                        need_render = true;
                    }
                    "toggle_camera" => {
                        if let Some(old) = camera_cancel.take() {
                            old.cancel();
                            *camera_rx = None;
                            session.state.insert("cameraActive".into(), json!(false));
                            session.state.insert("cameraLoading".into(), json!(false));
                            session.state.insert("cameraStatus".into(), json!("摄像头已关闭"));
                            session.has_frame = false;
                            let services = self.services.clone();
                            self.spawn_job(async move {
                                services.arena.stop(None).await;
                            });
                        } else {
                            let idx = session
                                .state
                                .get("cameraIndex")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as usize;
                            if let Some(path) = cameras.get(idx).cloned() {
                                let rate_idx = session
                                    .state
                                    .get("rate")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(2) as usize;
                                let fps = [8u32, 15, 20, 30].get(rate_idx).copied().unwrap_or(20);
                                *camera_epoch += 1;
                                *last_seq = 0;
                                let run_cancel = cancel.child_token();
                                let (tx, rx) = watch::channel(None);
                                *camera_cancel = Some(run_cancel.clone());
                                *camera_rx = Some(rx);
                                session.state.insert("cameraActive".into(), json!(true));
                                session.state.insert("cameraLoading".into(), json!(true));
                                session.state.insert("cameraStatus".into(), json!("正在连接摄像头…"));
                                let services = self.services.clone();
                                let gen = session.generation;
                                let weak = self.ui.clone();
                                let pid = active_id.clone();
                                self.spawn_job(async move {
                                    let res = camera_client::run(
                                        services,
                                        gen,
                                        path,
                                        fps,
                                        tx,
                                        run_cancel.clone(),
                                    )
                                    .await;
                                    if !run_cancel.is_cancelled() {
                                        let err = res
                                            .err()
                                            .map(|e| e.to_string())
                                            .unwrap_or_default();
                                        let _ = weak.upgrade_in_event_loop(move |ui| {
                                            ui.invoke_decl_ssh_event(
                                                pid.into(),
                                                "arena-camera-finished".into(),
                                                json!({"error": err}).to_string().into(),
                                            );
                                        });
                                    }
                                });
                            }
                        }
                        need_render = true;
                    }
                    _ => {}
                },
                RuntimeKind::SshVision {
                    cancel,
                    inspecting,
                    cameras,
                    windows,
                    run_id,
                    stream_cancel,
                    config_tx,
                    frame_rx,
                    status_rx,
                    last_seq,
                    ..
                } => match action {
                    "refresh_board" => {
                        *inspecting = true;
                        self.trigger_vision_inspect(&active_id, session.generation, cancel);
                        need_render = true;
                    }
                    "toggle_stream" => {
                        if let Some(old) = stream_cancel.take() {
                            old.cancel();
                            *config_tx = None;
                            *frame_rx = None;
                            *status_rx = None;
                            session.state.insert("active".into(), json!(false));
                            session.state.insert("loading".into(), json!(false));
                            session.state.insert("status".into(), json!("已停止"));
                            session.state.insert("statusPhase".into(), json!(0));
                            session.has_frame = false;
                            let services = self.services.clone();
                            self.spawn_job(async move {
                                services.vision.stop(None).await;
                            });
                        } else {
                            let cfg = build_vision_config(&session.state, cameras, windows);
                            *run_id += 1;
                            *last_seq = 0;
                            let run_cancel = cancel.child_token();
                            let (cfg_tx, cfg_rx) = watch::channel(cfg);
                            let (f_tx, f_rx) = watch::channel(None);
                            let (s_tx, s_rx) = watch::channel(Value::Null);
                            *stream_cancel = Some(run_cancel.clone());
                            *config_tx = Some(cfg_tx);
                            *frame_rx = Some(f_rx);
                            *status_rx = Some(s_rx);
                            session.error.clear();
                            session.state.insert("active".into(), json!(true));
                            session.state.insert("loading".into(), json!(true));
                            session.state.insert("status".into(), json!("正在连接…"));
                            session.state.insert("statusPhase".into(), json!(1));
                            let services = self.services.clone();
                            let gen = session.generation;
                            let weak = self.ui.clone();
                            let pid = active_id.clone();
                            self.spawn_job(async move {
                                let res = run_vision_stream(
                                    services,
                                    gen,
                                    run_cancel.clone(),
                                    cfg_rx,
                                    f_tx,
                                    s_tx,
                                )
                                .await;
                                if !run_cancel.is_cancelled() {
                                    let err = res
                                        .err()
                                        .map(|e| e.to_string())
                                        .unwrap_or_default();
                                    let _ = weak.upgrade_in_event_loop(move |ui| {
                                        ui.invoke_decl_ssh_event(
                                            pid.into(),
                                            "vision-stream-finished".into(),
                                            json!({"error": err}).to_string().into(),
                                        );
                                    });
                                }
                            });
                        }
                        need_render = true;
                    }
                    _ => {}
                },
                RuntimeKind::SshGeneric { .. } => {}
            }
        }

        if let Some((handle, act, item, payload)) = worker_call {
            self.spawn_job(async move {
                let _ = handle.send_action(&act, item.as_deref(), payload).await;
            });
        }
        if need_render {
            self.render_active();
        }
    }

    fn on_pick_directory(&self, state_key: &str, action: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let chosen = ui
            .window()
            .with_winit_window(|window| {
                rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(text("选择保存目录", ui.get_english()))
                    .pick_folder()
            })
            .flatten();
        ui.invoke_focus_main();
        let Some(dir) = chosen else { return };
        let path_str = dir.to_string_lossy().into_owned();
        self.on_state_change(state_key, json!(path_str));
        if !action.is_empty() {
            self.on_action(action, "", &path_str);
        }
    }

    fn on_copy_text(&self, content: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if cb.set_text(content.to_owned()).is_ok() {
                ui.invoke_notify(text("已复制到剪贴板", ui.get_english()).into());
            }
        }
    }

    fn save_snapshot(&self, plugin_id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let (jpeg, rgba) = {
            let sessions = self.sessions.borrow();
            let Some(s) = sessions.get(plugin_id) else {
                return;
            };
            (s.latest_jpeg.clone(), s.latest_rgba.clone())
        };
        if jpeg.is_none() && rgba.is_none() {
            return;
        }
        let default_name = format!(
            "{}-{}.png",
            plugin_id,
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let target = ui
            .window()
            .with_winit_window(|window| {
                rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(text("保存画面快照", ui.get_english()))
                    .set_file_name(&default_name)
                    .add_filter("PNG", &["png"])
                    .save_file()
            })
            .flatten();
        ui.invoke_focus_main();
        let Some(path) = target else { return };
        let result = (|| -> Result<()> {
            if let Some((w, h, bytes)) = rgba {
                let img = image::RgbaImage::from_raw(w, h, bytes)
                    .ok_or_else(|| anyhow!("图像缓冲尺寸不匹配。"))?;
                img.save(&path)?;
            } else if let Some(jpeg) = jpeg {
                let decoded = image::load_from_memory(&jpeg)?;
                decoded.save(&path)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => ui.invoke_notify(text("快照已保存", ui.get_english()).into()),
            Err(e) => ui.set_decl_error(e.to_string().into()),
        }
    }

    fn clear_ui(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_decl_video_fullscreen() {
            ui.invoke_decl_toggle_fullscreen(false);
        }
        ui.set_decl_error("".into());
        ui.set_decl_has_video_frame(false);
        ui.set_decl_has_arena_platform_image(false);
        self.primary_model.set_vec(Vec::new());
        self.secondary_model.set_vec(Vec::new());
        ui.set_decl_header_status("".into());
        ui.set_decl_header_phase(0);
    }

    fn render_active(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let active_id = self.active_id.borrow().clone();
        let english = ui.get_english();
        let sessions = self.sessions.borrow();
        let Some(session) = sessions.get(&active_id) else {
            return;
        };
        ui.set_decl_requires_device(session.manifest.requires_device());
        ui.set_decl_layout_stack(session.spec.layout == "stack");
        ui.set_decl_primary_ratio(session.spec.primary_width_ratio);
        ui.set_decl_error(text(&session.error, english).into());
        ui.set_decl_has_video_frame(session.has_frame);
        if let Some(img) = &session.last_frame_image {
            ui.set_decl_video_frame(img.clone());
        }
        if let RuntimeKind::SshArena {
            platform_image: Some(img),
            ..
        } = &session.runtime
        {
            ui.set_decl_arena_platform_image(img.clone());
            ui.set_decl_has_arena_platform_image(true);
        } else {
            ui.set_decl_has_arena_platform_image(false);
        }

        if let Some(hb) = &session.spec.header_badge {
            let status_txt = session
                .state
                .get(&hb.status_key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|s| text(s, english))
                .unwrap_or_else(|| {
                    loc(&hb.default_text, &hb.default_text_en, english).to_owned()
                });
            let phase = hb
                .phase_key
                .as_ref()
                .and_then(|k| session.state.get(k))
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32;
            ui.set_decl_header_status(status_txt.into());
            ui.set_decl_header_phase(phase);
        } else {
            ui.set_decl_header_status("".into());
            ui.set_decl_header_phase(0);
        }

        let primary: Vec<DeclBlock> = session
            .spec
            .primary
            .iter()
            .filter_map(|b| build_block(b, &session.state, english))
            .collect();
        let secondary: Vec<DeclBlock> = session
            .spec
            .secondary
            .iter()
            .filter_map(|b| build_block(b, &session.state, english))
            .collect();
        sync_block_model(&self.primary_model, primary);
        sync_block_model(&self.secondary_model, secondary);
        self.epoch.set(self.epoch.get().wrapping_add(1));
    }
}

fn sync_string_model(dst_rc: &ModelRc<SharedString>, src_rc: &ModelRc<SharedString>) -> bool {
    let Some(dst) = dst_rc.as_any().downcast_ref::<VecModel<SharedString>>() else {
        return false;
    };
    let next: Vec<SharedString> = src_rc.iter().collect();
    if dst.row_count() == next.len()
        && next
            .iter()
            .enumerate()
            .all(|(i, v)| dst.row_data(i).as_ref() == Some(v))
    {
        return true;
    }
    if dst.row_count() == next.len() {
        for (i, v) in next.into_iter().enumerate() {
            if dst.row_data(i).as_ref() != Some(&v) {
                dst.set_row_data(i, v);
            }
        }
    } else {
        dst.set_vec(next);
    }
    true
}

fn sync_button_model(dst_rc: &ModelRc<DeclButton>, src_rc: &ModelRc<DeclButton>) -> bool {
    let Some(dst) = dst_rc.as_any().downcast_ref::<VecModel<DeclButton>>() else {
        return false;
    };
    let next: Vec<DeclButton> = src_rc.iter().collect();
    if dst.row_count() == next.len() {
        for (i, v) in next.into_iter().enumerate() {
            if dst.row_data(i).as_ref() != Some(&v) {
                dst.set_row_data(i, v);
            }
        }
    } else {
        dst.set_vec(next);
    }
    true
}

fn control_scalars_eq(a: &DeclControl, b: &DeclControl) -> bool {
    a.id == b.id
        && a.kind == b.kind
        && a.label == b.label
        && a.state_key == b.state_key
        && a.action == b.action
        && a.secondary_action == b.secondary_action
        && a.secondary_label == b.secondary_label
        && a.secondary_danger == b.secondary_danger
        && a.int_value == b.int_value
        && a.float_value.to_bits() == b.float_value.to_bits()
        && a.min_value.to_bits() == b.min_value.to_bits()
        && a.max_value.to_bits() == b.max_value.to_bits()
        && a.step_value.to_bits() == b.step_value.to_bits()
        && a.unit == b.unit
        && a.bool_value == b.bool_value
        && a.text_value == b.text_value
        && a.placeholder == b.placeholder
        && a.icon_kind == b.icon_kind
        && a.primary == b.primary
        && a.danger == b.danger
        && a.busy == b.busy
        && a.enabled == b.enabled
        && a.tone == b.tone
}

fn sync_control_model(dst_rc: &ModelRc<DeclControl>, src_rc: &ModelRc<DeclControl>) -> bool {
    let Some(dst) = dst_rc.as_any().downcast_ref::<VecModel<DeclControl>>() else {
        return false;
    };
    let next: Vec<DeclControl> = src_rc.iter().collect();
    let same_structure = dst.row_count() == next.len()
        && next.iter().enumerate().all(|(i, c)| {
            dst.row_data(i)
                .is_some_and(|old| old.id == c.id && old.kind == c.kind)
        });
    if !same_structure {
        dst.set_vec(next);
        return true;
    }
    for (i, mut nc) in next.into_iter().enumerate() {
        let Some(old) = dst.row_data(i) else { continue };
        let reused_opts = sync_string_model(&old.options, &nc.options);
        if reused_opts {
            nc.options = old.options.clone();
        }
        let reused_btns = sync_button_model(&old.buttons, &nc.buttons);
        if reused_btns {
            nc.buttons = old.buttons.clone();
        }
        if !reused_opts || !reused_btns || !control_scalars_eq(&old, &nc) {
            dst.set_row_data(i, nc);
        }
    }
    true
}

fn sync_kpi_model(dst_rc: &ModelRc<DeclKpi>, src_rc: &ModelRc<DeclKpi>) -> bool {
    let Some(dst) = dst_rc.as_any().downcast_ref::<VecModel<DeclKpi>>() else {
        return false;
    };
    let next: Vec<DeclKpi> = src_rc.iter().collect();
    if dst.row_count() == next.len() {
        for (i, v) in next.into_iter().enumerate() {
            if dst.row_data(i).as_ref() != Some(&v) {
                dst.set_row_data(i, v);
            }
        }
    } else {
        dst.set_vec(next);
    }
    true
}

fn sync_task_action_model(
    dst_rc: &ModelRc<DeclTaskAction>,
    src_rc: &ModelRc<DeclTaskAction>,
) -> bool {
    let Some(dst) = dst_rc.as_any().downcast_ref::<VecModel<DeclTaskAction>>() else {
        return false;
    };
    let next: Vec<DeclTaskAction> = src_rc.iter().collect();
    if dst.row_count() == next.len() {
        for (i, v) in next.into_iter().enumerate() {
            if dst.row_data(i).as_ref() != Some(&v) {
                dst.set_row_data(i, v);
            }
        }
    } else {
        dst.set_vec(next);
    }
    true
}

fn task_scalars_eq(a: &DeclTask, b: &DeclTask) -> bool {
    a.id == b.id
        && a.title == b.title
        && a.subtitle == b.subtitle
        && a.badge == b.badge
        && a.badge_tone == b.badge_tone
        && a.progress.to_bits() == b.progress.to_bits()
        && a.show_progress == b.show_progress
}

fn sync_task_model(dst_rc: &ModelRc<DeclTask>, src_rc: &ModelRc<DeclTask>) -> bool {
    let Some(dst) = dst_rc.as_any().downcast_ref::<VecModel<DeclTask>>() else {
        return false;
    };
    let next: Vec<DeclTask> = src_rc.iter().collect();
    let same_ids = dst.row_count() == next.len()
        && next
            .iter()
            .enumerate()
            .all(|(i, t)| dst.row_data(i).is_some_and(|old| old.id == t.id));
    if !same_ids {
        dst.set_vec(next);
        return true;
    }
    for (i, mut nt) in next.into_iter().enumerate() {
        let Some(old) = dst.row_data(i) else { continue };
        let reused = sync_task_action_model(&old.actions, &nt.actions);
        if reused {
            nt.actions = old.actions.clone();
        }
        if !reused || !task_scalars_eq(&old, &nt) {
            dst.set_row_data(i, nt);
        }
    }
    true
}

fn block_scalars_eq(a: &DeclBlock, b: &DeclBlock) -> bool {
    a.id == b.id
        && a.kind == b.kind
        && a.title == b.title
        && a.badge == b.badge
        && a.badge_phase == b.badge_phase
        && a.summary == b.summary
        && a.empty_text == b.empty_text
        && a.columns == b.columns
        && a.active == b.active
        && a.loading == b.loading
        && a.fit_mode == b.fit_mode
        && a.capabilities == b.capabilities
        && a.show_snapshot == b.show_snapshot
        && a.snapshot_action == b.snapshot_action
        && a.show_fullscreen == b.show_fullscreen
        && a.footer_status == b.footer_status
        && a.width_meters.to_bits() == b.width_meters.to_bits()
        && a.height_meters.to_bits() == b.height_meters.to_bits()
        && a.live == b.live
        && a.world_x.to_bits() == b.world_x.to_bits()
        && a.world_y.to_bits() == b.world_y.to_bits()
        && a.heading.to_bits() == b.heading.to_bits()
        && a.has_heading == b.has_heading
        && a.coords == b.coords
}

fn sync_block_model(dst: &VecModel<DeclBlock>, next: Vec<DeclBlock>) {
    let same_structure = dst.row_count() == next.len()
        && next.iter().enumerate().all(|(i, b)| {
            dst.row_data(i)
                .is_some_and(|old| old.id == b.id && old.kind == b.kind)
        });
    if !same_structure {
        dst.set_vec(next);
        return;
    }
    for (i, mut nb) in next.into_iter().enumerate() {
        let Some(old) = dst.row_data(i) else { continue };
        let r_ctrl = sync_control_model(&old.controls, &nb.controls);
        if r_ctrl {
            nb.controls = old.controls.clone();
        }
        let r_act = sync_control_model(&old.action_controls, &nb.action_controls);
        if r_act {
            nb.action_controls = old.action_controls.clone();
        }
        let r_kpi = sync_kpi_model(&old.kpis, &nb.kpis);
        if r_kpi {
            nb.kpis = old.kpis.clone();
        }
        let r_task = sync_task_model(&old.tasks, &nb.tasks);
        if r_task {
            nb.tasks = old.tasks.clone();
        }
        if !r_ctrl || !r_act || !r_kpi || !r_task || !block_scalars_eq(&old, &nb) {
            dst.set_row_data(i, nb);
        }
    }
}

fn loc<'a>(zh: &'a str, en: &'a str, english: bool) -> &'a str {
    if english && !en.is_empty() {
        en
    } else {
        zh
    }
}

fn cond_ok(cond: &Option<Condition>, state: &Map<String, Value>) -> bool {
    cond.as_ref().is_none_or(|c| c.matches(state))
}

fn icon_code(name: &str) -> i32 {
    match name {
        "play" => 1,
        "restart" | "refresh" => 2,
        "copy" => 3,
        "download" => 4,
        "folder" | "folder-add" => 5,
        "camera" => 6,
        "plus" => 7,
        "trash" => 8,
        "check" => 9,
        "fit" => 10,
        _ => 0,
    }
}

fn option_strings(
    static_opts: &[OptionSpec],
    options_key: Option<&str>,
    state: &Map<String, Value>,
    english: bool,
) -> Vec<SharedString> {
    if let Some(key) = options_key {
        if let Some(arr) = state.get(key).and_then(Value::as_array) {
            return arr
                .iter()
                .map(|item| {
                    let s = item["label"]
                        .as_str()
                        .or_else(|| item.as_str())
                        .unwrap_or("");
                    SharedString::from(s)
                })
                .collect();
        }
    }
    static_opts
        .iter()
        .map(|o| SharedString::from(loc(&o.label, &o.label_en, english)))
        .collect()
}

fn build_control(ctrl: &ControlSpec, state: &Map<String, Value>, english: bool) -> Option<DeclControl> {
    let base = DeclControl {
        id: "".into(),
        kind: "".into(),
        label: "".into(),
        state_key: "".into(),
        action: "".into(),
        secondary_action: "".into(),
        secondary_label: "".into(),
        secondary_danger: false,
        options: ModelRc::default(),
        int_value: 0,
        float_value: 0.0,
        min_value: 0.0,
        max_value: 100.0,
        step_value: 1.0,
        unit: "".into(),
        bool_value: false,
        text_value: "".into(),
        placeholder: "".into(),
        icon_kind: 0,
        primary: false,
        danger: false,
        busy: false,
        enabled: true,
        tone: "muted".into(),
        buttons: ModelRc::default(),
    };
    match ctrl {
        ControlSpec::Segmented {
            id,
            label,
            label_en,
            state_key,
            options,
            disabled_when,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let opts = option_strings(options, None, state, english);
            let cur = state.get(state_key).and_then(Value::as_i64).unwrap_or(0) as i32;
            Some(DeclControl {
                id: id.into(),
                kind: "segmented".into(),
                label: loc(label, label_en, english).into(),
                state_key: state_key.into(),
                options: ModelRc::new(VecModel::from(opts)),
                int_value: cur,
                enabled: disabled_when.as_ref().is_none_or(|c| !c.matches(state)),
                ..base
            })
        }
        ControlSpec::Select {
            id,
            label,
            label_en,
            state_key,
            options,
            options_key,
            disabled_when,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let opts = option_strings(options, options_key.as_deref(), state, english);
            let cur = state.get(state_key).and_then(Value::as_i64).unwrap_or(0) as i32;
            Some(DeclControl {
                id: id.into(),
                kind: "select".into(),
                label: loc(label, label_en, english).into(),
                state_key: state_key.into(),
                options: ModelRc::new(VecModel::from(opts)),
                int_value: cur,
                enabled: disabled_when.as_ref().is_none_or(|c| !c.matches(state)),
                ..base
            })
        }
        ControlSpec::CheckBox {
            id,
            label,
            label_en,
            state_key,
            disabled_when,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let checked = state
                .get(state_key)
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(DeclControl {
                id: id.into(),
                kind: "checkbox".into(),
                label: loc(label, label_en, english).into(),
                state_key: state_key.into(),
                bool_value: checked,
                enabled: disabled_when.as_ref().is_none_or(|c| !c.matches(state)),
                ..base
            })
        }
        ControlSpec::Slider {
            id,
            label,
            label_en,
            state_key,
            min,
            max,
            step,
            unit,
            disabled_when,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let val = state
                .get(state_key)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .unwrap_or(*min);
            Some(DeclControl {
                id: id.into(),
                kind: "slider".into(),
                label: loc(label, label_en, english).into(),
                state_key: state_key.into(),
                float_value: val,
                min_value: *min,
                max_value: *max,
                step_value: *step,
                unit: unit.into(),
                enabled: disabled_when.as_ref().is_none_or(|c| !c.matches(state)),
                ..base
            })
        }
        ControlSpec::Input {
            id,
            label,
            label_en,
            placeholder,
            placeholder_en,
            state_key,
            submit_action,
            disabled_when,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let val = state
                .get(state_key)
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(DeclControl {
                id: id.into(),
                kind: "input".into(),
                label: loc(label, label_en, english).into(),
                state_key: state_key.into(),
                action: submit_action.as_deref().unwrap_or("").into(),
                text_value: val.into(),
                placeholder: loc(placeholder, placeholder_en, english).into(),
                enabled: disabled_when.as_ref().is_none_or(|c| !c.matches(state)),
                ..base
            })
        }
        ControlSpec::DirPicker {
            id,
            label,
            label_en,
            state_key,
            pick_action,
            open_action,
            disabled_when,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let val = state
                .get(state_key)
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(DeclControl {
                id: id.into(),
                kind: "dir-picker".into(),
                label: loc(label, label_en, english).into(),
                state_key: state_key.into(),
                action: pick_action.into(),
                secondary_action: open_action.as_deref().unwrap_or("").into(),
                text_value: val.into(),
                enabled: disabled_when.as_ref().is_none_or(|c| !c.matches(state)),
                ..base
            })
        }
        ControlSpec::Button {
            id,
            label,
            label_en,
            active_label,
            active_label_en,
            active_key,
            action,
            icon,
            primary,
            danger,
            busy_key,
            disabled_when,
            visible_when,
            ..
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let active = active_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let busy = busy_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let caption = if active && !active_label.is_empty() {
                loc(active_label, active_label_en, english)
            } else {
                loc(label, label_en, english)
            };
            Some(DeclControl {
                id: id.into(),
                kind: "button".into(),
                label: caption.into(),
                action: action.into(),
                icon_kind: icon_code(icon),
                primary: *primary && !active,
                danger: *danger,
                busy,
                enabled: disabled_when.as_ref().is_none_or(|c| !c.matches(state)),
                ..base
            })
        }
        ControlSpec::ButtonRow {
            id,
            buttons,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let mut items = Vec::new();
            for b in buttons {
                if let ControlSpec::Button {
                    id: bid,
                    label,
                    label_en,
                    active_label,
                    active_label_en,
                    active_key,
                    action,
                    payload,
                    icon,
                    primary,
                    danger,
                    busy_key,
                    disabled_when,
                    visible_when,
                } = b
                {
                    if !cond_ok(visible_when, state) {
                        continue;
                    }
                    let active = active_key
                        .as_ref()
                        .and_then(|k| state.get(k))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let busy = busy_key
                        .as_ref()
                        .and_then(|k| state.get(k))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let caption = if active && !active_label.is_empty() {
                        loc(active_label, active_label_en, english)
                    } else {
                        loc(label, label_en, english)
                    };
                    let payload_str = payload
                        .as_ref()
                        .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
                        .unwrap_or_default();
                    items.push(DeclButton {
                        id: bid.into(),
                        label: caption.into(),
                        action: action.into(),
                        payload: payload_str.into(),
                        icon_kind: icon_code(icon),
                        primary: *primary && !active,
                        selected: active,
                        danger: *danger,
                        busy,
                        enabled: disabled_when.as_ref().is_none_or(|c| !c.matches(state)),
                    });
                }
            }
            Some(DeclControl {
                id: id.into(),
                kind: "button-row".into(),
                buttons: ModelRc::new(VecModel::from(items)),
                ..base
            })
        }
        ControlSpec::CodeSnippet {
            id,
            label,
            label_en,
            text: snippet,
            copy_label,
            copy_label_en,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            Some(DeclControl {
                id: id.into(),
                kind: "code-snippet".into(),
                label: loc(label, label_en, english).into(),
                text_value: snippet.into(),
                secondary_label: loc(copy_label, copy_label_en, english).into(),
                ..base
            })
        }
        ControlSpec::Notice {
            id,
            text: static_text,
            text_en,
            text_key,
            tone,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let content = text_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_str)
                .map(|s| text(s, english))
                .unwrap_or_else(|| loc(static_text, text_en, english).to_owned());
            if content.is_empty() {
                return None;
            }
            Some(DeclControl {
                id: id.into(),
                kind: "notice".into(),
                text_value: content.into(),
                tone: tone.into(),
                ..base
            })
        }
    }
}

fn build_block(block: &BlockSpec, state: &Map<String, Value>, english: bool) -> Option<DeclBlock> {
    let base = DeclBlock {
        id: "".into(),
        kind: "".into(),
        title: "".into(),
        badge: "".into(),
        badge_phase: 0,
        summary: "".into(),
        empty_text: "".into(),
        columns: 2,
        controls: ModelRc::new(VecModel::<DeclControl>::default()),
        action_controls: ModelRc::new(VecModel::<DeclControl>::default()),
        kpis: ModelRc::new(VecModel::<DeclKpi>::default()),
        tasks: ModelRc::new(VecModel::<DeclTask>::default()),
        active: false,
        loading: false,
        fit_mode: 0,
        capabilities: "".into(),
        show_snapshot: false,
        snapshot_action: "".into(),
        show_fullscreen: false,
        footer_status: "".into(),
        width_meters: 3.8,
        height_meters: 3.8,
        live: false,
        world_x: 0.0,
        world_y: 0.0,
        heading: 0.0,
        has_heading: false,
        coords: "—".into(),
    };
    match block {
        BlockSpec::Toolbar {
            id,
            items,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let controls: Vec<DeclControl> = items
                .iter()
                .filter_map(|c| build_control(c, state, english))
                .collect();
            Some(DeclBlock {
                id: id.into(),
                kind: "toolbar".into(),
                controls: ModelRc::new(VecModel::from(controls)),
                ..base
            })
        }
        BlockSpec::VideoStage {
            id,
            title,
            title_en,
            status_key,
            badge_key,
            capabilities_key,
            active_key,
            loading_key,
            fit_mode_key,
            show_snapshot,
            snapshot_action,
            allow_fullscreen,
            empty_text,
            empty_text_en,
            footer_status_key,
            controls,
            visible_when,
            ..
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let all_ctrls: Vec<DeclControl> = controls
                .iter()
                .filter_map(|c| build_control(c, state, english))
                .collect();
            let has_select = all_ctrls.iter().any(|c| c.kind == "select" || c.kind == "input");
            let has_button = all_ctrls
                .iter()
                .any(|c| c.kind == "button" || c.kind == "button-row");
            let (primary_ctrls, action_ctrls) = if has_select && has_button {
                let mut sels = Vec::new();
                let mut acts = Vec::new();
                for c in all_ctrls {
                    if c.kind == "button" || c.kind == "button-row" {
                        acts.push(c);
                    } else {
                        sels.push(c);
                    }
                }
                (sels, acts)
            } else {
                (all_ctrls, Vec::new())
            };
            let active = active_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let loading = loading_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let fit_mode = fit_mode_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32;
            let badge = badge_key
                .as_ref()
                .or(status_key.as_ref())
                .and_then(|k| state.get(k))
                .and_then(Value::as_str)
                .map(|s| text(s, english))
                .unwrap_or_default();
            let caps = capabilities_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_str)
                .unwrap_or("");
            let footer = footer_status_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_str)
                .map(|s| text(s, english))
                .unwrap_or_default();
            Some(DeclBlock {
                id: id.into(),
                kind: "video-stage".into(),
                title: loc(title, title_en, english).into(),
                badge: badge.into(),
                badge_phase: if active { 2 } else { 0 },
                controls: ModelRc::new(VecModel::from(primary_ctrls)),
                action_controls: ModelRc::new(VecModel::from(action_ctrls)),
                active,
                loading,
                fit_mode,
                capabilities: caps.into(),
                show_snapshot: *show_snapshot,
                snapshot_action: snapshot_action.as_deref().unwrap_or("snapshot").into(),
                show_fullscreen: *allow_fullscreen,
                empty_text: loc(empty_text, empty_text_en, english).into(),
                footer_status: footer.into(),
                ..base
            })
        }
        BlockSpec::ArenaMap {
            id,
            title,
            title_en,
            width_meters,
            height_meters,
            live_key,
            x_key,
            y_key,
            heading_key,
            has_heading_key,
            coords_key,
            visible_when,
            ..
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            Some(DeclBlock {
                id: id.into(),
                kind: "arena-map".into(),
                title: loc(title, title_en, english).into(),
                width_meters: *width_meters,
                height_meters: *height_meters,
                live: state.get(live_key).and_then(Value::as_bool).unwrap_or(false),
                world_x: state.get(x_key).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                world_y: state.get(y_key).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                heading: state
                    .get(heading_key)
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32,
                has_heading: state
                    .get(has_heading_key)
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                coords: state
                    .get(coords_key)
                    .and_then(Value::as_str)
                    .unwrap_or("—")
                    .into(),
                ..base
            })
        }
        BlockSpec::KpiGrid {
            id,
            title,
            title_en,
            badge_key,
            badge_phase_key,
            columns,
            items,
            dynamic_items_key,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let mut kpis = Vec::new();
            for item in items {
                let raw = state.get(&item.value_key).map(|v| match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => String::new(),
                });
                let val = raw
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| item.default_value.clone());
                kpis.push(DeclKpi {
                    label: loc(&item.label, &item.label_en, english).into(),
                    value: val.into(),
                    full_width: item.full_width,
                });
            }
            if let Some(dyn_key) = dynamic_items_key {
                if let Some(arr) = state.get(dyn_key).and_then(Value::as_array) {
                    for entry in arr {
                        let lbl = entry["label"].as_str().unwrap_or("");
                        let val = entry["value"].as_str().unwrap_or("—");
                        kpis.push(DeclKpi {
                            label: text(lbl, english).into(),
                            value: val.into(),
                            full_width: false,
                        });
                    }
                }
            }
            let badge = badge_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_str)
                .map(|s| text(s, english))
                .unwrap_or_default();
            let phase = badge_phase_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32;
            Some(DeclBlock {
                id: id.into(),
                kind: "kpi-grid".into(),
                title: loc(title, title_en, english).into(),
                badge: badge.into(),
                badge_phase: phase,
                columns: (*columns) as i32,
                kpis: ModelRc::new(VecModel::from(kpis)),
                ..base
            })
        }
        BlockSpec::ControlCard {
            id,
            title,
            title_en,
            badge_key,
            controls,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let ctrls: Vec<DeclControl> = controls
                .iter()
                .filter_map(|c| build_control(c, state, english))
                .collect();
            let badge = badge_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_str)
                .map(|s| text(s, english))
                .unwrap_or_default();
            Some(DeclBlock {
                id: id.into(),
                kind: "control-card".into(),
                title: loc(title, title_en, english).into(),
                badge: badge.into(),
                controls: ModelRc::new(VecModel::from(ctrls)),
                ..base
            })
        }
        BlockSpec::TaskList {
            id,
            title,
            title_en,
            summary_key,
            items_key,
            empty_text,
            empty_text_en,
            visible_when,
        } => {
            if !cond_ok(visible_when, state) {
                return None;
            }
            let mut tasks = Vec::new();
            if let Some(arr) = state.get(items_key).and_then(Value::as_array) {
                for item in arr {
                    let actions: Vec<DeclTaskAction> = item["actions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|a| {
                            let lbl = loc(
                                a["label"].as_str().unwrap_or(""),
                                a["labelEn"].as_str().unwrap_or(""),
                                english,
                            );
                            DeclTaskAction {
                                id: a["id"].as_str().unwrap_or("").into(),
                                label: lbl.into(),
                                primary: a["primary"].as_bool().unwrap_or(false),
                                danger: a["danger"].as_bool().unwrap_or(false),
                                enabled: a["enabled"].as_bool().unwrap_or(true),
                            }
                        })
                        .collect();
                    let tone = match item["badgeTone"].as_str().unwrap_or("muted") {
                        "accent" => 1,
                        "ok" => 2,
                        "danger" => 3,
                        _ => 0,
                    };
                    tasks.push(DeclTask {
                        id: item["id"].as_str().unwrap_or("").into(),
                        title: item["title"].as_str().unwrap_or("").into(),
                        subtitle: item["subtitle"].as_str().unwrap_or("").into(),
                        badge: text(item["badge"].as_str().unwrap_or(""), english).into(),
                        badge_tone: tone,
                        progress: item["progress"].as_f64().unwrap_or(0.0) as f32,
                        show_progress: item["showProgress"].as_bool().unwrap_or(false),
                        actions: ModelRc::new(VecModel::from(actions)),
                    });
                }
            }
            let summary = summary_key
                .as_ref()
                .and_then(|k| state.get(k))
                .and_then(Value::as_str)
                .map(|s| text(s, english))
                .unwrap_or_default();
            Some(DeclBlock {
                id: id.into(),
                kind: "task-list".into(),
                title: loc(title, title_en, english).into(),
                summary: summary.into(),
                empty_text: loc(empty_text, empty_text_en, english).into(),
                tasks: ModelRc::new(VecModel::from(tasks)),
                ..base
            })
        }
    }
}

fn present_worker_frame(
    frame: &WorkerVideoFrame,
    state: &Map<String, Value>,
) -> Option<(slint::Image, Option<(u32, u32, Vec<u8>)>, Option<Arc<Vec<u8>>>)> {
    let rotation = state.get("rotation").and_then(Value::as_i64).unwrap_or(0).rem_euclid(360);
    let flip_h = state.get("flipH").and_then(Value::as_bool).unwrap_or(false);
    let flip_v = state.get("flipV").and_then(Value::as_bool).unwrap_or(false);

    let (w, h, mut rgba, jpeg) = match frame.format {
        FrameFormat::Rgba8 => (frame.width, frame.height, frame.data.as_ref().clone(), None),
        FrameFormat::Jpeg => {
            let decoded = image::load_from_memory(&frame.data).ok()?.to_rgba8();
            let (w, h) = decoded.dimensions();
            (w, h, decoded.into_raw(), Some(frame.data.clone()))
        }
    };

    let (out_w, out_h, final_rgba) = if rotation != 0 || flip_h || flip_v {
        transform_rgba(w, h, &rgba, rotation, flip_h, flip_v)
    } else {
        (w, h, std::mem::take(&mut rgba))
    };

    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&final_rgba, out_w, out_h);
    let img = slint::Image::from_rgba8(buf);
    Some((img, Some((out_w, out_h, final_rgba)), jpeg))
}

fn transform_rgba(
    w: u32,
    h: u32,
    src: &[u8],
    rotation: i64,
    flip_h: bool,
    flip_v: bool,
) -> (u32, u32, Vec<u8>) {
    let (dst_w, dst_h) = if rotation == 90 || rotation == 270 {
        (h, w)
    } else {
        (w, h)
    };
    let mut dst = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    let w_usize = w as usize;
    let dst_w_usize = dst_w as usize;
    for y in 0..h {
        for x in 0..w {
            let sx = if flip_h { w - 1 - x } else { x };
            let sy = if flip_v { h - 1 - y } else { y };
            let (dx, dy) = match rotation {
                90 => (h - 1 - sy, sx),
                180 => (w - 1 - sx, h - 1 - sy),
                270 => (sy, w - 1 - sx),
                _ => (sx, sy),
            };
            let s_idx = ((y as usize) * w_usize + (x as usize)) * 4;
            let d_idx = ((dy as usize) * dst_w_usize + (dx as usize)) * 4;
            dst[d_idx..d_idx + 4].copy_from_slice(&src[s_idx..s_idx + 4]);
        }
    }
    (dst_w, dst_h, dst)
}

fn apply_arena_pose_to_state(state: &mut Map<String, Value>, v: &Value) {
    let phase = v["phase"].as_str().unwrap_or("unavailable");
    let live = phase == "live";
    state.insert("poseLive".into(), json!(live));
    state.insert("posePhase".into(), json!(if live { 2 } else { 0 }));
    let status = match phase {
        "live" => "实时定位",
        "stale" => "定位已过期",
        "invalid" => "定位数据无效",
        _ => "等待定位数据",
    };
    state.insert("poseStatus".into(), json!(status));
    if live {
        let x = v["x"].as_f64().unwrap_or(0.0);
        let y = v["y"].as_f64().unwrap_or(0.0);
        state.insert("poseX".into(), json!(x));
        state.insert("poseY".into(), json!(y));
        state.insert("poseCoords".into(), json!(format!("X {:.2} m · Y {:.2} m", x, y)));
        if let Some(h) = v["heading"].as_f64() {
            state.insert("poseHeading".into(), json!(h));
            state.insert("poseHasHeading".into(), json!(true));
            state.insert("poseHeadingText".into(), json!(format!("{:.1}°", h)));
        } else {
            state.insert("poseHasHeading".into(), json!(false));
            state.insert("poseHeadingText".into(), json!("—"));
        }
        let conf = v["confidence"]
            .as_f64()
            .map(|c| format!("{:.0}%", c * 100.0))
            .unwrap_or_else(|| "—".into());
        let age = v["ageMs"]
            .as_f64()
            .map(|a| format!("{:.0} ms", a))
            .unwrap_or_else(|| "—".into());
        state.insert("poseConfidenceText".into(), json!(conf));
        state.insert("poseAgeText".into(), json!(age));
    } else {
        state.insert("poseCoords".into(), json!("—"));
        state.insert("poseHeadingText".into(), json!("—"));
        state.insert("poseConfidenceText".into(), json!("—"));
        state.insert("poseAgeText".into(), json!("—"));
    }
}

fn apply_vision_status_to_state(state: &mut Map<String, Value>, status: &Value) {
    let phase = status["phase"].as_str().unwrap_or("idle");
    let telem = &status["telemetry"];
    if phase == "connected" {
        state.insert("loading".into(), json!(false));
        state.insert("status".into(), json!("实时画面"));
        state.insert("statusPhase".into(), json!(2));
    } else if phase == "connecting" {
        state.insert("loading".into(), json!(true));
        state.insert("status".into(), json!("正在连接…"));
        state.insert("statusPhase".into(), json!(1));
    }
    if let Some(obj) = telem.as_object() {
        let mut facts = Vec::new();
        if let (Some(w), Some(h)) = (obj.get("width").and_then(Value::as_u64), obj.get("height").and_then(Value::as_u64)) {
            facts.push(json!({"label": "分辨率", "value": format!("{w} × {h}")}));
        }
        if let Some(fps) = obj.get("fps").and_then(Value::as_f64) {
            facts.push(json!({"label": "帧率", "value": format!("{fps:.1} FPS")}));
        }
        if let Some(ms) = obj.get("processMs").and_then(Value::as_f64) {
            facts.push(json!({"label": "算法耗时", "value": format!("{ms:.1} ms")}));
        }
        if let Some(src) = obj.get("sourceLabel").and_then(Value::as_str) {
            state.insert("sourceBadge".into(), json!(src));
        }
        if let Some(summary) = obj.get("summary").and_then(Value::as_str) {
            facts.push(json!({"label": "检测结果", "value": summary}));
        }
        if !facts.is_empty() {
            state.insert("facts".into(), Value::Array(facts));
        }
    }
}

fn build_vision_config(state: &Map<String, Value>, cameras: &[String], windows: &[String]) -> Value {
    let mode_idx = state.get("sourceMode").and_then(Value::as_i64).unwrap_or(0);
    let source = match mode_idx {
        1 => "window",
        2 => "shared",
        _ => "direct",
    };
    let cam_idx = state.get("cameraIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
    let win_idx = state.get("windowIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
    let rate_idx = state.get("rate").and_then(Value::as_u64).unwrap_or(1) as usize;
    let fps = [8, 15, 20, 30].get(rate_idx).copied().unwrap_or(15);
    let algo_idx = state.get("algo").and_then(Value::as_i64).unwrap_or(0);
    let algo = match algo_idx {
        1 => "binary",
        2 => "edges",
        3 => "hsv",
        4 => "shapes",
        5 => "apriltag",
        6 => "face",
        _ => "raw",
    };
    json!({
        "source": source,
        "device": cameras.get(cam_idx).cloned().unwrap_or_else(|| "/dev/video0".into()),
        "window": windows.get(win_idx).cloned().unwrap_or_default(),
        "fps": fps,
        "algo": algo,
        "applyOnWatch": state.get("applyOnWatch").and_then(Value::as_bool).unwrap_or(false),
        "flipH": state.get("flipH").and_then(Value::as_bool).unwrap_or(false),
        "flipV": state.get("flipV").and_then(Value::as_bool).unwrap_or(false),
        "crosshair": state.get("crosshair").and_then(Value::as_bool).unwrap_or(true),
        "binaryMode": state.get("binaryMode").and_then(Value::as_i64).unwrap_or(2),
        "thresh": state.get("thresh").and_then(Value::as_f64).unwrap_or(127.0).round() as i32,
        "blur": state.get("blur").and_then(Value::as_f64).unwrap_or(5.0).round() as i32,
        "featureMode": state.get("featureMode").and_then(Value::as_i64).unwrap_or(0),
        "cannyLow": state.get("cannyLow").and_then(Value::as_f64).unwrap_or(30.0).round() as i32,
        "cannyHigh": state.get("cannyHigh").and_then(Value::as_f64).unwrap_or(100.0).round() as i32,
        "hsvView": state.get("hsvView").and_then(Value::as_i64).unwrap_or(0),
        "hLow": state.get("hLow").and_then(Value::as_f64).unwrap_or(0.0).round() as i32,
        "hHigh": state.get("hHigh").and_then(Value::as_f64).unwrap_or(10.0).round() as i32,
        "sLow": state.get("sLow").and_then(Value::as_f64).unwrap_or(50.0).round() as i32,
        "sHigh": state.get("sHigh").and_then(Value::as_f64).unwrap_or(255.0).round() as i32,
        "vLow": state.get("vLow").and_then(Value::as_f64).unwrap_or(50.0).round() as i32,
        "vHigh": state.get("vHigh").and_then(Value::as_f64).unwrap_or(255.0).round() as i32,
        "morph": state.get("morph").and_then(Value::as_bool).unwrap_or(true),
        "lineRoi": state.get("lineRoi").and_then(Value::as_bool).unwrap_or(false),
        "houghParam2": state.get("houghParam2").and_then(Value::as_f64).unwrap_or(35.0).round() as i32,
        "minRadius": state.get("minRadius").and_then(Value::as_f64).unwrap_or(12.0).round() as i32,
        "drawContours": state.get("drawContours").and_then(Value::as_bool).unwrap_or(true),
        "tagFamily": state.get("tagFamily").and_then(Value::as_i64).unwrap_or(0),
        "tagGuide": state.get("tagGuide").and_then(Value::as_bool).unwrap_or(true),
        "faceNeighbors": state.get("faceNeighbors").and_then(Value::as_f64).unwrap_or(4.0).round() as i32,
        "faceMinSize": state.get("faceMinSize").and_then(Value::as_f64).unwrap_or(36.0).round() as i32,
    })
}

async fn run_vision_stream(
    services: Arc<Services>,
    generation: u64,
    cancel: CancellationToken,
    mut config_rx: watch::Receiver<Value>,
    frame_tx: watch::Sender<Option<Arc<VisionSample>>>,
    status_tx: watch::Sender<Value>,
) -> Result<()> {
    let initial = config_rx.borrow_and_update().clone();
    let id = services
        .vision
        .start(initial, generation, cancel.clone())
        .await?;
    let mut seq = 0u64;
    let mut ticker = tokio::time::interval(Duration::from_millis(16));
    let result = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break Ok(()),
            changed = config_rx.changed() => {
                if changed.is_ok() {
                    let next = config_rx.borrow_and_update().clone();
                    let _ = services.vision.update_config(next);
                }
            }
            _ = ticker.tick() => {
                let st = services.vision.status();
                let _ = status_tx.send(st.clone());
                let phase = st["phase"].as_str().unwrap_or("idle");
                if phase == "error" {
                    break Err(anyhow!("{}", st["message"].as_str().unwrap_or("视觉流已结束。")));
                }
                if phase == "idle" && st["id"].as_u64() == Some(id) && seq > 0 {
                    break Ok(());
                }
                if let Ok(Some(frame)) = services.vision.native_frame(id, seq, generation) {
                    seq = frame.sequence;
                    if let Ok(rgb) = decode(seq, &frame.jpeg) {
                        let _ = frame_tx.send(Some(Arc::new(VisionSample {
                            rgb,
                            jpeg: frame.jpeg,
                        })));
                    }
                }
            }
        }
    };
    services.vision.stop(Some(id)).await;
    result
}

fn default_downloads_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            let dl = PathBuf::from(profile).join("Downloads");
            if dl.is_dir() {
                return dl;
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let dl = PathBuf::from(home).join("Downloads");
            if dl.is_dir() {
                return dl;
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn open_in_os_explorer(path: &PathBuf) -> Result<()> {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer");
        if path.is_file() {
            cmd.arg(format!("/select,{}", path.display()));
        } else {
            cmd.arg(path);
        }
        let _ = cmd.spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if path.is_file() {
            cmd.arg("-R").arg(path);
        } else {
            cmd.arg(path);
        }
        let _ = cmd.spawn();
    }
    Ok(())
}

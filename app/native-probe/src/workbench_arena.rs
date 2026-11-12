use crate::{workbench::text, workbench_ui::Workbench};
use portcanvas_core::services::Services;
use portcanvas_native_probe::{
    arena_model::{pose_live, Metrics},
    camera_client::{self, Frame},
    file_browser::bytes,
};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, Rgb8Pixel, SharedPixelBuffer, VecModel};
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
    metrics: RefCell<Metrics>,
    plot_aspect: Cell<f64>,
    latest: RefCell<Option<(Value, Value)>>,
    devices: RefCell<Vec<String>>,
    listing: Cell<u64>,
    camera_epoch: Cell<u64>,
    camera: RefCell<Option<Camera>>,
    timer: slint::Timer,
    camera_timer: slint::Timer,
    reads: Arc<AtomicU64>,
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
            metrics: RefCell::new(Metrics::default()),
            plot_aspect: Cell::new(10.),
            latest: RefCell::new(None),
            devices: RefCell::new(Vec::new()),
            listing: Cell::new(0),
            camera_epoch: Cell::new(0),
            camera: RefCell::new(None),
            timer: slint::Timer::default(),
            camera_timer: slint::Timer::default(),
            reads: Arc::new(AtomicU64::new(0)),
            uploads: Cell::new(0),
            visibility: RefCell::new(Vec::new()),
        });
        ui.on_arena_plot_changed({let c=c.clone();move|aspect|{if aspect>0.{c.plot_aspect.set(aspect as f64);c.render_paths();}}});
        ui.on_arena_sync({
            let c = c.clone();
            move || c.sync()
        });
        ui.on_arena_metrics_result({let c=c.clone();move|ticket,packet|{
            if ticket.parse::<u64>().ok()!=Some(c.epoch.get())||!c.visible.get(){return;}
            if let Ok(v)=serde_json::from_str::<Value>(&packet){
                if v["error"].is_string(){c.metrics.borrow_mut().failed();*c.latest.borrow_mut()=Some((Value::Null,v));}
                else if let Some(r)=c.metrics.borrow_mut().push(v.clone()){*c.latest.borrow_mut()=Some((v,json!({"cpu":r.cpu,"memory":r.memory,"received":r.received,"sent":r.sent})));}
                c.render_metrics();
            }
        }});
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
                let selected = c.devices.borrow().get(ui.get_arena_camera_index().max(0) as usize).cloned();
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
                let index = selected.and_then(|path| c.devices.borrow().iter().position(|p| *p == path)).unwrap_or(0);
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
        let active = ui.get_page() == 7 && !ui.get_native_hidden() && ui.get_connected();
        if active == self.visible.get() && generation == self.generation.get() {
            self.render_pose();
            self.render_metrics();
            return;
        }
        self.cancel.borrow().cancel();
        self.stop_camera();
        self.timer.stop();
        self.pose_id.set(None);
        self.epoch.set(self.epoch.get() + 1);
        self.generation.set(generation);
        self.visible.set(active);
        *self.pose.borrow_mut() = (json!({"phase":"unavailable"}), Instant::now());
        self.metrics.borrow_mut().clear();
        *self.latest.borrow_mut() = None;
        if std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_some() {
            self.visibility.borrow_mut().push(json!({"active":active,"metrics":self.reads.load(Ordering::SeqCst),"uploads":self.uploads.get()}));
        }
        self.render_pose();
        self.render_metrics();
        if !active {
            return;
        }
        self.list_cameras();
        let cancel = CancellationToken::new();
        *self.cancel.borrow_mut() = cancel.clone();
        let ticket = self.epoch.get();
        let s = self.services.clone();
        let weak = self.ui.clone();
        let count = self.reads.clone();
        self.spawn(async move {
            let pose = async {
                if let Ok(v) = s
                    .pose
                    .start_for_generation(generation, cancel.clone())
                    .await
                {
                    let id = v["id"].as_u64().unwrap();
                    let w = weak.clone();
                    let _ = w.upgrade_in_event_loop(move |ui| {
                        ui.invoke_arena_pose_started(
                            ticket.to_string().into(),
                            id.to_string().into(),
                        )
                    });
                    cancel.cancelled().await;
                    s.pose.stop(Some(id)).await;
                }
            };
            let metrics = async {
                let mut tick = tokio::time::interval(Duration::from_secs(2));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {biased;_=cancel.cancelled()=>break,_=tick.tick()=>{}}
                    s.pose.touch();
                    count.fetch_add(1, Ordering::SeqCst);
                    let v = s
                        .arena
                        .metrics()
                        .await
                        .unwrap_or_else(|e| json!({"error":e.to_string()}));
                    if cancel.is_cancelled() {
                        break;
                    }
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.invoke_arena_metrics_result(
                            ticket.to_string().into(),
                            v.to_string().into(),
                        )
                    });
                }
            };
            tokio::join!(pose, metrics);
        });
        let weak = Rc::downgrade(self);
        self.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(250),
            move || {
                if let Some(c) = weak.upgrade() {
                    c.render_pose();
                }
            },
        );
    }
    fn list_cameras(self: &Rc<Self>) {
        if !self.visible.get() || self.camera.borrow().is_some() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_arena_camera_loading(true);
        ui.set_arena_camera_status(text("正在读取摄像头列表…", ui.get_english()).into());
        self.listing.set(self.listing.get() + 1);
        let serial = self.listing.get();
        let ticket = self.epoch.get();
        let s = self.services.clone();
        let weak = self.ui.clone();
        self.spawn(async move {
            let v = s
                .arena
                .cameras()
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
        if live {
            ui.set_arena_world_x(v["x"].as_f64().unwrap() as f32);
            ui.set_arena_world_y(v["y"].as_f64().unwrap() as f32);
            ui.set_arena_heading(v["heading"].as_f64().unwrap_or(0.) as f32);
            ui.set_arena_has_heading(v["heading"].is_number());
        }
    }
    fn render_paths(&self){if let Some(ui)=self.ui.upgrade(){
        ui.set_arena_cpu_path(self.metrics.borrow().path_for_aspect(false,self.plot_aspect.get()).into());
        ui.set_arena_memory_path(self.metrics.borrow().memory_dashes_for_aspect(self.plot_aspect.get()).into());
    }}
    fn render_metrics(&self) {
        let Some(ui)=self.ui.upgrade() else{return};let latest=self.latest.borrow();let english=ui.get_english();
        let labels=["CPU","内存","交换空间","往返时间","接收","发送","运行时间","负载"];
        let mut rows=labels.into_iter().enumerate().map(|(i,label)|crate::workbench_ui::MetricRow{label:text(label,english).into(),value:"—".into(),usage:-1.,wide:i>=6}).collect::<Vec<_>>();
        if let Some((v,r))=latest.as_ref(){
            if let Some(error)=r["error"].as_str(){ui.set_arena_metrics_status(text(error,english).into());}
            else{
                ui.set_arena_metrics_status(text("实时",english).into());
                if let Some(cpu)=r["cpu"].as_f64(){rows[0].value=format!("{cpu:.1}%").into();rows[0].usage=cpu as f32;}
                if let (Some(total),Some(free))=(v["memoryTotal"].as_u64(),v["memoryAvailable"].as_u64()){
                    if total>0&&free<=total{rows[1].value=format!("{} / {}",bytes(total-free),bytes(total)).into();rows[1].usage=(100.*(1.-free as f64/total as f64)) as f32;}
                }
                if let (Some(total),Some(free))=(v["swapTotal"].as_u64(),v["swapFree"].as_u64()){
                    if free<=total{rows[2].value=format!("{} / {}",bytes(total-free),bytes(total)).into();}
                }
                if let Some(ms)=v["roundTripMs"].as_f64(){rows[3].value=format!("{ms:.0} ms").into();}
                for (index,field) in [(4,"received"),(5,"sent")]{if let Some(rate)=r[field].as_f64(){rows[index].value=format!("{}/s",bytes(rate as u64)).into();}}
                if let Some(seconds)=v["uptime"].as_f64(){rows[6].value=format!("{:.0} min",seconds/60.).into();}
                let load=v["load"].as_array().into_iter().flatten().filter_map(Value::as_f64).map(|n|format!("{n:.2}")).collect::<Vec<_>>().join(" / ");
                if !load.is_empty(){rows[7].value=load.into();}
            }
        }else{ui.set_arena_metrics_status(text(if ui.get_connected(){"正在读取…"}else{"设备未连接"},english).into());}
        ui.set_arena_metric_rows(Rc::new(VecModel::from(rows)).into());self.render_paths();
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
        self.cancel.borrow().cancel();
        self.stop_camera();
        self.timer.stop();
        self.visible.set(false);
    }
    pub fn report(&self) -> Value {
        json!({"metricsRequests":self.reads.load(Ordering::SeqCst),"uploads":self.uploads.get(),"active":self.visible.get(),"cameraActive":self.camera.borrow().is_some(),"visibility":*self.visibility.borrow()})
    }
}

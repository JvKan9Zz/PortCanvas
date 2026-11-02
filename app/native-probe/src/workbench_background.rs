use crate::{workbench::text, workbench_ui::Workbench};
use anyhow::{ensure, Context, Result};
use portcanvas_core::{
    preferences::{BackgroundCrop as Crop, BackgroundDraft, Preferences},
    services::Services,
};
use portcanvas_native_probe::{
    background_image::{self, Picture},
    background_model as geometry,
    background_playback::Playback,
};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, task::JoinHandle};
use tokio_util::sync::CancellationToken;
enum Asset {
    Still(Picture),
    Video(Playback),
}
struct Ready {
    asset: Option<Asset>,
    draft: Option<Arc<BackgroundDraft>>,
    preferences: Option<Preferences>,
    path: Option<PathBuf>,
}
struct Canvas {
    image: slint::Image,
    player: Option<Playback>,
    sequence: u64,
    width: u32,
    height: u32,
    path: PathBuf,
    video: bool,
}
struct Draft {
    canvas: Canvas,
    source: Arc<BackgroundDraft>,
    crop: Crop,
    ratio: f64,
}
type Pending = Arc<Mutex<Option<(u64, Ready)>>>;
pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    ticket: Arc<AtomicU64>,
    cancel: RefCell<CancellationToken>,
    pending: Pending,
    saved: RefCell<Option<Canvas>>,
    draft: RefCell<Option<Draft>>,
    drag: Cell<Option<(f64, f64, Crop, Option<usize>)>>,
    timer: slint::Timer,
    save_timer: slint::Timer,
    uploads: Cell<u64>,
    save_error: RefCell<String>,
    hidden: Cell<Option<bool>>,
    visibility: RefCell<Vec<serde_json::Value>>,
    last_playback: RefCell<serde_json::Value>,
}
fn picture(p: Picture) -> slint::Image {
    slint::Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &p.rgba, p.width, p.height,
    ))
}
fn color(value: &str) -> Option<slint::Color> {
    let rgb = u32::from_str_radix(value.strip_prefix('#')?, 16).ok()?;
    Some(slint::Color::from_rgb_u8(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
    ))
}
fn asset(
    path: PathBuf,
    video: bool,
    crop: Option<Crop>,
    volume: u8,
    cancel: &CancellationToken,
) -> Result<Asset> {
    if !video {
        return Ok(Asset::Still(
            background_image::load(&path, crop, if crop.is_some() { 4096 } else { 2048 })
                .context("图片无法读取，请选择其他图片。")?,
        ));
    }
    let player = Playback::open(path, volume, false);
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        ensure!(!cancel.is_cancelled(), "已取消背景导入。");
        if player.read(|s| s.frame.is_some()) {
            return Ok(Asset::Video(player));
        }
        if let Some(error) = player.read(|s| s.error.clone()) {
            return Err(anyhow::anyhow!("{error}")).context("媒体无法读取，请选择其他图片或视频。");
        }
        ensure!(Instant::now() < end, "视频读取超时。");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn canvas(asset: Asset, path: PathBuf) -> Canvas {
    match asset {
        Asset::Still(p) => {
            let (width, height) = (p.width, p.height);
            Canvas {
                image: picture(p),
                player: None,
                sequence: 0,
                width,
                height,
                path,
                video: false,
            }
        }
        Asset::Video(player) => {
            let (sequence, f) = player.read(|s| (s.sequence, s.frame.clone().unwrap()));
            let image = slint::Image::from_rgba8(
                SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&f.rgba, f.width, f.height),
            );
            Canvas {
                image,
                sequence,
                player: Some(player),
                width: f.width,
                height: f.height,
                path,
                video: true,
            }
        }
    }
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
            ticket: Arc::new(AtomicU64::new(0)),
            cancel: RefCell::new(CancellationToken::new()),
            pending: Arc::new(Mutex::new(None)),
            saved: RefCell::new(None),
            draft: RefCell::new(None),
            drag: Cell::new(None),
            timer: slint::Timer::default(),
            save_timer: slint::Timer::default(),
            uploads: Cell::new(0),
            save_error: RefCell::new(String::new()),
            hidden: Cell::new(None),
            visibility: RefCell::new(Vec::new()),
            last_playback: RefCell::new(serde_json::Value::Null),
        });
        ui.on_bg_sync({
            let c = c.clone();
            move || c.sync()
        });
        ui.on_bg_action({
            let c = c.clone();
            move |action| match action.as_str() {
                "image" => c.choose(false),
                "video" => c.choose(true),
                "cancel" => c.cancel_stage(),
                "color" => c.color(false),
                "reset" => c.color(true),
                _ => {}
            }
        });
        ui.on_bg_controls({
            let c = c.clone();
            move || {
                if let Some(ui) = c.ui.upgrade() {
                    if let Some(p) = c.saved.borrow().as_ref().and_then(|c| c.player.as_ref()) {
                        p.volume(ui.get_bg_volume().round().clamp(0., 100.) as u8);
                    }
                }
                let weak = Rc::downgrade(&c);
                c.save_timer.start(
                    slint::TimerMode::SingleShot,
                    Duration::from_millis(200),
                    move || {
                        if let Some(c) = weak.upgrade() {
                            c.persist();
                        }
                    },
                );
            }
        });
        ui.on_bg_progress({
            let c = c.clone();
            move |ticket, done, total| {
                if ticket.parse::<u64>().ok() == Some(c.ticket.load(Ordering::SeqCst)) {
                    if let Some(ui) = c.ui.upgrade() {
                        ui.set_bg_status(format!("{} / {}", done, total).into());
                    }
                }
            }
        });
        ui.on_bg_ready({
            let c = c.clone();
            move |ticket, error| {
                if ticket.parse::<u64>().ok() == Some(c.ticket.load(Ordering::SeqCst)) {
                    c.ready(error.to_string());
                }
            }
        });
        ui.on_bg_crop_response({
            let c = c.clone();
            move |accepted| c.respond(accepted)
        });
        ui.on_bg_crop_reset({
            let c = c.clone();
            move || {
                if let (Some(ui), Some(d)) = (c.ui.upgrade(), c.draft.borrow_mut().as_mut()) {
                    d.crop = geometry::initial(1., d.ratio);
                    set_crop(&ui, d.crop);
                }
            }
        });
        ui.on_bg_crop_arrow({
            let c = c.clone();
            move |dx, dy, resize| {
                if let Some(d) = c.draft.borrow_mut().as_mut() {
                    d.crop = if resize {
                        geometry::resized(d.crop, 3, dx as f64, dy as f64, d.ratio)
                    } else {
                        geometry::moved(d.crop, dx as f64, dy as f64)
                    };
                    if let Some(ui) = c.ui.upgrade() {
                        set_crop(&ui, d.crop);
                    }
                }
            }
        });
        ui.on_bg_crop_gesture({
            let c = c.clone();
            move |kind, x, y| {
                let x = x as f64;
                let y = y as f64;
                if kind == 2 {
                    c.drag.set(None);
                    return;
                }
                if let Some(d) = c.draft.borrow_mut().as_mut() {
                    if kind == 0 {
                        let corner = [
                            (d.crop.x, d.crop.y),
                            (d.crop.x + d.crop.width, d.crop.y),
                            (d.crop.x, d.crop.y + d.crop.height),
                            (d.crop.x + d.crop.width, d.crop.y + d.crop.height),
                        ]
                        .iter()
                        .position(|&(a, b)| (x - a).abs() < 0.04 && (y - b).abs() < 0.04);
                        c.drag.set(Some((x, y, d.crop, corner)));
                    } else if let Some((a, b, start, corner)) = c.drag.get() {
                        d.crop = if let Some(corner) = corner {
                            geometry::resized(start, corner, x - a, y - b, d.ratio)
                        } else {
                            geometry::moved(start, x - a, y - b)
                        };
                        if let Some(ui) = c.ui.upgrade() {
                            set_crop(&ui, d.crop);
                        }
                    }
                }
            }
        });
        c.reload();
        c
    }
    fn spawn(&self, f: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|j| !j.is_finished());
        jobs.push(self.runtime.spawn(f));
    }
    fn begin(&self, cancellable: bool) -> (u64, CancellationToken) {
        self.cancel.borrow().cancel();
        let cancel = CancellationToken::new();
        *self.cancel.borrow_mut() = cancel.clone();
        let ticket = self.ticket.fetch_add(1, Ordering::SeqCst) + 1;
        self.pending.lock().unwrap().take();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_bg_busy(true);
            ui.set_bg_cancellable(cancellable);
            ui.set_bg_error("".into());
        }
        (ticket, cancel)
    }
    fn task(self: &Rc<Self>, ticket: u64, work: impl FnOnce() -> Result<Ready> + Send + 'static) {
        let pending = self.pending.clone();
        let current = self.ticket.clone();
        let weak = self.ui.clone();
        self.spawn(async move {
            let result = tokio::task::spawn_blocking(work)
                .await
                .map_err(anyhow::Error::from)
                .and_then(|v| v);
            let error = match result {
                Ok(ready) => {
                    let mut slot = pending.lock().unwrap();
                    if current.load(Ordering::SeqCst) != ticket {
                        return;
                    }
                    *slot = Some((ticket, ready));
                    String::new()
                }
                Err(e) => {
                    if current.load(Ordering::SeqCst) != ticket {
                        return;
                    }
                    eprintln!("Background operation: {e:#}");
                    e.to_string()
                }
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_bg_ready(ticket.to_string().into(), error.into())
            });
        });
    }
    fn reload(self: &Rc<Self>) {
        let (ticket, cancel) = self.begin(false);
        let s = self.services.clone();
        self.task(ticket, move || saved(&s, &cancel));
    }
    fn choose(self: &Rc<Self>, video: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_bg_busy() || ui.get_dialog_kind() != 0 || ui.get_busy() {
            return;
        }
        if let Some(p) = self.saved.borrow().as_ref().and_then(|c| c.player.as_ref()) {
            p.paused(true);
        }
        let path = ui
            .window()
            .with_winit_window(|window| {
                let dialog = rfd::FileDialog::new().set_parent(window).set_title(text(
                    if video {
                        "选择背景视频"
                    } else {
                        "选择背景图片"
                    },
                    ui.get_english(),
                ));
                if video {
                    dialog
                        .add_filter("MP4 / WebM", &["mp4", "m4v", "webm"])
                        .pick_file()
                } else {
                    dialog
                        .add_filter("PNG / JPEG / WebP", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                }
            })
            .flatten();
        let Some(path) = path else {
            self.sync();
            return;
        };
        let (ticket, cancel) = self.begin(true);
        let s = self.services.clone();
        let weak = self.ui.clone();
        self.task(ticket, move || {
            let directory = s.preferences.lock().unwrap().backgrounds()?;
            let mut last = Instant::now() - Duration::from_secs(1);
            let source = BackgroundDraft::open(&path, &directory, video, |done, total| {
                ensure!(!cancel.is_cancelled(), "已取消背景导入。");
                if done == total || last.elapsed() >= Duration::from_millis(100) {
                    last = Instant::now();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.invoke_bg_progress(
                            ticket.to_string().into(),
                            portcanvas_native_probe::file_browser::bytes(done).into(),
                            portcanvas_native_probe::file_browser::bytes(total).into(),
                        )
                    });
                }
                Ok(())
            })?;
            let media = asset(source.path.clone(), video, None, 0, &cancel)?;
            Ok(Ready {
                asset: Some(media),
                path: Some(source.path.clone()),
                draft: Some(Arc::new(source)),
                preferences: None,
            })
        });
        self.sync();
    }
    fn ready(self: &Rc<Self>, error: String) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_bg_cancellable(false);
        if !error.is_empty() {
            ui.set_bg_error(error.into());
            ui.set_bg_status("".into());
            self.recover_players();
            ui.set_bg_busy(false);
            self.sync();
            return;
        }
        let ready = self.pending.lock().unwrap().take();
        let Some((_, ready)) = ready else { return };
        if ready.draft.is_some()
            && (ui.get_page() != 1 || ui.get_native_hidden() || ui.get_dialog_kind() != 0)
        {
            drop(ready);
            ui.set_bg_busy(false);
            self.sync();
            return;
        }
        if let Some(source) = ready.draft {
            let media = canvas(ready.asset.unwrap(), ready.path.unwrap());
            let ratio = (ui.window().size().width as f64 / ui.window().size().height.max(1) as f64)
                / (media.width as f64 / media.height as f64);
            let crop = geometry::initial(1., ratio);
            ui.set_bg_preview(media.image.clone());
            ui.set_bg_preview_width(media.width as i32);
            ui.set_bg_preview_height(media.height as i32);
            set_crop(&ui, crop);
            *self.draft.borrow_mut() = Some(Draft {
                canvas: media,
                source,
                crop,
                ratio,
            });
            ui.set_bg_busy(false);
            ui.set_dialog_kind(8);
        } else if let Some(p) = ready.preferences {
            let previous = self.draft.borrow_mut().take();
            drop(previous);
            if ui.get_dialog_kind() == 8 {
                ui.set_dialog_kind(0);
            }
            ui.set_bg_preview(Default::default());
            let saved_color = p.background_color.clone();
            ui.set_bg_hex(
                saved_color
                    .clone()
                    .unwrap_or_else(|| "#eef3fb".into())
                    .into(),
            );
            ui.set_bg_color(
                saved_color
                    .as_deref()
                    .and_then(color)
                    .unwrap_or(slint::Color::from_rgb_u8(238, 243, 251)),
            );
            ui.set_bg_custom(saved_color.is_some() || ready.asset.is_some());
            ui.set_bg_volume(p.background_volume as f32);
            ui.set_bg_material(p.glass_opacity as f32);
            let media = ready.asset.map(|a| canvas(a, ready.path.unwrap()));
            let video = media.as_ref().is_some_and(|a| a.video);
            let crop = if video { p.background_crop } else { None }.unwrap_or(Crop {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.,
            });
            ui.set_bg_saved_crop_x(crop.x as f32);
            ui.set_bg_saved_crop_y(crop.y as f32);
            ui.set_bg_saved_crop_width(crop.width as f32);
            ui.set_bg_saved_crop_height(crop.height as f32);
            if let Some(a) = &media {
                ui.set_bg_frame(a.image.clone());
                ui.set_bg_width(a.width as i32);
                ui.set_bg_height(a.height as i32);
                self.uploads.set(self.uploads.get() + 1);
            } else {
                ui.set_bg_frame(Default::default());
            }
            ui.set_bg_has_frame(media.is_some());
            *self.saved.borrow_mut() = media;
        }
        ui.set_bg_busy(false);
        ui.set_bg_status("".into());
        self.sync();
    }
    fn recover_players(&self) {
        let volume = self
            .ui
            .upgrade()
            .map(|u| u.get_bg_volume() as u8)
            .unwrap_or(0);
        for data in [&self.saved] {
            if let Some(c) = data.borrow_mut().as_mut() {
                if c.video && c.player.is_none() && c.path.exists() {
                    c.player = Some(Playback::open(c.path.clone(), volume, true));
                    c.sequence = 0;
                }
            }
        }
        if let Some(d) = self.draft.borrow_mut().as_mut() {
            if d.canvas.video && d.canvas.player.is_none() && d.source.path.exists() {
                d.canvas.player = Some(Playback::open(d.source.path.clone(), 0, true));
                d.canvas.sequence = 0;
            }
        }
    }
    pub fn respond(self: &Rc<Self>, accepted: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_bg_busy() {
            return;
        }
        if !accepted {
            self.cancel_stage();
            return;
        }
        let (source, crop, preview) = {
            let mut draft = self.draft.borrow_mut();
            let Some(d) = draft.as_mut() else { return };
            (d.source.clone(), d.crop, d.canvas.player.take())
        };
        let previous = self
            .saved
            .borrow_mut()
            .as_mut()
            .and_then(|c| c.player.take());
        let (ticket, cancel) = self.begin(false);
        let s = self.services.clone();
        self.task(ticket, move || {
            drop(preview);
            drop(previous);
            source.commit(&s.preferences.lock().unwrap(), crop)?;
            saved(&s, &cancel)
        });
    }
    fn cancel_stage(self: &Rc<Self>) {
        self.cancel.borrow().cancel();
        self.ticket.fetch_add(1, Ordering::SeqCst);
        self.pending.lock().unwrap().take();
        let old = self.draft.borrow_mut().take();
        drop(old);
        self.drag.set(None);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_bg_busy(false);
            ui.set_bg_cancellable(false);
            ui.set_bg_preview(Default::default());
            if ui.get_dialog_kind() == 8 {
                ui.set_dialog_kind(0);
            }
            ui.set_bg_status("".into());
        }
        self.sync();
    }
    fn color(self: &Rc<Self>, reset: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_bg_busy() || ui.get_dialog_kind() != 0 {
            return;
        }
        let color = if reset {
            None
        } else {
            Some(ui.get_bg_hex().to_string())
        };
        let player = self
            .saved
            .borrow_mut()
            .as_mut()
            .and_then(|c| c.player.take());
        let (ticket, cancel) = self.begin(false);
        let s = self.services.clone();
        self.task(ticket, move || {
            drop(player);
            s.preferences.lock().unwrap().save_background_color(color)?;
            saved(&s, &cancel)
        });
    }
    fn persist(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let volume = ui.get_bg_volume().round().clamp(0., 100.) as u8;
        let material = ui.get_bg_material().round().clamp(0., 100.) as u8;
        let result = (|| {
            let store = self.services.preferences.lock().unwrap();
            let p = store.read()?;
            if p.background_volume != volume {
                store.save_background_volume(volume)?;
            }
            if p.glass_opacity != material {
                store.save_glass_opacity(material)?;
            }
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(e) = result {
            *self.save_error.borrow_mut() = e.to_string();
            ui.set_bg_error(e.to_string().into());
        }
    }
    pub fn sync(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let hidden = ui.get_native_hidden();
        if self.hidden.replace(Some(hidden)) != Some(hidden)
            && std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_some()
        {
            self.visibility.borrow_mut().push(serde_json::json!({"hidden":hidden,"textureUploads":self.uploads.get(),"playback":snapshot(self.saved.borrow().as_ref())}));
        }
        if ui.get_bg_cancellable() && (ui.get_page() != 1 || hidden) {
            self.cancel_stage();
            return;
        }
        let preview = self.draft.borrow().is_some();
        if let Some(p) = self.saved.borrow().as_ref().and_then(|c| c.player.as_ref()) {
            p.paused(hidden || preview || ui.get_bg_busy());
        }
        if let Some(p) = self
            .draft
            .borrow()
            .as_ref()
            .and_then(|d| d.canvas.player.as_ref())
        {
            p.paused(hidden || ui.get_bg_busy());
        }
        let videos = self
            .saved
            .borrow()
            .as_ref()
            .is_some_and(|c| c.player.is_some())
            || self
                .draft
                .borrow()
                .as_ref()
                .is_some_and(|d| d.canvas.player.is_some());
        if !hidden && videos {
            if !self.timer.running() {
                let weak = Rc::downgrade(self);
                self.timer.start(
                    slint::TimerMode::Repeated,
                    Duration::from_millis(33),
                    move || {
                        if let Some(c) = weak.upgrade() {
                            c.draw();
                        }
                    },
                );
            }
        } else {
            self.timer.stop();
        }
    }
    fn draw(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        for preview in [false, true] {
            let mut saved = self.saved.borrow_mut();
            let mut draft = self.draft.borrow_mut();
            let c = if preview {
                draft.as_mut().map(|d| &mut d.canvas)
            } else {
                saved.as_mut()
            };
            if let Some(c) = c {
                if let Some(p) = &c.player {
                    let (frame, sequence, error, fatal) = p.read(|s| {
                        (
                            s.frame.clone(),
                            s.sequence,
                            s.error.clone().or(s.audio_error.clone()),
                            s.error.is_some(),
                        )
                    });
                    if let Some(error) = error {
                        let message = if fatal {
                            "媒体无法读取，请选择其他图片或视频。"
                        } else {
                            "背景声音不可用，视频将静音继续播放。"
                        };
                        if ui.get_bg_error() != message {
                            eprintln!("Background playback: {error}");
                            ui.set_bg_error(message.into());
                        }
                    } else if ui.get_bg_error() == "背景声音不可用，视频将静音继续播放。"
                    {
                        ui.set_bg_error("".into());
                    }
                    if fatal {
                        c.player.take();
                    }
                    if sequence != c.sequence {
                        if let Some(f) = frame {
                            c.sequence = sequence;
                            c.width = f.width;
                            c.height = f.height;
                            c.image = slint::Image::from_rgba8(
                                SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                    &f.rgba, f.width, f.height,
                                ),
                            );
                            if preview {
                                ui.set_bg_preview(c.image.clone());
                                ui.set_bg_preview_width(f.width as i32);
                                ui.set_bg_preview_height(f.height as i32);
                            } else {
                                ui.set_bg_frame(c.image.clone());
                                ui.set_bg_width(f.width as i32);
                                ui.set_bg_height(f.height as i32);
                            }
                            self.uploads.set(self.uploads.get() + 1);
                        }
                    }
                }
            }
        }
        if !self
            .saved
            .borrow()
            .as_ref()
            .is_some_and(|c| c.player.is_some())
            && !self
                .draft
                .borrow()
                .as_ref()
                .is_some_and(|d| d.canvas.player.is_some())
        {
            self.timer.stop();
        }
    }
    pub fn shutdown(&self) {
        self.cancel.borrow().cancel();
        self.ticket.fetch_add(1, Ordering::SeqCst);
        self.timer.stop();
        if self.save_timer.running() {
            self.save_timer.stop();
            self.persist();
        }
        self.pending.lock().unwrap().take();
        *self.last_playback.borrow_mut() = snapshot(self.saved.borrow().as_ref());
        self.draft.borrow_mut().take();
        self.saved.borrow_mut().take();
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"lastPlayback":*self.last_playback.borrow(),"visibility":*self.visibility.borrow(),"textureUploads":self.uploads.get(),"saveError":*self.save_error.borrow(),"draft":self.draft.borrow().is_some(),"video":self.saved.borrow().as_ref().is_some_and(|c|c.player.is_some())})
    }
}
fn set_crop(ui: &Workbench, c: Crop) {
    ui.set_bg_crop_x(c.x as f32);
    ui.set_bg_crop_y(c.y as f32);
    ui.set_bg_crop_width(c.width as f32);
    ui.set_bg_crop_height(c.height as f32);
}
fn saved(s: &Services, cancel: &CancellationToken) -> Result<Ready> {
    let store = s.preferences.lock().unwrap();
    let p = store.read()?;
    let path = store.background_path()?;
    drop(store);
    let media = path
        .as_ref()
        .map(|path| {
            asset(
                path.clone(),
                p.background_video.is_some(),
                if p.background_video.is_some() {
                    None
                } else {
                    p.background_crop
                },
                p.background_volume,
                cancel,
            )
        })
        .transpose()?;
    Ok(Ready {
        asset: media,
        draft: None,
        preferences: Some(p),
        path,
    })
}

fn snapshot(canvas: Option<&Canvas>) -> serde_json::Value {
    canvas.and_then(|c|c.player.as_ref()).map(|p|p.read(|s|serde_json::json!({"sequence":s.sequence,"loops":s.loops,"position":s.position,"audioActive":s.audio_active,"audioVolume":s.audio_volume,"error":s.error,"audioError":s.audio_error}))).unwrap_or(serde_json::Value::Null)
}

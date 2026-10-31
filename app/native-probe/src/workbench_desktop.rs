use crate::{workbench::text, workbench_ui::Workbench};
use anyhow::Result;
use portcanvas_core::{desktop_settings::DesktopMode, services::Services};
use portcanvas_native_probe::{
    desktop_client::{self, Command, Input, Motion, State},
    rfb::{self, Frame},
};
use slint::{
    winit_030::{
        winit::{
            event::{ElementState, Ime, WindowEvent},
            keyboard::PhysicalKey,
        },
        WinitWindowAccessor,
    },
    ComponentHandle,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    runtime::Runtime,
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

struct Run {
    epoch: u64,
    generation: u64,
    cancel: CancellationToken,
    done: CancellationToken,
    input: mpsc::Sender<Input>,
    motion: watch::Sender<Motion>,
    frames: Option<watch::Receiver<Option<Arc<Frame>>>>,
    clipboard: Option<watch::Receiver<Option<String>>>,
    network: Option<State>,
    uploaded_view: Option<u64>,
    uploaded_pixels: Option<Arc<Vec<u8>>>,
    uploaded_cursor: Option<Arc<rfb::Cursor>>,
}
pub struct Controller {
    weak: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    running: RefCell<Option<Run>>,
    wanted: Cell<bool>,
    failed: Cell<bool>,
    allowed: Cell<bool>,
    epochs: Cell<u64>,
    sequence: Cell<u64>,
    physical: RefCell<HashMap<PhysicalKey, u32>>,
    current_key: Cell<Option<PhysicalKey>>,
    key_text: Cell<bool>,
    composing: Cell<bool>,
    buttons: Cell<u8>,
    size: Cell<(u16, u16)>,
    clip: RefCell<Option<String>>,
    timer: slint::Timer,
    pending_draw: Arc<AtomicU64>,
    draw_sequence: Cell<u64>,
    draw_ticket: Cell<u64>,
    uploads: Cell<u64>,
    draws: Cell<u64>,
    starts: Cell<u64>,
    visibility: RefCell<Vec<serde_json::Value>>,
}
fn image(width: u32, height: u32, pixels: &[u8]) -> slint::Image {
    slint::Image::from_rgba8(
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(pixels, width, height),
    )
}
impl Controller {
    pub fn install(
        ui: &Workbench,
        services: Arc<Services>,
        runtime: Arc<Runtime>,
        jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) -> Result<Rc<Self>> {
        let pending_draw = Arc::new(AtomicU64::new(0));
        let pending = pending_draw.clone();
        let weak = ui.as_weak();
        ui.window()
            .set_rendering_notifier(move |state, _| {
                if matches!(state, slint::RenderingState::AfterRendering) {
                    let ticket = pending.swap(0, Ordering::SeqCst);
                    if ticket != 0 {
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.invoke_desktop_drawn(ticket.to_string().into())
                        });
                    }
                }
            })
            .map_err(|error| anyhow::anyhow!("原生桌面绘制通知不可用：{error:?}"))?;
        let c = Rc::new(Self {
            weak: ui.as_weak(),
            services,
            runtime,
            jobs,
            running: RefCell::new(None),
            wanted: Cell::new(false),
            failed: Cell::new(false),
            allowed: Cell::new(false),
            epochs: Cell::new(0),
            sequence: Cell::new(0),
            physical: RefCell::new(HashMap::new()),
            current_key: Cell::new(None),
            key_text: Cell::new(false),
            composing: Cell::new(false),
            buttons: Cell::new(0),
            size: Cell::new((0, 0)),
            clip: RefCell::new(None),
            timer: slint::Timer::default(),
            pending_draw,
            draw_sequence: Cell::new(0),
            draw_ticket: Cell::new(0),
            uploads: Cell::new(0),
            draws: Cell::new(0),
            starts: Cell::new(0),
            visibility: RefCell::new(Vec::new()),
        });
        ui.on_desktop_view_changed({
            let c = c.clone();
            move || c.sync()
        });
        ui.on_desktop_quality_changed({
            let c = c.clone();
            move || {
                if c.wanted.get() {
                    c.failed.set(false);
                    c.cancel_active();
                }
                c.sync();
            }
        });
        ui.on_desktop_action({
            let c = c.clone();
            move |action| c.action(action.as_str())
        });
        ui.on_desktop_readonly_changed({
            let c = c.clone();
            move || c.release()
        });
        ui.on_desktop_focused({
            let c = c.clone();
            move |focused| {
                if !focused {
                    c.release();
                }
            }
        });
        ui.on_desktop_text({
            let c = c.clone();
            move |value| {
                if c.can_input() {
                    c.submit(Command::Text(value.into()));
                }
            }
        });
        ui.on_desktop_key({
            let c = c.clone();
            move |value, ctrl, alt, _shift| {
                if !c.can_input() {
                    return true;
                }
                let Some(ch) = value.chars().next() else {
                    return false;
                };
                if value.chars().count() != 1 || c.composing.get() {
                    return false;
                }
                let special = ch.is_control() || (0xf700..=0xf8ff).contains(&(ch as u32));
                if !special
                    && !ctrl
                    && !alt
                    && (!c.key_text.get()
                        || !matches!(c.current_key.get(), Some(PhysicalKey::Code(_))))
                {
                    return false;
                }
                let key = rfb::keysym(ch);
                if let Some(physical) = c.current_key.get() {
                    c.physical.borrow_mut().insert(physical, key);
                }
                c.submit(Command::Key(key, true));
                true
            }
        });
        ui.on_desktop_motion({
            let c = c.clone();
            move |x, y, w, h| {
                if c.can_input() {
                    if let Some(point) = c.point(x, y, w, h) {
                        if let Some(run) = c.running.borrow().as_ref() {
                            run.motion.send_replace(point);
                        }
                    }
                }
            }
        });
        ui.on_desktop_pointer({
            let c = c.clone();
            move |x, y, w, h, button, down| {
                if button == 0 || !c.can_input() {
                    return;
                }
                if let Some(point) = c.point(x, y, w, h) {
                    let button = button as u8;
                    let mask = if down {
                        c.buttons.get() | button
                    } else {
                        c.buttons.get() & !button
                    };
                    c.buttons.set(mask);
                    c.submit(Command::Pointer(point, mask));
                }
            }
        });
        ui.on_desktop_wheel({
            let c = c.clone();
            move |x, y, w, h, wheel, count| {
                if c.can_input() {
                    if let Some(point) = c.point(x, y, w, h) {
                        c.submit(Command::Wheel(point, wheel as u8, count.clamp(1, 8) as u8));
                    }
                }
            }
        });
        ui.on_desktop_notice({
            let c = c.clone();
            move |epoch, id, generation, view, ready| {
                let values = (
                    epoch.parse::<u64>(),
                    id.parse::<u64>(),
                    generation.parse::<u64>(),
                    view.parse::<u64>(),
                );
                if let (Ok(epoch), Ok(id), Ok(generation), Ok(view)) = values {
                    c.notice(
                        epoch,
                        State {
                            id,
                            generation,
                            view,
                            ready,
                        },
                    );
                }
            }
        });
        ui.on_desktop_finished({
            let c = c.clone();
            move |epoch, error| {
                if let Ok(epoch) = epoch.parse::<u64>() {
                    c.finished(epoch, error.as_str());
                }
            }
        });
        ui.on_desktop_drawn({
            let c = c.clone();
            move |ticket| {
                if ticket.parse::<u64>().ok() == Some(c.draw_ticket.get())
                    && c.draw_ticket.get() != 0
                    && c.drawable()
                {
                    c.draw_ticket.set(0);
                    c.draws.set(c.draws.get() + 1);
                    if let Some(ui) = c.weak.upgrade() {
                        ui.set_desktop_ready(true);
                        if ui.get_dialog_kind() == 0
                            && ui
                                .window()
                                .with_winit_window(|w| w.has_focus())
                                .unwrap_or(false)
                        {
                            ui.set_desktop_focus_revision(
                                ui.get_desktop_focus_revision().wrapping_add(1),
                            );
                        }
                    }
                    c.status();
                }
            }
        });
        ui.on_desktop_environment({
            let c = c.clone();
            move |packet| {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&packet) else {
                    return;
                };
                if value["feature"] != "desktop"
                    || value["generation"].as_u64() != Some(c.services.devices.generation())
                    || !c
                        .running
                        .borrow()
                        .as_ref()
                        .is_some_and(|r| !r.cancel.is_cancelled())
                    || c.services.devices.snapshot()["desktopStage"] != "prepare"
                {
                    return;
                }
                if let Some(ui) = c.weak.upgrade() {
                    let message = match value["phase"].as_str().unwrap_or_default() {
                        "downloading" => format!(
                            "{} {} / {} MiB",
                            text("正在下载组件", ui.get_english()),
                            value["done"].as_u64().unwrap_or(0) / 1024 / 1024,
                            value["total"].as_u64().unwrap_or(0) / 1024 / 1024
                        ),
                        "transferring" => format!(
                            "{} {} / {} MiB",
                            text("正在传输组件", ui.get_english()),
                            value["done"].as_u64().unwrap_or(0) / 1024 / 1024,
                            value["total"].as_u64().unwrap_or(0) / 1024 / 1024
                        ),
                        "board-downloading" => text("正在由板端下载组件…", ui.get_english()),
                        _ => text("正在准备远端运行环境…", ui.get_english()),
                    };
                    ui.set_desktop_status(message.into());
                }
            }
        });
        c.status();
        Ok(c)
    }
    fn allowed_now(&self) -> bool {
        self.weak
            .upgrade()
            .is_some_and(|ui| ui.get_page() == 5 && !ui.get_native_hidden() && ui.get_connected())
    }
    fn can_input(&self) -> bool {
        self.drawable()
            && self.weak.upgrade().is_some_and(|ui| {
                ui.get_desktop_ready()
                    && !ui.get_desktop_readonly()
                    && ui.get_dialog_kind() == 0
                    && ui
                        .window()
                        .with_winit_window(|w| w.has_focus())
                        .unwrap_or(false)
            })
    }
    fn drawable(&self) -> bool {
        self.size.get().0 > 0
            && self.size.get().1 > 0
            && self.allowed_now()
            && self.services.devices.snapshot()["desktop"] == "connected"
            && self.running.borrow().as_ref().is_some_and(|r| {
                !r.cancel.is_cancelled()
                    && r.network.is_some_and(|n| {
                        n.ready
                            && Some(n.view) == r.uploaded_view
                            && n.generation == self.services.devices.generation()
                    })
            })
    }
    fn next_sequence(&self) -> u64 {
        let n = self.sequence.get().wrapping_add(1);
        self.sequence.set(n);
        n
    }
    fn point(&self, x: f32, y: f32, w: f32, h: f32) -> Option<Motion> {
        let (width, height) = self.size.get();
        let (x, y) = desktop_client::point(x, y, w, h, width, height)?;
        let view = self.running.borrow().as_ref()?.uploaded_view?;
        Some(Motion {
            sequence: self.next_sequence(),
            view,
            x,
            y,
        })
    }
    fn submit(&self, command: Command) {
        let target = self
            .running
            .borrow()
            .as_ref()
            .filter(|r| !r.cancel.is_cancelled())
            .map(|r| {
                (
                    r.input.clone(),
                    r.uploaded_view.unwrap_or(0),
                    r.cancel.clone(),
                )
            });
        if let Some((input, view, cancel)) = target {
            if matches!(
                input.try_send(Input { view, command }),
                Err(mpsc::error::TrySendError::Full(_))
            ) {
                self.wanted.set(false);
                self.failed.set(true);
                cancel.cancel();
                self.clear_surface();
                self.error("桌面输入队列繁忙，连接已断开以释放按键。");
            }
        }
    }
    fn release(&self) {
        self.physical.borrow_mut().clear();
        self.buttons.set(0);
        self.submit(Command::Release(self.next_sequence()));
    }
    fn clear_surface(&self) {
        if let Some(run) = self.running.borrow_mut().as_mut() {
            run.uploaded_view = None;
            run.uploaded_pixels = None;
            run.uploaded_cursor = None;
        }
        self.pending_draw.store(0, Ordering::SeqCst);
        self.draw_ticket.set(0);
        self.size.set((0, 0));
        self.clip.borrow_mut().take();
        if let Some(ui) = self.weak.upgrade() {
            ui.set_desktop_ready(false);
            ui.set_desktop_frame(slint::Image::default());
            ui.set_desktop_cursor(slint::Image::default());
            ui.set_desktop_cursor_known(false);
            ui.set_desktop_source_width(0);
            ui.set_desktop_source_height(0);
            ui.set_desktop_clipboard_available(false);
        }
    }
    fn cancel_active(&self) {
        self.release();
        if let Some(run) = self.running.borrow_mut().as_mut() {
            run.cancel.cancel();
            run.frames.take();
            run.clipboard.take();
        }
        self.timer.stop();
        self.clear_surface();
    }
    pub fn sync(self: &Rc<Self>) {
        let allowed = self.allowed_now();
        let previous = self.allowed.replace(allowed);
        if allowed && !previous {
            self.failed.set(false);
        }
        let cancel = self.running.borrow().as_ref().is_some_and(|r| {
            !self.wanted.get() || !allowed || r.generation != self.services.devices.generation()
        });
        if cancel {
            self.cancel_active();
        }
        if self.running.borrow().is_none() && self.wanted.get() && allowed && !self.failed.get() {
            self.start();
        }
        if self.services.devices.snapshot()["desktop"] != "connected" {
            if let Some(ui) = self.weak.upgrade() {
                ui.set_desktop_ready(false);
            }
        }
        self.status();
    }
    fn start(self: &Rc<Self>) {
        self.clear_surface();
        if let Some(ui) = self.weak.upgrade() {
            ui.set_desktop_error("".into());
        }
        let epoch = self.epochs.get().wrapping_add(1);
        self.epochs.set(epoch);
        self.starts.set(self.starts.get() + 1);
        let generation = self.services.devices.generation();
        let cancel = CancellationToken::new();
        let done = CancellationToken::new();
        let (input, inputs) = mpsc::channel(64);
        let (motion, motions) = watch::channel(Motion::default());
        let (frames, frame_rx) = watch::channel(None);
        let (clipboard, clip_rx) = watch::channel(None);
        let mode = self
            .services
            .preferences
            .lock()
            .unwrap()
            .read()
            .map(|p| p.desktop_mode)
            .unwrap_or(DesktopMode::Smooth);
        *self.running.borrow_mut() = Some(Run {
            epoch,
            generation,
            cancel: cancel.clone(),
            done: done.clone(),
            input,
            motion,
            frames: Some(frame_rx),
            clipboard: Some(clip_rx),
            network: None,
            uploaded_view: None,
            uploaded_pixels: None,
            uploaded_cursor: None,
        });
        let weak = self.weak.clone();
        let notice_ui = weak.clone();
        let services = self.services.clone();
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|job| !job.is_finished());
        jobs.push(self.runtime.spawn(async move {
            let _done = done.drop_guard();
            let notice: desktop_client::StateSink = Arc::new(move |state| {
                let _ = notice_ui.upgrade_in_event_loop(move |ui| {
                    ui.invoke_desktop_notice(
                        epoch.to_string().into(),
                        state.id.to_string().into(),
                        state.generation.to_string().into(),
                        state.view.to_string().into(),
                        state.ready,
                    )
                });
            });
            let result = desktop_client::run(
                services,
                generation,
                mode,
                desktop_client::Channels {
                    inputs,
                    motion: motions,
                    frames,
                    clipboard,
                    state: notice,
                },
                cancel.clone(),
            )
            .await;
            let error = if cancel.is_cancelled() {
                String::new()
            } else {
                result.err().map(|e| e.to_string()).unwrap_or_default()
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_desktop_finished(epoch.to_string().into(), error.into())
            });
        }));
        drop(jobs);
        let target = Rc::downgrade(self);
        self.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || {
                if let Some(c) = target.upgrade() {
                    c.paint();
                }
            },
        );
    }
    fn notice(&self, epoch: u64, state: State) {
        let accepted = {
            let mut r = self.running.borrow_mut();
            if let Some(r) = r.as_mut().filter(|r| {
                r.epoch == epoch && !r.cancel.is_cancelled() && r.generation == state.generation
            }) {
                r.network = Some(state);
                true
            } else {
                false
            }
        };
        if !accepted {
            return;
        }
        if !state.ready {
            self.release();
            self.clear_surface();
        } else {
            self.paint();
            self.request_draw();
        }
        self.status();
    }
    fn request_draw(&self) {
        if !self.drawable() || self.weak.upgrade().is_some_and(|ui| ui.get_desktop_ready()) {
            return;
        }
        if self.draw_ticket.get() == 0 {
            let ticket = self.draw_sequence.get().wrapping_add(1).max(1);
            self.draw_sequence.set(ticket);
            self.draw_ticket.set(ticket);
            self.pending_draw.store(ticket, Ordering::SeqCst);
            if let Some(ui) = self.weak.upgrade() {
                ui.window().with_winit_window(|w| w.request_redraw());
            }
        }
    }
    fn paint(&self) {
        if !self.allowed_now() || self.services.devices.snapshot()["desktop"] != "connected" {
            return;
        }
        let (frame, clipboard) = {
            let mut run = self.running.borrow_mut();
            let Some(run) = run.as_mut().filter(|r| !r.cancel.is_cancelled()) else {
                return;
            };
            let uploaded = run.uploaded_view;
            let frame = run.frames.as_mut().and_then(|rx| {
                let needs = rx.has_changed().unwrap_or(false) || {
                    let current = rx.borrow();
                    current.as_ref().is_some_and(|f| Some(f.view) != uploaded)
                };
                if needs {
                    rx.borrow_and_update().clone()
                } else {
                    None
                }
            });
            let clipboard = run.clipboard.as_mut().and_then(|rx| {
                if rx.has_changed().unwrap_or(false) {
                    rx.borrow_and_update().clone()
                } else {
                    None
                }
            });
            if let Some(frame) = &frame {
                run.uploaded_view = Some(frame.view);
            }
            (frame, clipboard)
        };
        let Some(ui) = self.weak.upgrade() else {
            return;
        };
        if let Some(frame) = frame {
            let (changed, cursor_changed) = {
                let mut run = self.running.borrow_mut();
                let Some(run) = run.as_mut() else { return };
                let pixels = run
                    .uploaded_pixels
                    .as_ref()
                    .is_none_or(|p| !Arc::ptr_eq(p, &frame.pixels));
                let cursor = match (&run.uploaded_cursor, &frame.cursor) {
                    (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                    (None, None) => false,
                    _ => true,
                };
                run.uploaded_pixels = Some(frame.pixels.clone());
                run.uploaded_cursor = frame.cursor.clone();
                (pixels, cursor)
            };
            if changed {
                ui.set_desktop_frame(image(
                    frame.width.into(),
                    frame.height.into(),
                    &frame.pixels,
                ));
                self.size.set((frame.width, frame.height));
                ui.set_desktop_source_width(frame.width.into());
                ui.set_desktop_source_height(frame.height.into());
                self.uploads.set(self.uploads.get() + 1);
            }
            if cursor_changed {
                ui.set_desktop_cursor_known(frame.cursor.is_some());
                if let Some(cursor) = &frame.cursor {
                    ui.set_desktop_cursor_width(cursor.width.into());
                    ui.set_desktop_cursor_height(cursor.height.into());
                    ui.set_desktop_cursor_hot_x(cursor.hot_x.into());
                    ui.set_desktop_cursor_hot_y(cursor.hot_y.into());
                    ui.set_desktop_cursor(if cursor.width > 0 && cursor.height > 0 {
                        image(cursor.width.into(), cursor.height.into(), &cursor.pixels)
                    } else {
                        slint::Image::default()
                    });
                }
            }
        }
        if let Some(clipboard) = clipboard {
            *self.clip.borrow_mut() = Some(clipboard);
            ui.set_desktop_clipboard_available(true);
        }
        self.request_draw();
    }
    fn finished(self: &Rc<Self>, epoch: u64, error: &str) {
        let owned = {
            let mut run = self.running.borrow_mut();
            if run.as_ref().is_some_and(|r| r.epoch == epoch) {
                run.take();
                true
            } else {
                false
            }
        };
        if !owned {
            return;
        }
        self.timer.stop();
        self.clear_surface();
        if !error.is_empty() {
            self.failed.set(true);
            self.error(error);
        }
        self.sync();
    }
    fn action(self: &Rc<Self>, action: &str) {
        let Some(ui) = self.weak.upgrade() else {
            return;
        };
        if ui.get_dialog_kind() != 0 {
            return;
        }
        match action {
            "start" => {
                if !ui.get_connected() {
                    ui.set_page(0);
                    return;
                }
                self.failed.set(false);
                self.wanted.set(true);
                self.sync();
            }
            "stop" => {
                self.wanted.set(false);
                self.cancel_active();
                self.status();
            }
            "send-clipboard" if self.can_input() => {
                match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    Ok(text) => {
                        if text.chars().any(|c| u32::from(c) > 255) {
                            self.error("RFB剪贴板仅支持Latin-1文字，其他文字请直接输入。");
                        } else if text.chars().count() > 1024 * 1024 {
                            self.error("剪贴板文字过长。");
                        } else {
                            self.submit(Command::Clipboard(text));
                        }
                    }
                    Err(error) => self.error(&error.to_string()),
                }
            }
            "copy-clipboard" => {
                if let Some(text) = self.clip.borrow().clone() {
                    if let Err(error) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text))
                    {
                        self.error(&error.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    fn status(&self) {
        let Some(ui) = self.weak.upgrade() else {
            return;
        };
        let snapshot = self.services.devices.snapshot();
        let active = self.running.borrow().is_some();
        ui.set_desktop_active(active);
        ui.set_desktop_wanted(self.wanted.get());
        let source = if !ui.get_connected() {
            "尚未连接设备"
        } else if self.wanted.get() && !self.allowed_now() {
            "桌面已暂停。"
        } else if active && !ui.get_desktop_ready() {
            match snapshot["desktopStage"].as_str().unwrap_or_default() {
                "prepare" => "正在准备远端运行环境…",
                "service" => "正在启动远端桌面服务…",
                "tunnel" => "正在进行RFB握手…",
                "frame" => "正在接收完整桌面画面…",
                _ => "正在断开桌面…",
            }
        } else if ui.get_desktop_ready() {
            "远程桌面已连接"
        } else {
            "桌面已断开。"
        };
        ui.set_desktop_status(text(source, ui.get_english()).into());
    }
    fn error(&self, message: &str) {
        if let Some(ui) = self.weak.upgrade() {
            ui.set_desktop_error(text(message, ui.get_english()).into());
        }
    }
    pub fn window_event(&self, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(false) | WindowEvent::CursorLeft { .. } => self.release(),
            WindowEvent::Ime(Ime::Preedit(value, _)) => {
                self.current_key.set(None);
                self.key_text.set(false);
                self.composing.set(!value.is_empty());
            }
            WindowEvent::Ime(Ime::Commit(_) | Ime::Disabled) => {
                self.current_key.set(None);
                self.key_text.set(false);
                self.composing.set(false);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Released {
                    let key = self.physical.borrow_mut().remove(&event.physical_key);
                    if let Some(key) = key {
                        self.submit(Command::Key(key, false));
                    }
                } else {
                    self.current_key.set(Some(event.physical_key));
                    self.key_text
                        .set(event.text.as_ref().is_some_and(|s| !s.is_empty()));

                }
            }
            _ => {}
        }
    }
    pub fn visibility(&self, hidden: bool) {
        if std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_some() {
            self.visibility.borrow_mut().push(serde_json::json!({"hidden":hidden,"uploads":self.uploads.get(),"starts":self.starts.get(),"at":chrono::Utc::now().to_rfc3339()}));
        }
    }
    pub fn shutdown(&self) -> Option<CancellationToken> {
        self.wanted.set(false);
        self.cancel_active();
        self.running.borrow().as_ref().map(|r| r.done.clone())
    }
    pub fn finish_shutdown(&self) {
        if self
            .running
            .borrow()
            .as_ref()
            .is_some_and(|r| r.done.is_cancelled())
        {
            self.running.borrow_mut().take();
        }
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"wanted":self.wanted.get(),"active":self.running.borrow().is_some(),"starts":self.starts.get(),"uploads":self.uploads.get(),"firstDraws":self.draws.get(),"visibility":*self.visibility.borrow()})
    }
}

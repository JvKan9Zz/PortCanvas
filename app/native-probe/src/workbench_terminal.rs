use crate::{
    workbench::text,
    workbench_ui::{CompletionRow, TerminalCell, TerminalTab, Workbench},
};
use anyhow::Context;
use portcanvas_core::{services::Services, terminal::TerminalService};
use portcanvas_native_probe::{
    terminal_editor::Reply,
    terminal_sessions::{key_bytes, Sessions},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

struct Packet {
    service: TerminalService,
    device: String,
    key: String,
    id: u64,
    sequence: u64,
    bytes: Vec<u8>,
}
enum Input {
    Text(TerminalService, u64, String, Option<String>),
    Resize(TerminalService, u64, u32, u32),
}
pub struct Controller {
    weak: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    state: RefCell<Sessions>,
    input: mpsc::Sender<Input>,
    packets: Arc<Mutex<VecDeque<Packet>>>,
    wake: Arc<AtomicBool>,
    input_stop: tokio_util::sync::CancellationToken,
    resize_timer: slint::Timer,
    pending: RefCell<Option<(String, String, bool)>>,
    editor_focused: Cell<bool>,
    edit_revision: Cell<i32>,
}
impl Controller {
    fn job(&self, task: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|j| !j.is_finished());
        jobs.push(self.runtime.spawn(task));
    }
    pub fn install(
        ui: &Workbench,
        services: Arc<Services>,
        runtime: Arc<Runtime>,
        jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) -> Rc<Self> {
        let (input, mut receiver) = mpsc::channel::<Input>(64);
        let input_stop = tokio_util::sync::CancellationToken::new();
        let stop = input_stop.clone();
        let weak = ui.as_weak();
        jobs.lock().unwrap().push(runtime.spawn(async move {
            while let Some(request) =
                tokio::select! {biased;_=stop.cancelled()=>None,request=receiver.recv()=>request}
            {
                let (key, id, result, draft) = match request {
                    Input::Text(service, id, text, draft) => (
                        service.key().to_string(),
                        id,
                        service.write(id, text).await,
                        draft,
                    ),
                    Input::Resize(service, id, cols, rows) => (
                        service.key().to_string(),
                        id,
                        service.resize(id, cols, rows).await,
                        None,
                    ),
                };
                if let Err(error) = result {
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.invoke_terminal_failure(
                            key.into(),
                            id.to_string().into(),
                            error.to_string().into(),
                        )
                    });
                } else if let Some(draft) = draft {
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.invoke_terminal_sent(key.into(), id.to_string().into(), draft.into())
                    });
                }
            }
        }));
        let this = Rc::new(Self {
            weak: ui.as_weak(),
            services,
            runtime,
            jobs,
            state: RefCell::new(Sessions {
                columns: 100,
                rows: 24,
                ..Default::default()
            }),
            input,
            packets: Arc::new(Mutex::new(VecDeque::new())),
            wake: Arc::new(AtomicBool::new(false)),
            input_stop,
            resize_timer: slint::Timer::default(),
            pending: RefCell::new(None),
            editor_focused: Cell::new(false),
            edit_revision: Cell::new(0),
        });
        ui.on_terminal_reload({
            let c = this.clone();
            move || c.reload()
        });
        ui.on_terminal_flush({
            let c = this.clone();
            move || c.flush()
        });
        ui.on_terminal_failure({
            let c = this.clone();
            move |key, id, message| {
                let mut s = c.state.borrow_mut();
                if let Some(p) = s.panes.get_mut(key.as_str()) {
                    if p.id.to_string() == id.as_str() {
                        p.sending = false;
                        c.error(message.as_str());
                    }
                }
                drop(s);
                c.render();
            }
        });
        ui.on_terminal_sent({
            let c = this.clone();
            move |key, id, draft| {
                if let Some(p) = c.state.borrow_mut().panes.get_mut(key.as_str()) {
                    if p.id.to_string() == id.as_str() {
                        p.sending = false;
                        p.editor.sent(draft.as_str());
                        if p.draft == draft.as_str() {
                            p.draft.clear();
                            p.editor.cursor = 0;
                        }
                    }
                }
                c.render();
                c.render_editor(true);
            }
        });
        ui.on_terminal_select({
            let c = this.clone();
            move |key| {
                if c.state.borrow().panes.contains_key(key.as_str()) {
                    c.cancel_completion();
                    c.state.borrow_mut().active = key.into();
                    c.render();
                    c.render_editor(true);
                }
            }
        });
        ui.on_terminal_action({
            let c = this.clone();
            move |action| c.action(action.as_str())
        });
        ui.on_terminal_key({
            let c = this.clone();
            move |key, ctrl, alt, shift| {
                let bytes = c
                    .state
                    .borrow()
                    .current()
                    .and_then(|p| key_bytes(&p.terminal, &key, ctrl, alt, shift));
                if let Some(bytes) = bytes {
                    c.send(String::from_utf8(bytes).unwrap(), None);
                    true
                } else {
                    false
                }
            }
        });
        ui.on_terminal_committed({
            let c = this.clone();
            move |value| {
                c.send(value.into(), None);
            }
        });
        ui.on_terminal_draft_edited({
            let c = this.clone();
            move |value, cursor| {
                if let Some(p) = c.state.borrow_mut().current_mut() {
                    p.draft = value.into();
                    p.editor.changed(&p.draft, cursor.max(0) as usize);
                }
                c.render_editor(false);
            }
        });
        ui.on_terminal_editor_focused({
            let c = this.clone();
            move |focused| {
                c.editor_focused.set(focused);
                if !focused {
                    c.cancel_completion();
                }
            }
        });
        ui.on_terminal_cancel_completion({
            let c = this.clone();
            move || c.cancel_completion()
        });
        ui.on_terminal_choose_completion({
            let c = this.clone();
            move |index| c.choose_completion(index.max(0) as usize)
        });
        ui.on_terminal_editor_key({
            let c = this.clone();
            move |key, ctrl, shift, alt, expanded| {
                c.editor_key(key.as_str(), ctrl, shift, alt, expanded)
            }
        });
        ui.on_terminal_completion_result({
            let c = this.clone();
            move |key, id, serial, result, error| {
                if c.state.borrow().active != key.as_str() {
                    return;
                }
                let Some(ui) = c.weak.upgrade() else { return };
                if !c.editor_focused.get() || ui.get_page() != 4 || ui.get_native_hidden() {
                    c.cancel_completion();
                    return;
                }
                let mut s = c.state.borrow_mut();
                let Some(p) = s.current_mut() else { return };
                if p.id.to_string() != id.as_str()
                    || p.editor
                        .pending
                        .as_ref()
                        .is_none_or(|r| r.serial.to_string() != serial.as_str())
                {
                    return;
                }
                let reply = if error.is_empty() {
                    serde_json::from_str::<Reply>(result.as_str()).map_err(|e| e.to_string())
                } else {
                    Err(error.to_string())
                };
                match reply {
                    Ok(reply) if reply.items.len() <= 64 => {
                        let serial = p.editor.pending.as_ref().unwrap().serial;
                        p.editor.complete(serial, &mut p.draft, reply);
                    }
                    Ok(_) => {
                        p.editor.cancel();
                        c.error("补全结果超过上限。");
                    }
                    Err(error) => {
                        p.editor.cancel();
                        c.error(&error);
                    }
                }
                drop(s);
                c.render_editor(true);
            }
        });
        ui.on_terminal_project_edited({
            let c = this.clone();
            move |value| {
                if let Some(p) = c.state.borrow_mut().current_mut() {
                    p.project = value.into();
                }
            }
        });
        ui.on_terminal_send_command({
            let c = this.clone();
            move || {
                if c.state.borrow().current().is_some_and(|p| p.sending) {
                    return;
                }
                let value = c.state.borrow().current().map(|p| {
                    (
                        String::from_utf8(p.terminal.command(&p.draft)).unwrap(),
                        p.draft.clone(),
                    )
                });
                if let Some((value, draft)) = value {
                    if c.send(value, Some(draft)) {
                        if let Some(p) = c.state.borrow_mut().current_mut() {
                            p.sending = true;
                        }
                        c.render();
                    }
                }
            }
        });
        ui.on_terminal_scroll({
            let c = this.clone();
            move |lines| {
                if let Some(p) = c.state.borrow_mut().current_mut() {
                    p.terminal.scroll(lines);
                }
                c.render_grid();
            }
        });
        ui.on_terminal_selection({
            let c = this.clone();
            move |x, y, x2, y2| {
                if let Some(p) = c.state.borrow_mut().current_mut() {
                    p.terminal.select(
                        (x.max(0) as usize, y.max(0) as usize),
                        (x2.max(0) as usize, y2.max(0) as usize),
                    );
                }
                c.render_grid();
            }
        });
        ui.on_terminal_copy({
            let c = this.clone();
            move || {
                let value = c
                    .state
                    .borrow()
                    .current()
                    .and_then(|p| p.terminal.selected_text());
                if let Some(value) = value {
                    if let Err(e) =
                        arboard::Clipboard::new().and_then(|mut clip| clip.set_text(value))
                    {
                        c.error(&e.to_string());
                    }
                }
            }
        });
        ui.on_terminal_paste({
            let c = this.clone();
            move || match arboard::Clipboard::new().and_then(|mut clip| clip.get_text()) {
                Ok(value) => {
                    let value = c
                        .state
                        .borrow()
                        .current()
                        .map(|p| String::from_utf8(p.terminal.paste(&value)).unwrap());
                    if let Some(value) = value {
                        c.send(value, None);
                    }
                }
                Err(e) => c.error(&e.to_string()),
            }
        });
        ui.on_terminal_resize({
            let c = this.clone();
            move |columns, rows| {
                if c.weak
                    .upgrade()
                    .is_none_or(|ui| ui.get_page() != 4 || ui.get_native_hidden())
                {
                    return;
                }
                let mut s = c.state.borrow_mut();
                let (cols, rows) = (columns.clamp(2, 1000) as usize, rows.clamp(1, 500) as usize);
                if (s.columns, s.rows) == (cols, rows) {
                    return;
                }
                s.columns = cols;
                s.rows = rows;
                for pane in s.panes.values_mut() {
                    pane.terminal.resize(cols, rows);
                }
                drop(s);
                c.render_grid();
                let target = Rc::downgrade(&c);
                c.resize_timer.start(
                    slint::TimerMode::SingleShot,
                    std::time::Duration::from_millis(80),
                    move || {
                        let Some(c) = target.upgrade() else { return };
                        let s = c.state.borrow();
                        let (cols, rows) = s.dimensions();
                        for (key, pane) in &s.panes {
                            if pane.phase == "connected" {
                                if let Ok(service) = c.services.terminal.get(key, Some(&s.device)) {
                                    if c.input
                                        .try_send(Input::Resize(
                                            service,
                                            pane.id,
                                            cols as u32,
                                            rows as u32,
                                        ))
                                        .is_err()
                                    {
                                        c.error("终端尺寸队列繁忙，请调整窗口后重试。");
                                    }
                                }
                            }
                        }
                    },
                );
            }
        });
        this.reload();
        this
    }
    pub fn reload(&self) {
        match self.services.terminal.list() {
            Ok(catalog) => {
                let mut s = self.state.borrow_mut();
                s.sync(&catalog);
                for (key, p) in &mut s.panes {
                    if !p.project_loaded {
                        p.project = self.services.logs.terminal_snapshot(key)["project"]
                            .as_str()
                            .unwrap_or_default()
                            .into();
                        p.project_loaded = true;
                    }
                }
                drop(s);
                self.render();
            }
            Err(e) => self.error(&e.to_string()),
        }
    }
    pub fn render(&self) {
        let Some(ui) = self.weak.upgrade() else {
            return;
        };
        let s = self.state.borrow();
        let mut tabs = s
            .panes
            .iter()
            .map(|(key, p)| (key, p.number))
            .collect::<Vec<_>>();
        tabs.sort_by_key(|(_, n)| *n);
        ui.set_terminal_tabs(ModelRc::new(VecModel::from(
            tabs.into_iter()
                .map(|(key, n)| TerminalTab {
                    key: key.as_str().into(),
                    title: format!("{} {n}", text("终端", ui.get_english())).into(),
                })
                .collect::<Vec<_>>(),
        )));
        ui.set_terminal_active(s.active.as_str().into());
        if let Some(p) = s.current() {
            ui.set_terminal_phase(p.phase.as_str().into());
            ui.set_terminal_status(text(&p.message, ui.get_english()).into());
            ui.set_terminal_project(p.project.as_str().into());
            ui.set_terminal_sending(p.sending);
            let logs = self.services.logs.terminal_snapshot(&s.active);
            ui.set_terminal_log_status(
                text(
                    logs["message"].as_str().unwrap_or_default(),
                    ui.get_english(),
                )
                .into(),
            );
        } else {
            ui.set_terminal_phase("disconnected".into());
            ui.set_terminal_status(text("连接设备后可打开终端。", ui.get_english()).into());
            ui.set_terminal_project("".into());
        }
        drop(s);
        self.render_editor(false);
        self.render_grid();
    }
    fn render_editor(&self, apply: bool) {
        let Some(ui) = self.weak.upgrade() else {
            return;
        };
        let (draft, cursor, pending, items, index, truncated) = {
            let s = self.state.borrow();
            if let Some(p) = s.current() {
                let menu = p.editor.menu.as_ref();
                (
                    p.draft.clone(),
                    p.editor.cursor,
                    p.editor.pending.is_some(),
                    menu.map(|m| {
                        m.items
                            .iter()
                            .map(|i| CompletionRow {
                                label: i.label.as_str().into(),
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                    menu.map(|m| m.selected).unwrap_or_default(),
                    menu.is_some_and(|m| m.truncated),
                )
            } else {
                (String::new(), 0, false, vec![], 0, false)
            }
        };
        ui.set_terminal_draft(draft.into());
        ui.set_terminal_command_caret(cursor as i32);
        ui.set_terminal_completions(ModelRc::new(VecModel::from(items)));
        ui.set_terminal_completion_index(index as i32);
        ui.set_terminal_completion_loading(pending);
        ui.set_terminal_completion_truncated(truncated);
        if apply {
            let revision = self.edit_revision.get().wrapping_add(1);
            self.edit_revision.set(revision);
            ui.set_terminal_command_revision(revision);
        }
    }
    fn cancel_completion(&self) {
        for p in self.state.borrow_mut().panes.values_mut() {
            p.editor.cancel();
        }
        self.render_editor(false);
    }
    fn choose_completion(&self, index: usize) {
        if let Some(p) = self.state.borrow_mut().current_mut() {
            p.editor.choose(&mut p.draft, index);
        }
        self.render_editor(true);
    }
    fn editor_key(&self, key: &str, ctrl: bool, shift: bool, alt: bool, expanded: bool) -> bool {
        if alt {
            return false;
        }
        let has_menu = self
            .state
            .borrow()
            .current()
            .is_some_and(|p| p.editor.menu.is_some());
        if has_menu {
            if matches!(key, "\u{f700}" | "\u{f701}" | "\t") && !ctrl {
                if let Some(p) = self.state.borrow_mut().current_mut() {
                    p.editor
                        .select(if key == "\u{f700}" || shift { -1 } else { 1 });
                }
                self.render_editor(false);
                return true;
            }
            if matches!(key, "\r" | "\n") && !ctrl && !shift {
                let index = self
                    .state
                    .borrow()
                    .current()
                    .unwrap()
                    .editor
                    .menu
                    .as_ref()
                    .unwrap()
                    .selected;
                self.choose_completion(index);
                return true;
            }
        }
        if key == "\u{1b}" {
            self.cancel_completion();
            return has_menu;
        }
        if key == "\t" && !shift && !ctrl {
            self.complete_command();
            return true;
        }
        if matches!(key, "\r" | "\n") && (ctrl || (!expanded && !shift)) {
            self.cancel_completion();
            if let Some(ui) = self.weak.upgrade() {
                ui.invoke_terminal_send_command();
            }
            return true;
        }
        if !expanded && !ctrl && !shift && matches!(key, "\u{f700}" | "\u{f701}") {
            let mut s = self.state.borrow_mut();
            let Some(p) = s.current_mut() else {
                return false;
            };
            if p.draft.contains('\n') || p.editor.history.is_empty() {
                return false;
            }
            p.editor.history(&mut p.draft, key == "\u{f700}");
            drop(s);
            self.render_editor(true);
            return true;
        }
        false
    }
    fn complete_command(&self) {
        let mut s = self.state.borrow_mut();
        let (key, device) = (s.active.clone(), s.device.clone());
        let Some(p) = s.current_mut() else { return };
        if p.phase != "connected" || p.sending || p.editor.pending.is_some() {
            return;
        }
        let request = p.editor.begin(&p.draft, p.editor.cursor);
        let id = p.id;
        drop(s);
        self.render_editor(false);
        let service = match self.services.terminal.get(&key, Some(&device)) {
            Ok(service) => service,
            Err(e) => {
                self.cancel_completion();
                self.error(&e.to_string());
                return;
            }
        };
        let weak = self.weak.clone();
        self.job(async move {
            let result = service
                .complete(
                    id,
                    request.context.prefix,
                    request.context.commands,
                    request.context.directories,
                )
                .await;
            let (value, error) = match result {
                Ok(value) => (value.to_string(), String::new()),
                Err(error) => (String::new(), error.to_string()),
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_terminal_completion_result(
                    key.into(),
                    id.to_string().into(),
                    request.serial.to_string().into(),
                    value.into(),
                    error.into(),
                )
            });
        });
    }
    pub fn render_grid(&self) {
        let Some(ui) = self.weak.upgrade() else {
            return;
        };
        if ui.get_page() != 4 || ui.get_native_hidden() {
            return;
        }
        let s = self.state.borrow();
        let cells = s
            .current()
            .map(|p| p.terminal.cells())
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c.text != " " || c.background != [24, 28, 36])
            .map(|c| TerminalCell {
                column: c.column as i32,
                row: c.row as i32,
                text: c.text.into(),
                wide: c.wide,
                bold: c.bold,
                underline: c.underline,
                ink: slint::Color::from_rgb_u8(c.foreground[0], c.foreground[1], c.foreground[2]),
                paper: slint::Color::from_rgb_u8(c.background[0], c.background[1], c.background[2]),
            })
            .collect::<Vec<_>>();
        ui.set_terminal_cells(ModelRc::new(VecModel::from(cells)));
        let (column, row) = s.current().map(|p| p.terminal.cursor()).unwrap_or_default();
        ui.set_terminal_cursor_column(column as i32);
        ui.set_terminal_cursor_row(row as i32);
        ui.set_terminal_cursor_visible(s.current().is_some_and(|p| p.terminal.cursor_visible()));
    }
    fn error(&self, message: &str) {
        if let Some(ui) = self.weak.upgrade() {
            ui.set_terminal_error(text(message, ui.get_english()).into());
        }
    }
    fn send(&self, value: String, draft: Option<String>) -> bool {
        let s = self.state.borrow();
        let Some(p) = s.current() else { return false };
        if p.phase != "connected" {
            return false;
        }
        let result = (|| {
            anyhow::ensure!(value.len() <= 65536, "单次输入过长，请分段粘贴。");
            let service = self.services.terminal.get(&s.active, Some(&s.device))?;
            self.input
                .try_send(Input::Text(service, p.id, value, draft))
                .context("终端输入队列繁忙，请重试。")?;
            Ok::<_, anyhow::Error>(())
        })();
        match result {
            Ok(()) => true,
            Err(e) => {
                self.error(&e.to_string());
                false
            }
        }
    }
    fn action(&self, action: &str) {
        let Some(ui) = self.weak.upgrade() else {
            return;
        };
        if ui.get_dialog_kind() != 0 || ui.get_terminal_busy() {
            return;
        }
        ui.set_terminal_error("".into());
        if action == "clear" {
            if let Some(p) = self.state.borrow_mut().current_mut() {
                p.terminal.clear();
            }
            self.render_grid();
            return;
        }
        let s = self.state.borrow();
        let (key, device) = (s.active.clone(), s.device.clone());
        let (cols, rows) = s.dimensions();
        drop(s);
        if matches!(action, "reset" | "remove") {
            *self.pending.borrow_mut() = Some((key, device, action == "remove"));
            ui.set_dialog_title(
                if action == "remove" {
                    "关闭终端"
                } else {
                    "重启终端"
                }
                .into(),
            );
            ui.set_dialog_message(
                text(
                    if action == "remove" {
                        "关闭此终端会结束其中的Shell和前台程序，是否继续？"
                    } else {
                        "重启会结束此终端中的Shell和前台程序，是否继续？"
                    },
                    ui.get_english(),
                )
                .into(),
            );
            ui.set_dialog_kind(if action == "remove" { 5 } else { 4 });
            return;
        }
        if action == "create" {
            let result = self.services.terminal.create(Some(&device));
            match result {
                Ok(value) => {
                    self.reload();
                    self.state.borrow_mut().active =
                        value["key"].as_str().unwrap_or_default().into();
                    self.render();
                }
                Err(e) => self.error(&e.to_string()),
            }
            return;
        }
        let service = match self.services.terminal.get(&key, Some(&device)) {
            Ok(s) => s,
            Err(e) => {
                self.error(&e.to_string());
                return;
            }
        };
        if action == "stop" {
            service.stop();
            self.reload();
            return;
        }
        let weak = self.weak.clone();
        if action == "project" {
            let project = self
                .state
                .borrow()
                .current()
                .map(|p| p.project.clone())
                .unwrap_or_default();
            let services = self.services.clone();
            self.job(async move {
                let result = services
                    .logs
                    .set_project_for_device(&key, project, Some(&device))
                    .await;
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    if let Err(e) = result {
                        ui.set_terminal_error(e.to_string().into());
                    }
                    ui.invoke_terminal_reload();
                });
            });
            return;
        }
        if action != "start" {
            return;
        }
        let packets = self.packets.clone();
        let wake = self.wake.clone();
        let sink_ui = weak.clone();
        let ack = service.clone();
        let sink = Arc::new(move |frame: Vec<u8>| {
            anyhow::ensure!(frame.len() >= 16, "终端数据帧无效。");
            let id = u64::from_le_bytes(frame[..8].try_into()?);
            let sequence = u64::from_le_bytes(frame[8..16].try_into()?);
            packets.lock().unwrap().push_back(Packet {
                service: ack.clone(),
                device: device.clone(),
                key: key.clone(),
                id,
                sequence,
                bytes: frame[16..].to_vec(),
            });
            if !wake.swap(true, Ordering::SeqCst) {
                sink_ui
                    .upgrade_in_event_loop(|ui| ui.invoke_terminal_flush())
                    .map_err(|_| anyhow::anyhow!("原生终端窗口已关闭。"))?;
            }
            Ok(())
        });
        self.job(async move {
            let result = service.start(cols as u32, rows as u32, sink).await;
            let _ = weak.upgrade_in_event_loop(move |ui| {
                if let Err(e) = result {
                    ui.set_terminal_error(e.to_string().into());
                }
                ui.invoke_terminal_reload();
            });
        });
    }
    fn flush(&self) {
        // Clear the wake while holding the queue lock: a producer after this drain schedules a new wake.
        let packets = {
            let mut queue = self.packets.lock().unwrap();
            let packets = queue.drain(..).collect::<Vec<_>>();
            self.wake.store(false, Ordering::SeqCst);
            packets
        };
        if let Ok(catalog) = self.services.terminal.list() {
            self.state.borrow_mut().sync(&catalog);
        }
        for packet in packets {
            let reply =
                self.state
                    .borrow_mut()
                    .feed(&packet.device, &packet.key, packet.id, &packet.bytes);
            packet.service.ack(packet.id, packet.sequence);
            if let Some(bytes) = reply.filter(|b| !b.is_empty()) {
                if self
                    .input
                    .try_send(Input::Text(
                        packet.service,
                        packet.id,
                        String::from_utf8(bytes).unwrap(),
                        None,
                    ))
                    .is_err()
                {
                    self.error("终端输入队列繁忙，请重试。");
                }
            }
        }
        self.render_grid();
    }
    pub fn respond(&self, accepted: bool) {
        let Some((key, device, remove)) = self.pending.borrow_mut().take() else {
            return;
        };
        if !accepted {
            return;
        }
        let Some(ui) = self.weak.upgrade() else {
            return;
        };
        ui.set_terminal_busy(true);
        let services = self.services.clone();
        let weak = self.weak.clone();
        self.job(async move {
            let result = async {
                if remove {
                    services.terminal.remove(&key, Some(&device)).await?;
                } else {
                    services
                        .terminal
                        .get(&key, Some(&device))?
                        .reset(false)
                        .await?;
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_terminal_busy(false);
                if let Err(e) = result {
                    ui.set_terminal_error(e.to_string().into());
                }
                ui.invoke_terminal_reload();
            });
        });
    }
    pub fn shutdown(&self) {
        self.resize_timer.stop();
        self.input_stop.cancel();
        for packet in self.packets.lock().unwrap().drain(..) {
            packet.service.ack(packet.id, packet.sequence);
        }
    }
}

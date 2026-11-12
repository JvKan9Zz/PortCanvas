use crate::{
    workbench::text,
    workbench_ui::{FileRow, Workbench},
};
use portcanvas_core::{
    files::{Confirmation, Side},
    services::Services,
};
use portcanvas_native_probe::file_browser::{bytes, hit_row, Panel};
use serde_json::{json, Value};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
    path::PathBuf,
    time::{Duration, Instant},
};
use slint::winit_030::{winit::event::{ElementState, MouseButton, WindowEvent}, WinitWindowAccessor};
use tokio::{runtime::Runtime, sync::oneshot, task::JoinHandle};
use tokio_util::sync::CancellationToken;
type Pending = Arc<Mutex<Option<(u64, oneshot::Sender<bool>)>>>;
struct Drag {
    side: usize,
    start: (f32, f32),
    path: String,
    generation: u64,
    names: Vec<String>,
    active: bool,
}
#[derive(Clone)]
struct Destination { side: usize, row: Option<usize>, path: String, generation: u64 }
pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    panels: RefCell<[Panel; 2]>,
    models: [Rc<VecModel<FileRow>>; 2],
    generation: Cell<u64>,
    operation: Cell<u64>,
    mutating: Cell<bool>,
    cancel: RefCell<CancellationToken>,
    pending: Pending,
    reads: Cell<u64>,
    drives: RefCell<Vec<String>>,
    position: Cell<(f32, f32)>,
    drag: RefCell<Option<Drag>>,
    suppress_click: Cell<Instant>,
    hover_timer: slint::Timer,
    drop_timer: slint::Timer,
    dropped: RefCell<Vec<PathBuf>>,
    destination: RefCell<Option<Destination>>,
    drops: Cell<u64>,
}
fn side(index: usize) -> Side {
    if index == 0 {
        Side::Local
    } else {
        Side::Remote
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
            panels: RefCell::new(Default::default()),
            models: [Rc::new(VecModel::default()), Rc::new(VecModel::default())],
            generation: Cell::new(u64::MAX),
            operation: Cell::new(0),
            mutating: Cell::new(false),
            cancel: RefCell::new(CancellationToken::new()),
            pending: Arc::new(Mutex::new(None)),
            reads: Cell::new(0),
            drives: RefCell::new(Vec::new()),
            position: Cell::new((0.,0.)),
            drag: RefCell::new(None),
            suppress_click: Cell::new(Instant::now()),
            hover_timer: slint::Timer::default(),
            drop_timer: slint::Timer::default(),
            dropped: RefCell::new(Vec::new()),
            destination: RefCell::new(None),
            drops: Cell::new(0),
        });
        if let Ok(drives) = portcanvas_core::files::local_drives() {
            *c.drives.borrow_mut()=drives.as_array().into_iter().flatten().filter_map(|d|d["path"].as_str().map(str::to_owned)).collect();
            ui.set_file_drives(ModelRc::new(VecModel::from(c.drives.borrow().iter().map(|s|slint::SharedString::from(s.as_str())).collect::<Vec<_>>())));
        }
        ui.on_files_drive({let c=c.clone();move|index|{
            if !c.locked() {let path=c.drives.borrow().get(index as usize).cloned();if let Some(path)=path{c.load(0,path);}}
        }});
        ui.set_file_local_rows(ModelRc::from(c.models[0].clone()));
        ui.set_file_remote_rows(ModelRc::from(c.models[1].clone()));
        ui.on_files_sync({
            let c = c.clone();
            move || c.sync()
        });
        ui.on_files_state_changed({
            let c = c.clone();
            move || c.render()
        });
        ui.on_files_load_result({
            let c = c.clone();
            move |index, ticket, generation, packet| {
                let index = index as usize;
                if index > 1 {
                    return;
                }
                let (Ok(ticket), Ok(generation), Ok(packet)) = (
                    ticket.parse::<u64>(),
                    generation.parse::<u64>(),
                    serde_json::from_str::<Value>(&packet),
                ) else {
                    return;
                };
                if index == 1 && generation != c.services.devices.generation() {
                    return;
                }
                let result = if let Some(error) = packet["error"].as_str() {
                    Err(anyhow::anyhow!(error.to_owned()))
                } else {
                    Ok(packet)
                };
                let mut panels = c.panels.borrow_mut();
                let panel = &mut panels[index];
                if panel.receive(ticket, result) && panel.error.is_empty() {
                    if let Some(ui) = c.ui.upgrade() {
                        if index == 0 {
                            ui.set_file_local_path(panel.path.clone().into());
                        } else {
                            ui.set_file_remote_path(panel.path.clone().into());
                        }
                    }
                }
                drop(panels);
                c.render();
            }
        });
        ui.on_files_choose({
            let c = c.clone();
            move |index, row, add, range| {
                if c.locked() || Instant::now()<c.suppress_click.get() {
                    return;
                }
                let index = index as usize;
                if index < 2 {
                    c.panels.borrow_mut()[index].choose(row.max(0) as usize, add, range);
                    c.render();
                }
            }
        });
        ui.on_files_all({
            let c = c.clone();
            move |index, checked| {
                if c.locked() {
                    return;
                }
                let index = index as usize;
                if index < 2 {
                    c.panels.borrow_mut()[index].all(checked);
                    c.render();
                }
            }
        });
        ui.on_files_open({
            let c = c.clone();
            move |index, row| {
                let index = index as usize;
                if index > 1 || c.locked() || Instant::now()<c.suppress_click.get() {
                    return;
                }
                let path = c.panels.borrow()[index].folder(row.max(0) as usize, index == 1);
                if let Some(path) = path {
                    c.load(index, path);
                }
            }
        });
        ui.on_files_action({
            let c = c.clone();
            move |index, action| c.action(index as usize, action.as_str())
        });
        ui.on_files_confirm({
            let c = c.clone();
            move |epoch, generation, deleting, paths| {
                let Some(ui) = c.ui.upgrade() else { return };
                let Ok(epoch) = epoch.parse::<u64>() else {
                    return;
                };
                let accepted = epoch == c.operation.get()
                    && generation.parse::<u64>().ok() == Some(c.services.devices.generation())
                    && c.mutating.get()
                    && !c.cancel.borrow().is_cancelled()
                    && ui.get_dialog_kind() == 0;
                if !accepted {
                    c.respond_for(epoch, false);
                    return;
                }
                let Ok(paths) = serde_json::from_str::<Vec<String>>(&paths) else {
                    c.respond_for(epoch, false);
                    return;
                };
                let title = if deleting {
                    "确认删除远端项目"
                } else {
                    "确认覆盖文件"
                };
                let intro = if deleting {
                    format!("删除所选 {} 个远端项目及其内容？", paths.len())
                } else {
                    format!("目标已有 {} 个同名文件，是否覆盖？", paths.len())
                };
                let mut message = text(&intro, ui.get_english());
                for path in paths.iter().take(12) {
                    message.push('\n');
                    message.push_str(path);
                }
                if paths.len() > 12 {
                    message.push_str(&format!("\n… (+{})", paths.len() - 12));
                }
                ui.set_dialog_title(title.into());
                ui.set_dialog_message(message.into());
                ui.set_dialog_kind(if deleting { 7 } else { 6 });
            }
        });
        ui.on_files_finished({
            let c = c.clone();
            move |epoch, error| {
                if epoch.parse::<u64>().ok() != Some(c.operation.get()) {
                    return;
                }
                c.mutating.set(false);
                c.respond(false);
                if let Some(ui) = c.ui.upgrade() {
                    if matches!(ui.get_dialog_kind(), 6 | 7) {
                        ui.set_dialog_kind(0);
                    }
                    if ui.get_dialog_kind() == 15 && error.is_empty() {
                        ui.set_dialog_kind(0);
                    }
                    if !error.is_empty() {
                        ui.set_file_error(text(&error, ui.get_english()).into());
                    }
                }
                for p in c.panels.borrow_mut().iter_mut() {
                    p.loaded = false;
                    p.error.clear();
                }
                c.sync();
            }
        });
        c.sync();
        c
    }
    fn visible(&self) -> bool {
        self.ui
            .upgrade()
            .is_some_and(|u| u.get_page() == 6 && !u.get_native_hidden())
    }
    fn locked(&self) -> bool {
        self.mutating.get()
            || self.services.files.snapshot()["busy"] == true
            || self.panels.borrow().iter().any(|p| p.loading)
            || self.ui.upgrade().is_none_or(|u| u.get_dialog_kind() != 0)
    }
    fn spawn(&self, future: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|j| !j.is_finished());
        jobs.push(self.runtime.spawn(future));
    }
    pub fn sync(self: &Rc<Self>) {
        if !self.visible() { self.clear_drag(); }
        let generation = self.services.devices.generation();
        if self.generation.replace(generation) != generation {
            self.clear_drag();
            self.panels.borrow_mut()[1].reset();
            self.respond(false);
            if let Some(ui) = self.ui.upgrade() {
                ui.set_file_remote_path("".into());
                if matches!(ui.get_dialog_kind(), 6 | 7) {
                    ui.set_dialog_kind(0);
                }
                if ui.get_dialog_kind() == 15 && ui.get_file_folder_side() == 1 {
                    ui.set_dialog_kind(0);
                }
            }
        }
        self.render();
        if !self.visible() {
            return;
        }
        for index in 0..2 {
            let need = {
                let panels = self.panels.borrow();
                let p = &panels[index];
                !p.loaded && !p.loading && p.error.is_empty()
            };
            if need && (index == 0 || self.services.devices.snapshot()["ssh"] == "connected") {
                let path = self.panels.borrow()[index].path.clone();
                self.load(index, path);
            }
        }
    }
    fn load(self: &Rc<Self>, index: usize, path: String) {
        if index > 1 || self.mutating.get() || self.services.files.snapshot()["busy"] == true {
            return;
        }
        if index == 1 && self.services.devices.snapshot()["ssh"] != "connected" {
            return;
        }
        let ticket = self.panels.borrow_mut()[index].begin();
        self.reads.set(self.reads.get() + 1);
        self.render();
        let services = self.services.clone();
        let weak = self.ui.clone();
        let generation = services.devices.generation();
        self.spawn(async move {
            let result = if index == 1 && generation != services.devices.generation() {
                Err(anyhow::anyhow!("设备连接已变化，请刷新。"))
            } else {
                services.files.list(side(index), path).await
            };
            let value = result.unwrap_or_else(|e| json!({"error":e.to_string()}));
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_files_load_result(
                    index as i32,
                    ticket.to_string().into(),
                    generation.to_string().into(),
                    value.to_string().into(),
                )
            });
        });
    }
    fn action(self: &Rc<Self>, index: usize, action: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        if action == "cancel" {
            self.cancel.borrow().cancel();
            self.services.files.cancel();
            self.respond(false);
            return;
        }
        if action == "connect" {
            ui.set_page(0);
            return;
        }
        if index > 1 || self.locked() {
            return;
        }
        match action {
            "new-folder" => {
                if !self.panels.borrow()[index].loaded { return; }
                ui.set_file_folder_side(index as i32);
                ui.set_file_folder_name("".into());
                ui.set_file_error("".into());
                ui.set_dialog_title("新建文件夹".into());
                ui.set_dialog_message(self.panels.borrow()[index].path.clone().into());
                ui.set_dialog_kind(15);
            }
            "go" => self.load(
                index,
                if index == 0 {
                    ui.get_file_local_path().into()
                } else {
                    ui.get_file_remote_path().into()
                },
            ),
            "refresh" | "up" | "home" => {
                let path = {
                    let p = &self.panels.borrow()[index];
                    match action {
                        "up" => p.parent.clone(),
                        "home" => p.home.clone(),
                        _ => p.path.clone(),
                    }
                };
                self.load(index, path);
            }
            "choose" => {
                let weak = self.ui.clone();
                let initial = self.panels.borrow()[0].path.clone();
                self.spawn(async move {
                    let path = rfd::AsyncFileDialog::new()
                        .set_directory(initial)
                        .pick_folder()
                        .await;
                    if let Some(path) = path {
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.set_file_local_path(path.path().to_string_lossy().as_ref().into());
                            ui.invoke_files_action(0, "go".into());
                        });
                    }
                });
            }
            "mkdir" | "upload" | "download" | "delete" => self.mutate(index, action),
            _ => {}
        }
    }
    pub fn respond_folder(self: &Rc<Self>, accepted: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_dialog_kind() != 15 || self.mutating.get() { return; }
        if !accepted { ui.set_dialog_kind(0); return; }
        let index = ui.get_file_folder_side().clamp(0, 1) as usize;
        if !self.panels.borrow()[index].loaded { return; }
        let name = ui.get_file_folder_name();
        if name.trim().is_empty() { return; }
        if index == 0 { ui.set_file_local_folder(name); }
        else { ui.set_file_remote_folder(name); }
        self.mutate(index, "mkdir");
    }
    fn mutate(self: &Rc<Self>, index: usize, action: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = self.services.devices.generation();
        let deleting = action == "delete";
        let panels = self.panels.borrow();
        let directory = panels[index].path.clone();
        let name = if index == 0 {
            ui.get_file_local_folder().to_string()
        } else {
            ui.get_file_remote_folder().to_string()
        };
        let names = panels[if action == "upload" { 0 } else { 1 }]
            .selected
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        if action != "mkdir"
            && (!panels[1].loaded || names.is_empty() || (!deleting && !panels[0].loaded))
        {
            return;
        }
        if (action != "mkdir" || index == 1)
            && (!ui.get_connected() || panels[1].generation != generation)
        {
            return;
        }
        let rate = [0, 1048576, 5242880, 10485760][ui.get_file_rate().clamp(0, 3) as usize];
        let options = json!({"generation":generation,"direction":action,"names":names,"localPath":panels[0].path,"remotePath":panels[1].path,"rateLimit":rate});
        drop(panels);
        self.start_mutation(index, action, directory, name, options);
    }
    fn start_mutation(self: &Rc<Self>, index: usize, action: &str, directory: String, name: String, options: Value) {
        let Some(ui)=self.ui.upgrade() else{return};
        let generation=options["generation"].as_u64().unwrap();
        let deleting=action=="delete";
        self.operation.set(self.operation.get() + 1);
        let epoch = self.operation.get();
        let cancel = CancellationToken::new();
        *self.cancel.borrow_mut() = cancel.clone();
        self.mutating.set(true);
        ui.set_file_error("".into());
        self.render();
        let services = self.services.clone();
        let pending = self.pending.clone();
        let weak = self.ui.clone();
        let confirm_ui = weak.clone();
        let closing = cancel.clone();
        let connection_cancel = services
            .devices
            .connection_with_generation()
            .ok()
            .map(|(_, c)| c.cancel.clone());
        let confirm: Confirmation = Arc::new(move |paths| {
            let (sender, receiver) = oneshot::channel();
            *pending.lock().unwrap() = Some((epoch, sender));
            let weak = confirm_ui.clone();
            let closing = closing.clone();
            let connection = connection_cancel.clone();
            Box::pin(async move {
                if weak
                    .upgrade_in_event_loop(move |ui| {
                        ui.invoke_files_confirm(
                            epoch.to_string().into(),
                            generation.to_string().into(),
                            deleting,
                            serde_json::to_string(&paths).unwrap().into(),
                        )
                    })
                    .is_err()
                {
                    return Ok(false);
                }
                let disconnected = async {
                    if let Some(c) = connection {
                        c.cancelled().await
                    } else {
                        std::future::pending::<()>().await
                    }
                };
                tokio::select! {biased;_=closing.cancelled()=>Ok(false),_=disconnected=>Ok(false),result=receiver=>Ok(result.unwrap_or(false))}
            })
        });
        let action = action.to_owned();
        self.spawn(async move{
            let result=async{anyhow::ensure!(!cancel.is_cancelled(),"操作已取消。");if action=="mkdir"{let path=services.files.mkdir_for_generation(side(index),directory,name,Some(generation)).await?;Ok(json!({"path":path}))}
                else if deleting{services.files.remove_remote(serde_json::from_value(json!({"parent":directory,"names":options["names"],"generation":generation}))?,confirm).await}
                else{services.files.transfer(serde_json::from_value(options)?,confirm).await}}.await;
            let error=result.err().map(|e:anyhow::Error|e.to_string()).unwrap_or_default();let _=weak.upgrade_in_event_loop(move|ui|{if action=="mkdir"&&error.is_empty(){if index==0{ui.set_file_local_folder("".into());}else{ui.set_file_remote_folder("".into());}}ui.invoke_files_finished(epoch.to_string().into(),error.into());});
        });
    }
    pub fn respond(&self, accepted: bool) {
        self.respond_for(self.operation.get(), accepted);
    }
    fn respond_for(&self, epoch: u64, accepted: bool) {
        let target = {
            let mut pending = self.pending.lock().unwrap();
            if pending.as_ref().is_some_and(|(id, _)| *id == epoch) {
                pending.take()
            } else {
                None
            }
        };
        if let Some((_, sender)) = target {
            let _ = sender.send(accepted);
        }
    }
    fn render(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let state = self.services.files.snapshot();
        let busy = self.mutating.get() || state["busy"] == true;
        ui.set_file_mutating(busy);
        if !self.visible() {
            return;
        }
        let english = ui.get_english();
        for index in 0..2 {
            let panels = self.panels.borrow();
            let p = &panels[index];
            let rows = p
                .entries
                .iter()
                .map(|e| FileRow {
                    name: e.name.clone().into(),
                    kind: text(
                        match e.kind.as_str() {
                            "directory" => "文件夹",
                            "link" => "链接",
                            _ => "文件",
                        },
                        english,
                    )
                    .into(),
                    size: e.size.map(bytes).unwrap_or_default().into(),
                    modified: e
                        .modified
                        .and_then(|n| chrono::DateTime::from_timestamp_millis(n as i64))
                        .map(|t| {
                            t.with_timezone(&chrono::Local)
                                .format("%m-%d %H:%M")
                                .to_string()
                        })
                        .unwrap_or_default()
                        .into(),
                    selected: p.selected.contains(&e.name),
                    directory: e.kind == "directory",
                })
                .collect::<Vec<_>>();
            let model = &self.models[index];
            if model.row_count() != rows.len() {
                model.set_vec(rows);
            } else {
                for (n, row) in rows.into_iter().enumerate() {
                    if model.row_data(n).as_ref() != Some(&row) {
                        model.set_row_data(n, row);
                    }
                }
            }
            let message = if !p.error.is_empty() {
                text(&p.error, english)
            } else if p.loading {
                text("正在读取目录…", english)
            } else if index == 1 && !ui.get_connected() {
                text("设备未连接", english)
            } else {
                text(
                    &format!("{} 个项目 · 已选 {} 个", p.entries.len(), p.selected.len()),
                    english,
                )
            };
            if index == 0 {
                ui.set_file_drive_index(self.drives.borrow().iter().position(|d|p.path.to_lowercase().starts_with(&d.to_lowercase())).map(|n|n as i32).unwrap_or(-1));
                ui.set_file_local_message(message.into());
                ui.set_file_local_loading(p.loading);
                ui.set_file_local_loaded(p.loaded);
                ui.set_file_local_selected(p.selected.len() as i32);
            } else {
                ui.set_file_remote_message(message.into());
                ui.set_file_remote_loading(p.loading);
                ui.set_file_remote_loaded(p.loaded);
                ui.set_file_remote_selected(p.selected.len() as i32);
            }
        }
        let phase = state["phase"].as_str().unwrap_or("idle");
        let message = state["message"].as_str().unwrap_or_default();
        ui.set_file_status(
            if phase == "idle" {
                String::new()
            } else {
                text(message, english)
            }
            .into(),
        );
        ui.set_file_current(state["currentFile"].as_str().unwrap_or_default().into());
        let done = state["bytes"].as_u64().unwrap_or(0);
        let total = state["totalBytes"].as_u64().unwrap_or(0);
        let deleting = state["operation"] == "delete";
        let n = state["completed"].as_u64().unwrap_or(0);
        let count = state["totalFiles"].as_u64().unwrap_or(0);
        ui.set_file_numbers(
            if deleting {
                format!("{n}/{count}")
            } else if phase == "idle" {
                String::new()
            } else {
                format!(
                    "{} / {} · {n}/{count} · {}/s",
                    bytes(done),
                    bytes(total),
                    bytes(state["speed"].as_f64().unwrap_or(0.) as u64)
                )
            }
            .into(),
        );
        ui.set_file_progress(if phase == "completed" {
            1.
        } else if deleting && count > 0 {
            n as f32 / count as f32
        } else if total > 0 {
            done as f32 / total as f32
        } else {
            0.
        });
    }
    pub fn shutdown(&self) {
        self.clear_drag();
        self.cancel.borrow().cancel();
        self.respond(false);
        self.services.files.cancel();
    }
    pub fn report(&self) -> Value {
        json!({"listRequests":self.reads.get(),"operations":self.operation.get(),"drops":self.drops.get(),"busy":self.services.files.snapshot()["busy"]})
    }
    fn available(&self) -> bool {
        self.visible() && !self.locked() && self.services.devices.snapshot()["ssh"]=="connected"
            && self.panels.borrow().iter().all(|p|p.loaded)
            && self.panels.borrow()[1].generation==self.services.devices.generation()
    }
    fn target(&self, x: f32, y: f32, wanted: usize) -> Option<Destination> {
        if !self.available() {return None;}
        let ui=self.ui.upgrade()?;
        let area=if wanted==0{ui.get_file_local_area()}else{ui.get_file_remote_area()};
        let panels=self.panels.borrow();let p=&panels[wanted];
        let row=hit_row(x,y,[area.x,area.y,area.width,area.height,area.offset],p.entries.len())?;
        let folder=row.and_then(|r|p.folder(r,wanted==1));
        Some(Destination {side:wanted,row:if folder.is_some(){row}else{None},path:folder.unwrap_or_else(||p.path.clone()),generation:self.services.devices.generation()})
    }
    fn highlight(&self, target: Option<&Destination>) {
        if let Some(ui)=self.ui.upgrade(){
            ui.set_file_drop_side(target.map(|t|t.side as i32).unwrap_or(-1));
            ui.set_file_drop_row(target.and_then(|t|t.row).map(|n|n as i32).unwrap_or(-1));
            ui.set_file_drop_label(target.map(|t|text(if t.side==1{"松开以上传"}else{"松开以下载"},ui.get_english())).unwrap_or_default().into());
        }
    }
    fn clear_drag(&self) {
        self.drag.borrow_mut().take();self.hover_timer.stop();self.drop_timer.stop();
        self.dropped.borrow_mut().clear();self.destination.borrow_mut().take();self.highlight(None);
    }
    fn system_position(&self) -> Option<(f32,f32)> {
        let ui=self.ui.upgrade()?;
        ui.window().with_winit_window(|window|{
            let mut point=windows::Win32::Foundation::POINT::default();
            unsafe {windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point).ok()?;}
            let origin=window.inner_position().ok()?;let scale=window.scale_factor() as f32;
            Some(((point.x-origin.x) as f32/scale,(point.y-origin.y) as f32/scale))
        }).flatten()
    }
    fn transfer_drop(self: &Rc<Self>, target: Destination, source: Option<Drag>, paths: Vec<PathBuf>) {
        if !self.available() || target.generation!=self.services.devices.generation(){return;}
        let panels=self.panels.borrow();
        let rate=self.ui.upgrade().map(|u|[0,1048576,5242880,10485760][u.get_file_rate().clamp(0,3) as usize]).unwrap_or(0);
        let options=if let Some(source)=source {
            if source.side==target.side || source.generation!=target.generation || panels[source.side].path!=source.path{return;}
            json!({"generation":target.generation,"direction":if source.side==0{"upload"}else{"download"},"names":source.names,
                "localPath":if source.side==0{source.path.clone()}else{target.path.clone()},"remotePath":if source.side==1{source.path}else{target.path},"rateLimit":rate})
        }else{
            if paths.is_empty()||target.side!=1{return;}
            json!({"generation":target.generation,"direction":"upload","names":[],"localItems":paths,"remotePath":target.path,"rateLimit":rate})
        };
        drop(panels);self.drops.set(self.drops.get()+1);
        let action=options["direction"].as_str().unwrap().to_owned();
        self.start_mutation(0,&action,String::new(),String::new(),options);
    }
    pub fn window_event(self: &Rc<Self>, window: &slint::Window, event: &WindowEvent) {
        if self.visible()&&matches!(event,WindowEvent::CursorMoved{..}|WindowEvent::MouseInput{..}|WindowEvent::HoveredFile(_)|WindowEvent::DroppedFile(_)|WindowEvent::Resized(_)|WindowEvent::ScaleFactorChanged{..}){if let Some(ui)=self.ui.upgrade(){ui.set_file_geometry_ticket(ui.get_file_geometry_ticket().wrapping_add(1));}}
        match event {
            WindowEvent::CursorMoved{position,..}=>{
                let scale=window.scale_factor();let point=(position.x as f32/scale,position.y as f32/scale);self.position.set(point);
                let mut drag=self.drag.borrow_mut();
                if let Some(d)=drag.as_mut(){
                    if (point.0-d.start.0).hypot(point.1-d.start.1)>=7.{d.active=true;}
                    if d.active{self.highlight(self.target(point.0,point.1,1-d.side).as_ref());}
                }
            }
            WindowEvent::MouseInput{state,button:MouseButton::Left,..}=>{
                let (x,y)=self.position.get();
                if *state==ElementState::Pressed && self.available(){
                    for index in 0..2 {
                        let ui=self.ui.upgrade().unwrap();let a=if index==0{ui.get_file_local_area()}else{ui.get_file_remote_area()};
                        let panels=self.panels.borrow();let p=&panels[index];
                        if x<a.x+36. || x>=a.x+a.width-16.{continue;}
                        if let Some(Some(row))=hit_row(x,y,[a.x,a.y,a.width,a.height,a.offset],p.entries.len()){
                            *self.drag.borrow_mut()=Some(Drag{side:index,start:(x,y),path:p.path.clone(),generation:self.services.devices.generation(),names:p.drag_names(row),active:false});break;
                        }
                    }
                }else if *state==ElementState::Released{
                    let source=self.drag.borrow_mut().take();
                    if let Some(d)=source.filter(|d|d.active){
                        self.suppress_click.set(Instant::now()+Duration::from_millis(250));
                        let target=self.target(x,y,1-d.side);self.highlight(None);
                        if let Some(target)=target{self.transfer_drop(target,Some(d),Vec::new());}
                    }
                }
            }
            WindowEvent::HoveredFile(_)=>{
                let weak=Rc::downgrade(self);
                self.hover_timer.start(slint::TimerMode::Repeated,Duration::from_millis(30),move||{
                    if let Some(c)=weak.upgrade(){let target=c.system_position().and_then(|(x,y)|c.target(x,y,1));c.highlight(target.as_ref());}
                });
            }
            WindowEvent::DroppedFile(path)=>{
                self.hover_timer.stop();
                if self.dropped.borrow().is_empty(){*self.destination.borrow_mut()=self.system_position().and_then(|(x,y)|self.target(x,y,1));}
                self.dropped.borrow_mut().push(path.clone());
                let weak=Rc::downgrade(self);
                self.drop_timer.start(slint::TimerMode::SingleShot,Duration::from_millis(40),move||{
                    if let Some(c)=weak.upgrade(){let paths=c.dropped.take();let target=c.destination.take();c.highlight(None);if let Some(target)=target{c.transfer_drop(target,None,paths);}}
                });
            }
            WindowEvent::HoveredFileCancelled|WindowEvent::Focused(false)=>self.clear_drag(),
            WindowEvent::CursorLeft{..} if self.dropped.borrow().is_empty()=>self.clear_drag(),
            WindowEvent::KeyboardInput{event,..} if event.logical_key==slint::winit_030::winit::keyboard::Key::Named(slint::winit_030::winit::keyboard::NamedKey::Escape)=>self.clear_drag(),
            _=>{}
        }
    }
}

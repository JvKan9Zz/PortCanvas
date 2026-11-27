use crate::{
    workbench::{fill, preferences, profiles, text},
    workbench_background, workbench_glass,
    workbench_logs,
    workbench_ui::Workbench,
};
use anyhow::{Context, Result};
use portcanvas_core::{
    migration::{bundle, MigrationSession, Plan},
    services::Services,
};
use portcanvas_native_probe::debug_log;
use slint::{winit_030::WinitWindowAccessor, ComponentHandle};
use std::{
    cell::Cell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};
use tokio::{runtime::Runtime, sync::oneshot, task::JoinHandle};
use tokio_util::sync::CancellationToken;
pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    logs: workbench_logs::Controller,
    legacy: PathBuf,
    pending: Arc<Mutex<Option<oneshot::Sender<bool>>>>,
    stop: CancellationToken,
    serial: Cell<u64>,
    completed: Cell<u64>,
    glass: Rc<workbench_glass::Controller>,
    background: Rc<workbench_background::Controller>,
}
impl Controller {
    pub fn install(
        ui: &Workbench,
        services: Arc<Services>,
        runtime: Arc<Runtime>,
        jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
        logs: workbench_logs::Controller,
        legacy: PathBuf,
        glass: Rc<workbench_glass::Controller>,
        background: Rc<workbench_background::Controller>,
    ) -> Rc<Self> {
        let c = Rc::new(Self {
            ui: ui.as_weak(),
            services,
            runtime,
            jobs,
            logs,
            legacy,
            pending: Arc::new(Mutex::new(None)),
            stop: CancellationToken::new(),
            serial: Cell::new(0),
            completed: Cell::new(0),
            glass,
            background,
        });
        ui.on_migration_action({
            let c = c.clone();
            move |export| c.choose(export)
        });
        ui.on_migration_confirm({
            let c = c.clone();
            move |ticket, title, message, restore| {
                if ticket.parse::<u64>().ok() != Some(c.serial.get()) {
                    return;
                }
                let Some(ui) = c.ui.upgrade() else { return };
                if ui.get_dialog_kind() != 0 || ui.get_native_hidden() {
                    c.pending.lock().unwrap().take();
                    return;
                }
                ui.set_dialog_title(text(&title, ui.get_english()).into());
                ui.set_dialog_message(text(&message, ui.get_english()).into());
                ui.set_dialog_kind(if restore { 11 } else { 10 });
            }
        });
        ui.on_migration_finished({
            let c = c.clone();
            move |ticket, result, error| {
                if ticket.parse::<u64>().ok() != Some(c.serial.get()) {
                    return;
                }
                let Some(ui) = c.ui.upgrade() else { return };
                ui.set_migration_executing(false);
                ui.set_migration_busy(false);
                if matches!(ui.get_dialog_kind(), 10 | 11) {
                    ui.set_dialog_kind(0);
                } else if ui.get_dialog_kind() == 0 {
                    ui.invoke_focus_main();
                }
                if !error.is_empty() {
                    debug_log::error("migration", &error);
                    ui.set_migration_error(text(&error, ui.get_english()).into());
                    return;
                }
                let value = serde_json::from_str::<serde_json::Value>(&result).unwrap_or_default();
                if value["canceled"] == true {
                    debug_log::info("migration", "canceled");
                    return;
                }
                if value["exported"] == true {
                    debug_log::info("migration", "export succeeded");
                    ui.invoke_notify("导出成功".into());
                    return;
                }
                c.completed.set(c.completed.get() + 1);
                let restored = value["restored"] == true;
                debug_log::info(
                    "migration",
                    &format!(
                        "import completed (restored={restored}, addedDevices={}, backup={})",
                        value["addedDevices"].as_u64().unwrap_or(0),
                        value["backup"].as_str().unwrap_or_default()
                    ),
                );
                let refresh = (|| -> Result<()> {
                    profiles(&ui, &c.services)?;
                    let data = c.services.profiles.lock().unwrap().read()?;
                    fill(&ui, data.devices.first());
                    if value["preferencesImported"] == true {
                        preferences(&ui, &c.services)?;
                        c.services.devices.set_environment_source(c.services.preferences.lock().unwrap().read()?.environment_source);
                        c.glass.reload()?;
                        c.background.reload();
                    }
                    c.logs.sync_settings();
                    ui.set_log_limit(
                        c.services
                            .logs
                            .settings()
                            .max_bytes
                            .map(|bytes| (bytes / 1048576).to_string())
                            .unwrap_or_default()
                            .into(),
                    );
                    Ok(())
                })();
                if let Err(e) = refresh {
                    debug_log::error("migration", &format!("post-import refresh failed: {e:#}"));
                    ui.set_migration_error(e.to_string().into());
                } else {
                    ui.invoke_notify("导入成功".into());
                }
            }
        });
        c
    }
    fn choose(self: &Rc<Self>, export: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_dialog_kind() != 0
            || ui.get_migration_busy()
            || ui.get_system_busy()
            || ui.get_busy()
            || ui.get_connected()
            || ui.get_bg_busy()
        {
            return;
        }
        self.glass.flush();
        self.background.flush();
        let guard = match MigrationSession::begin(self.services.clone()) {
            Ok(g) => g,
            Err(e) => {
                ui.set_migration_error(e.to_string().into());
                return;
            }
        };
        ui.set_migration_error("".into());
        ui.set_migration_status("".into());
        ui.set_migration_busy(true);
        let initial = self
            .services
            .profiles
            .lock()
            .unwrap()
            .directory
            .join("migration-backups");
        let path = ui
            .window()
            .with_winit_window(|window| {
                let picker = rfd::FileDialog::new().set_parent(window);
                if export {
                    picker
                        .set_title(text("导出配置", ui.get_english()))
                        .add_filter("PortCanvas", &["portcanvas"])
                        .set_file_name("PortCanvas.portcanvas")
                        .save_file()
                } else {
                    picker
                        .set_title(text(
                            "选择配置文件、旧版 devices.json 或备份 report.json",
                            ui.get_english(),
                        ))
                        .add_filter(text("PortCanvas 配置与旧版资料", ui.get_english()), &["portcanvas", "json"])
                        .set_directory(initial)
                        .pick_file()
                }
            })
            .flatten();
        let Some(path) = path else {
            ui.set_migration_busy(false);
            ui.invoke_focus_main();
            return;
        };
        let ticket = self.serial.get() + 1;
        self.serial.set(ticket);
        let service = self.services.clone();
        let legacy = self.legacy.clone();
        let weak = self.ui.clone();
        let pending = self.pending.clone();
        let stop = self.stop.clone();
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|j| !j.is_finished());
        jobs.push(self.runtime.spawn(async move {
            let operation = async {
                guard.check_idle()?;
                let target = service.profiles.lock().unwrap().directory.clone();
                let default_logs = service.default_log_root.clone();
                if export {
                    anyhow::ensure!(path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("portcanvas")), "请选择 .portcanvas 文件名。");
                    tokio::task::spawn_blocking(move || bundle::export(&target, &path, &default_logs)).await??;
                    return Ok(serde_json::json!({"exported":true}));
                }
                let current = service.logs.settings();
                let is_bundle = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("portcanvas"));
                let filename = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
                let restore = filename.eq_ignore_ascii_case("report.json");
                let folder = path.parent().context("无法确定资料目录。")?.to_path_buf();
                let mut staging = None;
                let (plan, title, message) = if restore {
                    (None, "恢复导入前资料", format!("恢复所选迁移之前的PortCanvas配置？\n{}\n当前配置会另存备份；日志文件不会删除。", folder.display()))
                } else {
                    let plan = if is_bundle {
                        let (plan, stage) = tokio::task::spawn_blocking(move || bundle::prepare(path, target, current, default_logs)).await??;
                        staging = Some(stage);
                        plan
                    } else {
                        anyhow::ensure!(filename.eq_ignore_ascii_case("devices.json"), "选择 .portcanvas、devices.json 或 report.json。");
                        let source = folder.clone();
                        tokio::task::spawn_blocking(move || Plan::prepare_native(source, target, current, legacy)).await??
                    };
                    let p = &plan.preview;
                    let message = format!("导入资料？\n来源：{}\n目标：{}\n新增设备：{}；受信指纹：{}\n日志位置：{}\n{}\n导入前将备份现有配置。",p["source"].as_str().unwrap_or_default(),p["target"].as_str().unwrap_or_default(),p["addedDevices"],p["addedHostKeys"],p["logRoot"].as_str().unwrap_or_default(),p["warning"].as_str().unwrap_or_default());
                    (Some(plan), "导入数据", message)
                };
                if stop.is_cancelled() {
                    return Ok(serde_json::json!({"canceled":true}));
                }
                let (tx, rx) = oneshot::channel();
                *pending.lock().unwrap() = Some(tx);
                weak.upgrade_in_event_loop(move |ui| ui.invoke_migration_confirm(ticket.to_string().into(), title.into(), message.into(), restore))
                    .map_err(|error| { pending.lock().unwrap().take(); error })
                    .context("确认窗口未正常结束。")?;
                let accepted = tokio::select! {
                    _ = stop.cancelled() => false,
                    result = rx => result.unwrap_or(false)
                };
                pending.lock().unwrap().take();
                if !accepted {
                    return Ok(serde_json::json!({"canceled":true}));
                }
                let result = if restore { guard.restore(folder).await } else { guard.apply(plan.unwrap()).await };
                drop(staging);
                result
            }.await;
            let (result, error) = match operation {
                Ok(value) => (value.to_string(), String::new()),
                Err(e) => (String::new(), format!("{e:#}")),
            };
            let _ = weak.upgrade_in_event_loop(move |ui| ui.invoke_migration_finished(ticket.to_string().into(), result.into(), error.into()));
        }));
    }
    pub fn respond(&self, accepted: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_migration_executing() {
            return;
        }
        if accepted {
            ui.set_migration_executing(true);
        }
        if let Some(tx) = self.pending.lock().unwrap().take() {
            let _ = tx.send(accepted);
        }
    }
    pub fn shutdown(&self) {
        self.stop.cancel();
        self.pending.lock().unwrap().take();
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"requests":self.serial.get(),"completed":self.completed.get(),"confirmationPending":self.pending.lock().unwrap().is_some()})
    }
}

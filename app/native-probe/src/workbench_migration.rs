use crate::{
    workbench::{fill, profiles, text},
    workbench_logs,
    workbench_ui::Workbench,
};
use anyhow::{Context, Result};
use portcanvas_core::{
    migration::{MigrationSession, Plan},
    services::Services,
};
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
}
impl Controller {
    pub fn install(
        ui: &Workbench,
        services: Arc<Services>,
        runtime: Arc<Runtime>,
        jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
        logs: workbench_logs::Controller,
        legacy: PathBuf,
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
        });
        ui.on_migration_action({
            let c = c.clone();
            move |restore| c.choose(restore)
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
                    ui.set_migration_error(error);
                    return;
                }
                let value = serde_json::from_str::<serde_json::Value>(&result).unwrap_or_default();
                if value["canceled"] == true {
                    ui.set_migration_status(text("已取消资料操作。", ui.get_english()).into());
                    return;
                }
                c.completed.set(c.completed.get() + 1);
                ui.set_migration_status(
                    text(
                        &if value["restored"] == true {
                            format!(
                                "已恢复导入前资料，日志文件保留。\n当前资料备份：{}",
                                value["backup"].as_str().unwrap_or_default()
                            )
                        } else {
                            format!(
                                "资料导入完成，新增 {} 台设备。备份：{}",
                                value["addedDevices"].as_u64().unwrap_or(0),
                                value["backup"].as_str().unwrap_or_default()
                            )
                        },
                        ui.get_english(),
                    )
                    .into(),
                );
                let refresh = (|| -> Result<()> {
                    profiles(&ui, &c.services)?;
                    let data = c.services.profiles.lock().unwrap().read()?;
                    fill(&ui, data.devices.first());
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
                    ui.set_migration_error(e.to_string().into());
                }
            }
        });
        c
    }
    fn choose(self: &Rc<Self>, restore: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_dialog_kind() != 0
            || ui.get_migration_busy()
            || ui.get_system_busy()
            || ui.get_busy()
            || ui.get_connected()
        {
            return;
        }
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
        let folder = ui
            .window()
            .with_winit_window(|window| {
                let picker = rfd::FileDialog::new().set_parent(window);
                if restore {
                    picker
                        .set_title(text("选择迁移备份目录（含report.json）", ui.get_english()))
                        .set_directory(initial)
                        .pick_folder()
                } else {
                    picker
                        .set_title(text(
                            "选择旧版软件的data目录（含devices.json）",
                            ui.get_english(),
                        ))
                        .pick_folder()
                }
            })
            .flatten();
        let Some(folder) = folder else {
            ui.set_migration_busy(false);
            ui.set_migration_status(text("已取消资料操作。", ui.get_english()).into());
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
                let current = service.logs.settings();
                let (plan, title, message) = if restore {
                    (None, "恢复导入前资料", format!("恢复所选迁移之前的PortCanvas配置？\n{}\n当前配置会另存备份；日志文件不会删除。", folder.display()))
                } else {
                    let source = folder.clone();
                    let plan = tokio::task::spawn_blocking(move || Plan::prepare(source, target, current, legacy)).await??;
                    let p = &plan.preview;
                    let message = format!("导入旧版资料？\n来源：{}\n目标：{}\n新增设备：{}；受信指纹：{}\n日志位置：{}\n{}\n将备份目标资料；不移动日志，不修改旧版目录。",p["source"].as_str().unwrap_or_default(),p["target"].as_str().unwrap_or_default(),p["addedDevices"],p["addedHostKeys"],p["logRoot"].as_str().unwrap_or_default(),p["warning"].as_str().unwrap_or_default());
                    (Some(plan), "导入旧版资料", message)
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
                if restore { guard.restore(folder).await } else { guard.apply(plan.unwrap()).await }
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

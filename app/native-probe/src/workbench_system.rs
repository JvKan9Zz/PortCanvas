use crate::{workbench::text, workbench_ui::Workbench};
use anyhow::{ensure, Context, Result};
use portcanvas_core::services::Services;
use portcanvas_native_probe::debug_log;
use slint::{winit_030::WinitWindowAccessor, ComponentHandle};
use std::{
    cell::Cell,
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
    pending: Arc<Mutex<Option<oneshot::Sender<bool>>>>,
    stop: CancellationToken,
    serial: Cell<u64>,
    opens: Cell<u64>,
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
            pending: Arc::new(Mutex::new(None)),
            stop: CancellationToken::new(),
            serial: Cell::new(0),
            opens: Cell::new(0),
        });
        ui.on_system_action({
            let c = c.clone();
            move |action| c.action(action.as_str())
        });
        ui.on_system_confirm({let c=c.clone();move|ticket,target|{
            let Some(ui)=c.ui.upgrade()else{return};if ticket.parse::<u64>().ok()!=Some(c.serial.get()){return;}
            if ui.get_dialog_kind()!=0{c.pending.lock().unwrap().take();return;}
            ui.set_dialog_title(text("重启远端设备",ui.get_english()).into());
            ui.set_dialog_message(text(&format!("即将重启：\n{target}\n远端正在运行的程序将结束，所有连接会断开，是否继续？"),ui.get_english()).into());ui.set_dialog_kind(9);
        }});
        ui.on_system_finished({
            let c = c.clone();
            move |ticket, error| {
                if ticket.parse::<u64>().ok() != Some(c.serial.get()) {
                    return;
                }
                if let Some(ui) = c.ui.upgrade() {
                    ui.set_system_executing(false);
                    ui.set_system_busy(false);
                    if ui.get_dialog_kind() == 9 {
                        ui.set_dialog_kind(0);
                    } else if ui.get_dialog_kind() == 0 {
                        ui.invoke_focus_main();
                    }
                    ui.set_system_error(error);
                }
            }
        });
        c
    }
    fn action(self: &Rc<Self>, action: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_system_busy() || ui.get_dialog_kind() != 0 {
            return;
        }
        if action != "export-logs" && ui.get_busy() {
            return;
        }
        if !matches!(action, "repository" | "directory" | "reboot" | "export-logs") {
            return;
        }
        if action == "reboot" && (!ui.get_connected() || ui.get_file_mutating()) {
            return;
        }
        ui.set_system_error("".into());
        if action == "export-logs" {
            let default_name = debug_log::default_export_filename();
            let path = ui
                .window()
                .with_winit_window(|window| {
                    rfd::FileDialog::new()
                        .set_parent(window)
                        .set_title(text("导出运行日志", ui.get_english()))
                        .add_filter(text("日志文件", ui.get_english()), &["log", "txt"])
                        .set_file_name(&default_name)
                        .save_file()
                })
                .flatten();
            let Some(path) = path else {
                ui.invoke_focus_main();
                return;
            };
            ui.set_system_busy(true);
            let ticket = self.serial.get() + 1;
            self.serial.set(ticket);
            let weak = self.ui.clone();
            let mut jobs = self.jobs.lock().unwrap();
            jobs.retain(|j| !j.is_finished());
            jobs.push(self.runtime.spawn(async move {
                let target = path.clone();
                let result = tokio::task::spawn_blocking(move || debug_log::export_latest(&target))
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|v| v);
                let error = match result {
                    Ok(bytes) => {
                        debug_log::info("feedback", &format!("exported debug log ({bytes} bytes) to {}", path.display()));
                        String::new()
                    }
                    Err(e) => {
                        debug_log::error("feedback", &format!("export debug log failed: {e:#}"));
                        format!("{e:#}")
                    }
                };
                let ok = error.is_empty();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.invoke_system_finished(ticket.to_string().into(), error.into());
                    if ok {
                        ui.invoke_notify("日志已导出".into());
                    }
                });
            }));
            return;
        }
        ui.set_system_busy(true);
        let ticket = self.serial.get() + 1;
        self.serial.set(ticket);
        let weak = self.ui.clone();
        let s = self.services.clone();
        let pending = self.pending.clone();
        let stop = self.stop.clone();
        let action = action.to_owned();
        if action != "reboot" {
            self.opens.set(self.opens.get() + 1);
        }
        debug_log::info("system", &format!("action={action}"));
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|j| !j.is_finished());
        jobs.push(self.runtime.spawn(async move {
            let result = if action == "reboot" {
                let prompt = weak.clone();
                s.reboot(move |target| async move {
                    let (tx, rx) = oneshot::channel();
                    *pending.lock().unwrap() = Some(tx);
                    if prompt
                        .upgrade_in_event_loop(move |ui| {
                            ui.invoke_system_confirm(ticket.to_string().into(), target.into())
                        })
                        .is_err()
                    {
                        return Ok(false);
                    }
                    Ok(tokio::select! {_ = stop.cancelled()=>false,result=rx=>result.unwrap_or(false)})
                })
                .await
                .map(|_| ())
            } else {
                tokio::task::spawn_blocking(move || open(&action))
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|v| v)
            };
            let error = result.err().map(|e| {
                debug_log::error("system", &format!("action failed: {e:#}"));
                format!("{e:#}")
            }).unwrap_or_default();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_system_finished(ticket.to_string().into(), error.into())
            });
        }));
    }
    pub fn respond(&self, accepted: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_system_executing() {
            return;
        }
        if accepted {
            ui.set_system_executing(true);
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
        serde_json::json!({"requests":self.serial.get(),"externalOpens":self.opens.get(),"confirmationPending":self.pending.lock().unwrap().is_some()})
    }
}
fn open(action: &str) -> Result<()> {
    let target = if action == "repository" {
        std::ffi::OsString::from(portcanvas_core::preferences::REPOSITORY)
    } else {
        let executable = std::env::current_exe()?;
        let path = executable.parent().context("无法读取软件安装目录。")?;
        ensure!(path.is_dir(), "目录不存在或不是绝对路径。");
        path.as_os_str().to_owned()
    };
    #[cfg(target_os="macos")]
    anyhow::ensure!(std::process::Command::new("/usr/bin/open").arg(&target).status()?.success(),"无法打开目录或链接。");
    #[cfg(windows)]
    {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::{w, PCWSTR},
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let status = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(target.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    ensure!(status.0 as isize > 32, "无法打开目录或仓库。");
    }
    Ok(())
}

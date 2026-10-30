use crate::{
    workbench::text,
    workbench_ui::{LogRow, Workbench},
};
use anyhow::{Context, Result};
use portcanvas_core::services::Services;
use portcanvas_native_probe::log_browser::{removable, Browser, Page};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

type Jobs = Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>;
const STATUSES: [&str; 6] = [
    "",
    "running",
    "succeeded",
    "failed",
    "interrupted",
    "write_error",
];
fn value<'a>(record: &'a Value, key: &str) -> &'a str {
    record[key].as_str().unwrap_or("")
}
fn date(input: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(input)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| input.to_owned())
}
fn bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1048576 {
        format!("{:.1} KiB", n as f64 / 1024.)
    } else {
        format!("{:.1} MiB", n as f64 / 1048576.)
    }
}
fn status(record: &Value, english: bool) -> String {
    let label = match value(record, "status") {
        "running" => "记录中",
        "succeeded" => "命令成功",
        "failed" => "命令失败",
        "interrupted" => "记录中断",
        "write_error" => "写入失败",
        other => other,
    };
    format!(
        "{}{}",
        text(label, english),
        if record["complete"] == true {
            String::new()
        } else {
            text(" · 有缺失", english)
        }
    )
}
#[derive(Clone)]
pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<tokio::runtime::Runtime>,
    jobs: Jobs,
    state: Arc<Mutex<Browser>>,
    pending: Arc<Mutex<Option<(PathBuf, Vec<String>)>>>,
    counts: Arc<Mutex<(u64, u64, Vec<Value>)>>,
}
impl Controller {
    pub fn install(
        ui: &Workbench,
        services: Arc<Services>,
        runtime: Arc<tokio::runtime::Runtime>,
        jobs: Jobs,
    ) -> Self {
        let this = Self {
            ui: ui.as_weak(),
            state: Arc::new(Mutex::new(Browser::new(services.logs.settings().root))),
            services,
            runtime,
            jobs,
            pending: Arc::new(Mutex::new(None)),
            counts: Arc::new(Mutex::new((0, 0, Vec::new()))),
        };
        ui.on_refresh_logs({
            let this = this.clone();
            move || this.refresh()
        });
        ui.on_poll_logs({
            let this = this.clone();
            move || {
                if this.state.lock().unwrap().selected_running() {
                    this.read();
                }
            }
        });
        ui.on_filter_logs({
            let this = this.clone();
            move |project, day, status| {
                let changed = {
                    let mut state = this.state.lock().unwrap();
                    let project = state
                        .project_options()
                        .get(project.saturating_sub(1) as usize)
                        .filter(|_| project > 0)
                        .map(|p| p.0.clone())
                        .unwrap_or_default();
                    let day = state
                        .day_options()
                        .get(day.saturating_sub(1) as usize)
                        .filter(|_| day > 0)
                        .cloned()
                        .unwrap_or_default();
                    let status = STATUSES.get(status as usize).unwrap_or(&"").to_string();
                    state.filters(project, day, status)
                };
                if changed {
                    this.refresh();
                }
            }
        });
        ui.on_log_page({
            let this = this.clone();
            move |next| {
                let changed = {
                    let mut state = this.state.lock().unwrap();
                    if next {
                        state.next()
                    } else {
                        state.back()
                    }
                };
                if changed {
                    this.refresh();
                }
            }
        });
        ui.on_select_log({
            let this = this.clone();
            move |id| {
                this.state.lock().unwrap().select(id.into());
                if let Some(ui) = this.ui.upgrade() {
                    this.render(&ui);
                }
                this.read();
            }
        });
        ui.on_toggle_log({
            let this = this.clone();
            move |id, checked| {
                this.state.lock().unwrap().toggle(&id, checked);
                if let Some(ui) = this.ui.upgrade() {
                    this.render(&ui);
                }
            }
        });
        ui.on_select_all_logs({
            let this = this.clone();
            move |checked| {
                this.state.lock().unwrap().select_all(checked);
                if let Some(ui) = this.ui.upgrade() {
                    this.render(&ui);
                }
            }
        });
        ui.on_remove_logs({
            let this = this.clone();
            move || this.prepare_remove()
        });
        ui.on_choose_log_directory({
            let this = this.clone();
            move || this.choose_directory()
        });
        ui.on_open_log_directory({
            let this = this.clone();
            move |record| this.open_directory(record)
        });
        this.render(ui);
        this
    }
    fn reap_jobs(&self) {
        self.jobs.lock().unwrap().retain(|job| !job.is_finished());
    }
    fn visible(ui: &Workbench) -> bool {
        ui.get_page() == 3 && !ui.get_native_hidden()
    }
    pub fn refresh(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        self.state
            .lock()
            .unwrap()
            .sync_root(self.services.logs.settings().root);
        if !Self::visible(&ui) || ui.get_logs_mutating() {
            return;
        }
        let request = self.state.lock().unwrap().begin_page();
        let Some((version, query)) = request else {
            return;
        };
        ui.set_logs_loading(true);
        self.counts.lock().unwrap().0 += 1;
        let this = self.clone();
        let weak = self.ui.clone();
        self.reap_jobs();
        self.jobs
            .lock()
            .unwrap()
            .push(self.runtime.spawn(async move {
                let result = this
                    .services
                    .logs
                    .page(query)
                    .await
                    .and_then(|v| Ok(serde_json::from_value::<Page>(v)?));
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    let (accepted, reload) = {
                        let mut state = this.state.lock().unwrap();
                        let accepted = match result {
                            Ok(page) => {
                                let current = state.finish_page(version, Some(page));
                                if current {
                                    ui.set_logs_error("".into());
                                }
                                current
                            }
                            Err(e) => {
                                let current = state.finish_page(version, None);
                                if current {
                                    ui.set_logs_error(e.to_string().into());
                                }
                                current
                            }
                        };
                        (accepted, state.reload)
                    };
                    this.render(&ui);
                    if reload && Self::visible(&ui) {
                        this.refresh();
                    } else if accepted && Self::visible(&ui) {
                        this.read();
                    }
                });
            }));
    }
    fn read(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if !Self::visible(&ui) || ui.get_logs_mutating() {
            return;
        }
        let Some((version, id)) = self.state.lock().unwrap().begin_detail() else {
            return;
        };
        self.render_detail(&ui);
        self.counts.lock().unwrap().1 += 1;
        let this = self.clone();
        let weak = self.ui.clone();
        self.reap_jobs();
        self.jobs
            .lock()
            .unwrap()
            .push(self.runtime.spawn(async move {
                let result = this.services.logs.detail(id).await;
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    let reread = {
                        let mut state = this.state.lock().unwrap();
                        match result {
                            Ok(detail) => {
                                state.finish_detail(version, Some(detail));
                            }
                            Err(e) => {
                                if state.finish_detail(version, None) {
                                    state.clear_detail();
                                    ui.set_logs_error(e.to_string().into());
                                }
                            }
                        }
                        state.reread
                    };
                    this.render_detail(&ui);
                    if reread && Self::visible(&ui) {
                        this.read();
                    }
                });
            }));
    }
    pub fn render(&self, ui: &Workbench) {
        let state = self.state.lock().unwrap();
        let english = ui.get_english();
        let projects = state.project_options();
        let days = state.day_options();
        ui.set_logs_loading(state.loading);
        ui.set_log_directory(state.root.to_string_lossy().as_ref().into());
        let snapshot = self.services.logs.snapshot();
        let notice = if snapshot["configError"] == true {
            value(&snapshot, "message")
        } else if !state.page.notice.is_empty() {
            &state.page.notice
        } else {
            value(&snapshot, "notice")
        };
        ui.set_logs_notice(text(notice, english).into());
        ui.set_log_projects(ModelRc::new(VecModel::from(
            std::iter::once(text("全部项目", english).into())
                .chain(projects.iter().map(|p| p.1.as_str().into()))
                .collect::<Vec<slint::SharedString>>(),
        )));
        ui.set_log_days(ModelRc::new(VecModel::from(
            std::iter::once(text("全部日期", english).into())
                .chain(days.iter().map(|s| s.as_str().into()))
                .collect::<Vec<slint::SharedString>>(),
        )));
        ui.set_log_project_index(
            projects
                .iter()
                .position(|p| p.0 == state.query.project)
                .map(|n| n as i32 + 1)
                .unwrap_or(0),
        );
        ui.set_log_day_index(
            days.iter()
                .position(|d| *d == state.query.day)
                .map(|n| n as i32 + 1)
                .unwrap_or(0),
        );
        ui.set_log_status_index(
            STATUSES
                .iter()
                .position(|s| *s == state.query.status)
                .unwrap_or(0) as i32,
        );
        ui.set_log_rows(ModelRc::new(VecModel::from(
            state
                .page
                .records
                .iter()
                .map(|r| LogRow {
                    id: value(r, "id").into(),
                    command: value(r, "command").into(),
                    context: format!(
                        "{} · {} · {}",
                        value(&r["project"], "name"),
                        date(value(r, "startedAt")),
                        bytes(r["bytes"].as_u64().unwrap_or(0))
                    )
                    .into(),
                    status: status(r, english).into(),
                    removable: removable(r),
                    checked: state.checked.contains(value(r, "id")),
                    selected: state.selected.as_deref() == Some(value(r, "id")),
                })
                .collect::<Vec<_>>(),
        )));
        let eligible = state.page.records.iter().filter(|r| removable(r)).count();
        ui.set_logs_selectable(eligible > 0);
        ui.set_logs_all_checked(eligible > 0 && state.checked.len() == eligible);
        ui.set_logs_checked(state.checked.len() as i32);
        ui.set_logs_back(!state.history.is_empty());
        ui.set_logs_next(state.page.has_more);
        ui.set_logs_summary(
            format!(
                "{} · {} {}",
                text(
                    &format!(
                        "第 {} 页 · 共 {} 份",
                        state.history.len() + 1,
                        state.page.total
                    ),
                    english
                ),
                text("全部日志占用", english),
                bytes(state.page.usage_bytes)
            )
            .into(),
        );
        drop(state);
        self.render_detail(ui);
    }
    fn render_detail(&self, ui: &Workbench) {
        let state = self.state.lock().unwrap();
        let english = ui.get_english();
        ui.set_log_selected(state.selected.is_some());
        ui.set_log_reading(state.reading);
        ui.set_log_live(state.selected_running());
        if let Some(record) = &state.detail {
            ui.set_log_command(value(record, "command").into());
            let exit = record["exitCode"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| text("未确认", english));
            let duration = record["durationMs"]
                .as_u64()
                .map(|n| text(&format!("{:.2} 秒", n as f64 / 1000.), english))
                .unwrap_or_else(|| text("未确认", english));
            ui.set_log_metadata(
                format!(
                    "{} · {}\n{} → {}\n{}: {} · {}: {}\n{}: {}\n{}",
                    value(&record["project"], "name"),
                    status(record, english),
                    date(value(record, "startedAt")),
                    date(value(record, "endedAt")),
                    text("退出码", english),
                    exit,
                    text("耗时", english),
                    duration,
                    text("工作目录", english),
                    value(record, "cwd"),
                    value(record, "file")
                )
                .into(),
            );
            let warnings = [
                (
                    record["complete"] != true,
                    "此记录包含缺失输出或未正常封存，不能视为完整运行记录。",
                ),
                (
                    record["truncated"] == true,
                    "仅显示末尾256KiB，完整文本保存在日志文件中。",
                ),
                (
                    record["backgroundJobs"]
                        .as_array()
                        .is_some_and(|jobs| !jobs.is_empty()),
                    "后台程序可能继续运行，其输出请结合会话记录查看。",
                ),
            ];
            ui.set_log_warning(
                warnings
                    .into_iter()
                    .filter(|(show, _)| *show)
                    .map(|(_, s)| text(s, english))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into(),
            );
            let output = value(record, "text");
            if ui.get_log_output().as_str() != output {
                ui.set_log_output(output.into());
            }
        } else {
            ui.set_log_command("".into());
            ui.set_log_metadata("".into());
            ui.set_log_warning("".into());
            ui.set_log_output("".into());
        }
    }
    fn prepare_remove(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_logs_mutating() || ui.get_busy() || ui.get_dialog_kind() != 0 {
            return;
        }
        let (root, ids) = {
            let state = self.state.lock().unwrap();
            (
                state.root.clone(),
                state.checked.iter().cloned().collect::<Vec<_>>(),
            )
        };
        if ids.is_empty() {
            return;
        }
        ui.set_logs_mutating(true);
        ui.set_logs_error("".into());
        ui.set_logs_message("".into());
        let this = self.clone();
        let weak = self.ui.clone();
        self.reap_jobs();
        self.jobs.lock().unwrap().push(self.runtime.spawn(async move{
            let result=this.services.logs.removable(ids.clone()).await;
            let _=weak.upgrade_in_event_loop(move|ui|{
                ui.set_logs_mutating(false);
                if this.services.logs.settings().root!=root {ui.set_logs_error("日志位置已变化，请重新选择。".into());return;}
                match result {Err(e)=>ui.set_logs_error(e.to_string().into()),Ok(records)=>{
                    if records.is_empty(){this.state.lock().unwrap().checked.clear();this.refresh();return;}
                    let detail=records.iter().take(8).map(|r|format!("{} / {} / {}",value(&r["project"],"name"),value(r,"day"),value(r,"command").chars().take(70).collect::<String>())).collect::<Vec<_>>().join("\n");
                    *this.pending.lock().unwrap()=Some((root,ids));
                    ui.set_dialog_title("清理本地日志".into());
                    ui.set_dialog_message(text(&format!("删除选中的{}份已结束或中断日志？\n{}\n仅处理电脑本地记录，运行中的日志保留。",records.len(),detail),ui.get_english()).into());ui.set_dialog_kind(3);
                }}
            });
        }));
    }
    pub fn respond(&self, accepted: bool) {
        let pending = self.pending.lock().unwrap().take();
        let Some((root, ids)) = pending else { return };
        if !accepted {
            return;
        }
        let Some(ui) = self.ui.upgrade() else { return };
        if self.services.logs.settings().root != root {
            ui.set_logs_error("日志位置已变化，请重新选择。".into());
            return;
        }
        ui.set_logs_mutating(true);
        let this = self.clone();
        let weak = self.ui.clone();
        self.reap_jobs();
        self.jobs
            .lock()
            .unwrap()
            .push(self.runtime.spawn(async move {
                let result = this.services.logs.remove(ids.clone()).await;
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_logs_mutating(false);
                    match result {
                        Ok(result) => {
                            let mut state = this.state.lock().unwrap();
                            state.checked.clear();
                            if state.selected.as_ref().is_some_and(|id| ids.contains(id)) {
                                state.clear_detail();
                            }
                            ui.set_logs_error("".into());
                            ui.set_logs_message(
                                text(
                                    &format!(
                                        "已清理 {} 份本地日志。",
                                        result["removed"].as_u64().unwrap_or(0)
                                    ),
                                    ui.get_english(),
                                )
                                .into(),
                            );
                        }
                        Err(e) => ui.set_logs_error(e.to_string().into()),
                    };
                    this.refresh();
                });
            }));
    }
    fn choose_directory(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if let Err(e) = self.services.logs.can_change_root() {
            ui.set_error(e.to_string().into());
            return;
        }
        let folder = ui
            .window()
            .with_winit_window(|window| {
                rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(text("选择电脑本地日志目录", ui.get_english()))
                    .set_directory(self.services.logs.settings().root)
                    .pick_folder()
            })
            .flatten();
        let Some(folder) = folder else { return };
        ui.set_logs_mutating(true);
        ui.set_error("".into());
        let this = self.clone();
        let weak = self.ui.clone();
        self.reap_jobs();
        self.jobs
            .lock()
            .unwrap()
            .push(self.runtime.spawn(async move {
                let result = this.services.logs.set_root(folder).await;
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_logs_mutating(false);
                    match result {
                        Ok(_) => {
                            this.state
                                .lock()
                                .unwrap()
                                .sync_root(this.services.logs.settings().root);
                            this.render(&ui);
                        }
                        Err(e) => ui.set_error(e.to_string().into()),
                    }
                });
            }));
    }
    fn open_directory(&self, record: bool) {
        let path = {
            let state = self.state.lock().unwrap();
            if record {
                state
                    .detail
                    .as_ref()
                    .map(|v| PathBuf::from(value(v, "directory")))
            } else {
                Some(state.root.clone())
            }
        };
        let Some(path) = path else { return };
        let weak = self.ui.clone();
        self.reap_jobs();
        self.jobs
            .lock()
            .unwrap()
            .push(self.runtime.spawn(async move {
                let result = tokio::task::spawn_blocking(move || open_folder(path, !record))
                    .await
                    .context("无法打开目录。")
                    .and_then(|r| r);
                if let Err(e) = result {
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        if ui.get_page() == 1 {
                            ui.set_error(e.to_string().into());
                        } else {
                            ui.set_logs_error(e.to_string().into());
                        }
                    });
                }
            }));
    }
    pub fn visibility(&self, hidden: bool) {
        if std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_none() {
            return;
        }
        let mut counts = self.counts.lock().unwrap();
        let (page, detail) = (counts.0, counts.1);
        counts
            .2
            .push(json!({"at":chrono::Utc::now().to_rfc3339(),"hidden":hidden,"pages":page,"details":detail}));
    }
    pub fn report(&self) -> Value {
        let counts = self.counts.lock().unwrap();
        json!({"pages":counts.0,"details":counts.1,"visibility":counts.2})
    }
}
use slint::winit_030::WinitWindowAccessor;
fn open_folder(path: PathBuf, create: bool) -> Result<()> {
    anyhow::ensure!(path.is_absolute(), "目录必须是绝对路径。");
    if create {
        std::fs::create_dir_all(&path)?;
    }
    anyhow::ensure!(
        path.is_absolute() && path.is_dir(),
        "目录不存在或不是绝对路径。"
    );
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::{w, PCWSTR},
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!(result.0 as isize > 32, "无法打开目录。");
    Ok(())
}

use crate::{
    workbench::text,
    workbench_declarative::BackgroundBadge,
    workbench_ui::{PluginCard, Workbench},
};
use anyhow::Result;
use portcanvas_core::plugins::{
    CatalogClient, CatalogEntry, ImportPreview, InstalledPlugin, Manifest, PluginStore, ARENA_ID,
    FAST_DOWNLOADER_ID, VIDEO_CAPTURE_ID, VISION_ID,
};
use portcanvas_native_probe::{debug_log, file_browser::bytes};
use serde_json::{json, Value};
use slint::{winit_030::WinitWindowAccessor, ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    future::Future,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const INSTALLED: i32 = 0;
const DOWNLOADABLE: i32 = 1;
const INSTALLED_STATUS: &str = "插件已安装";
const IMPORT_OP_ID: &str = "__import__";

const KEY: &str = include_str!("../assets/update-public-key.txt");
const PLUGIN_DEV_DOC: &str = include_str!("../../../docs/plugin-development.md");

struct Operation {
    cancel: CancellationToken,
    progress: f32,
    status: &'static str,
    last_draw: Instant,
}

#[derive(Default)]
struct State {
    catalog: Vec<CatalogEntry>,
    installed: Vec<InstalledPlugin>,
    refresh: Option<CancellationToken>,
    operations: BTreeMap<String, Operation>,
    background_badges: BTreeMap<String, BackgroundBadge>,
    active_downloader_id: Option<String>,
    status: &'static str,
    catalog_error: String,
    local_error: String,
    operation_error: String,
    closed: bool,
}

struct PendingImport {
    path: PathBuf,
    preview: ImportPreview,
}

pub struct Controller {
    ui: slint::Weak<Workbench>,
    store: PluginStore,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    state: Arc<Mutex<State>>,
    uninstall: RefCell<Option<String>>,
    pending_import: RefCell<Option<PendingImport>>,
    stop_hook: RefCell<Option<Rc<dyn Fn(&str)>>>,
}

impl Controller {
    pub fn install(
        ui: &Workbench,
        store: PluginStore,
        runtime: Arc<Runtime>,
        jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) -> Rc<Self> {
        let mut state = State::default();
        state.read_installed(&store);
        ui.set_plugin_filter(state.initial_filter());
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            store,
            runtime,
            jobs,
            state: Arc::new(Mutex::new(state)),
            uninstall: RefCell::new(None),
            pending_import: RefCell::new(None),
            stop_hook: RefCell::new(None),
        });
        ui.on_plugin_refresh({
            let controller = controller.clone();
            move || controller.refresh()
        });
        ui.on_plugin_import({
            let controller = controller.clone();
            move || controller.open_upload_menu()
        });
        ui.on_plugin_upload_choice({
            let controller = controller.clone();
            move |choice| controller.handle_upload_choice(choice)
        });
        ui.on_plugin_filter_changed({
            let controller = controller.clone();
            move |filter| {
                controller.render();
                if filter == DOWNLOADABLE {
                    controller.refresh();
                }
            }
        });
        ui.on_plugin_action({
            let controller = controller.clone();
            move |id, action| controller.action(&id, &action)
        });
        ui.on_plugin_close_active({
            let controller = controller.clone();
            move || {
                controller.close(None);
                if controller
                    .ui
                    .upgrade()
                    .is_some_and(|ui| ui.get_plugin_filter() == DOWNLOADABLE)
                {
                    controller.refresh();
                }
            }
        });
        ui.on_plugin_state_changed({
            let controller = controller.clone();
            move || controller.render()
        });
        controller.render();
        controller
    }

    pub fn set_stop_hook(&self, hook: Rc<dyn Fn(&str)>) {
        *self.stop_hook.borrow_mut() = Some(hook);
    }

    pub fn update_background_badges(&self, badges: BTreeMap<String, BackgroundBadge>) {
        {
            let mut state = self.state.lock().unwrap();
            state.background_badges = badges;
        }
        self.render();
    }

    fn stop_runtime(&self, id: &str) {
        if let Some(hook) = self.stop_hook.borrow().as_ref() {
            hook(id);
        }
    }

    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|job| !job.is_finished());
        jobs.push(self.runtime.spawn(task));
    }

    fn refresh(&self) {
        let cancel = CancellationToken::new();
        {
            let mut state = self.state.lock().unwrap();
            if state.closed || state.refresh.is_some() {
                return;
            }
            state.refresh = Some(cancel.clone());
            state.catalog_error.clear();
        }
        self.render();
        let state = self.state.clone();
        let weak = self.ui.clone();
        self.spawn(async move {
            let result =
                async { CatalogClient::from_environment(KEY)?.refresh(&cancel).await }.await;
            {
                let mut state = state.lock().unwrap();
                state.refresh = None;
                if !state.closed {
                    match result {
                        Ok(catalog) => state.catalog = catalog.plugins,
                        Err(error) => state.catalog_error = error.to_string(),
                    }
                }
            }
            let _ = weak.upgrade_in_event_loop(|ui| ui.invoke_plugin_state_changed());
        });
    }

    fn open_upload_menu(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if self.plugin_busy(IMPORT_OP_ID) || ui.get_dialog_kind() != 0 {
            return;
        }
        ui.set_dialog_title(text("上传插件", ui.get_english()).into());
        ui.set_dialog_message(
            text("请选择导入本地插件包，或下载 PortCanvas 插件开发规范文档。", ui.get_english())
                .into(),
        );
        ui.set_dialog_kind(19);
    }

    fn handle_upload_choice(&self, choice: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_dialog_kind(0);
        match choice {
            0 => self.pick_local_plugin(),
            1 => self.save_dev_spec_doc(),
            _ => {}
        }
    }

    fn save_dev_spec_doc(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let default_name = if ui.get_english() {
            "PortCanvas-Plugin-Development-Spec.md"
        } else {
            "PortCanvas-插件开发规范.md"
        };
        let target = ui
            .window()
            .with_winit_window(|window| {
                rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(text("下载插件开发规范文档", ui.get_english()))
                    .set_file_name(default_name)
                    .add_filter("Markdown", &["md"])
                    .save_file()
            })
            .flatten();
        ui.invoke_focus_main();
        let Some(path) = target else { return };
        match std::fs::write(&path, PLUGIN_DEV_DOC) {
            Ok(()) => {
                debug_log::info("plugins", &format!("saved dev spec to {}", path.display()));
                ui.invoke_notify(text("插件开发规范文档已保存", ui.get_english()).into());
            }
            Err(err) => {
                let mut state = self.state.lock().unwrap();
                state.operation_error = format!("保存插件开发规范文档失败：{err}");
                drop(state);
                self.render();
            }
        }
    }

    fn pick_local_plugin(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if self.plugin_busy(IMPORT_OP_ID) || ui.get_dialog_kind() != 0 {
            return;
        }
        let path = ui
            .window()
            .with_winit_window(|window| {
                rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(text("导入本地插件", ui.get_english()))
                    .add_filter("PortCanvas", &["pcplugin"])
                    .pick_file()
            })
            .flatten();
        ui.invoke_focus_main();
        let Some(path) = path else { return };
        match self.store.inspect_import(&path) {
            Ok(preview) => {
                let english = ui.get_english();
                let name = plugin_name(&preview.manifest, english);
                let runtime_desc = if preview.runtime_kind == "local-worker" {
                    text("本机独立程序 (local-worker)", english)
                } else {
                    text("SSH 远端脚本 (ssh)", english)
                };
                let perms = format_permissions(&preview.manifest.permissions, english);
                let mut lines = vec![
                    format!(
                        "{}: {} (v{})",
                        text("插件名称", english),
                        name,
                        preview.manifest.version
                    ),
                    format!("{}: {}", text("作者", english), preview.manifest.author),
                    format!("{}: {}", text("运行方式", english), runtime_desc),
                    format!(
                        "{}: {}",
                        text("申请权限", english),
                        if perms.is_empty() {
                            text("无特殊权限", english)
                        } else {
                            perms
                        }
                    ),
                ];
                if preview.manifest.provides_downloader() {
                    lines.push(text(
                        "附加能力：安装后将默认替换内置 32 线程下载器，卸载后自动恢复。",
                        english,
                    ));
                }
                lines.push(text(
                    "此插件为本地未签名安装包，仅在信任来源时继续安装。",
                    english,
                ));
                *self.pending_import.borrow_mut() = Some(PendingImport { path, preview });
                ui.set_dialog_title(text("确认安装本地插件", english).into());
                ui.set_dialog_message(lines.join("\n").into());
                ui.set_dialog_kind(20);
            }
            Err(err) => {
                debug_log::warn("plugins", &format!("inspect import failed: {err:#}"));
                {
                    let mut state = self.state.lock().unwrap();
                    state.operation_error = err.to_string();
                }
                self.render();
            }
        }
    }

    pub fn respond_import(&self, accepted: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        let pending = self.pending_import.borrow_mut().take();
        ui.set_dialog_kind(0);
        let Some(pending) = pending.filter(|_| accepted) else {
            return;
        };
        let plugin_id = pending.preview.manifest.id.clone();
        self.close(Some(&plugin_id));
        self.stop_runtime(&plugin_id);
        debug_log::info(
            "plugins",
            &format!("import confirmed path={}", pending.path.display()),
        );
        let Some(cancel) = self.begin(IMPORT_OP_ID, "正在校验并安装…") else {
            return;
        };
        let store = self.store.clone();
        let path = pending.path;
        self.finish(
            IMPORT_OP_ID.to_owned(),
            cancel.clone(),
            INSTALLED_STATUS,
            async move {
                tokio::task::spawn_blocking(move || store.import(&path, &cancel)).await??;
                Ok(())
            },
        );
    }

    fn action(&self, id: &str, action: &str) {
        if action == "cancel" {
            if let Some(operation) = self.state.lock().unwrap().operations.get(id) {
                debug_log::info("plugins", &format!("cancel plugin={id}"));
                operation.cancel.cancel();
            }
            return;
        }

        let Some(ui) = self.ui.upgrade() else { return };
        if self.plugin_busy(id) || ui.get_page() != 9 || ui.get_dialog_kind() != 0 {
            return;
        }
        let installed = self
            .state
            .lock()
            .unwrap()
            .installed
            .iter()
            .find(|plugin| plugin.manifest.id == id)
            .cloned();
        match (action, installed) {
            ("install", None) => self.download(id, None),
            ("update", Some(plugin)) => self.download(id, Some(plugin)),
            ("open", Some(plugin)) if can_open(&plugin) => {
                debug_log::info(
                    "plugins",
                    &format!("open plugin={id} version={}", plugin.manifest.version),
                );
                ui.set_plugin_active_title(plugin_name(&plugin.manifest, ui.get_english()).into());
                ui.set_plugin_active_version(plugin.manifest.version.to_string().into());
                ui.set_plugin_active_id(id.into());
            }
            ("enable" | "disable", Some(_)) => self.change_enabled(id, action == "enable"),
            ("uninstall", Some(plugin)) => {
                *self.uninstall.borrow_mut() = Some(id.to_owned());
                let name = plugin_name(&plugin.manifest, ui.get_english());
                let english = ui.get_english();
                let mut msg = text(&format!("确认卸载「{name}」？"), english);
                if plugin.manifest.provides_downloader() {
                    msg.push('\n');
                    msg.push_str(&text(
                        "卸载后将自动恢复软件内置 32 线程下载器。",
                        english,
                    ));
                }
                ui.set_dialog_title(text("卸载插件", english).into());
                ui.set_dialog_message(msg.into());
                ui.set_dialog_kind(18);
            }
            _ => {}
        }
    }

    fn download(&self, id: &str, installed: Option<InstalledPlugin>) {
        let entry = self
            .state
            .lock()
            .unwrap()
            .catalog
            .iter()
            .find(|entry| {
                entry.manifest.id == id
                    && entry.manifest.compatible()
                    && entry.current_package().is_some()
                    && installed
                        .as_ref()
                        .is_none_or(|plugin| entry.manifest.version > plugin.manifest.version)
            })
            .cloned();
        let Some(entry) = entry else { return };
        let Some(pkg) = entry.current_package() else {
            return;
        };
        self.close(Some(id));
        self.stop_runtime(id);
        let Some(cancel) = self.begin(id, "正在下载插件…") else {
            return;
        };
        debug_log::info(
            "plugins",
            &format!("download start plugin={id} version={}", entry.manifest.version),
        );
        let store = self.store.clone();
        let weak = self.ui.clone();
        let state = self.state.clone();
        let op_id = id.to_owned();
        let progress_id = op_id.clone();
        self.finish(op_id, cancel.clone(), INSTALLED_STATUS, async move {
            let size = pkg.size_bytes.max(1);
            let mut_bytes = Mutex::new(0u64);
            CatalogClient::from_environment(KEY)?
                .download_install(&store, &entry, &cancel, move |chunk, _, _| {
                    let mut received = mut_bytes.lock().unwrap();
                    *received += chunk as u64;
                    let mut state = state.lock().unwrap();
                    if let Some(operation) = state.operations.get_mut(&progress_id) {
                        operation.progress = (*received as f64 / size as f64).min(1.) as f32;
                        let complete = *received >= size;
                        if complete {
                            operation.status = "正在校验并安装…";
                        }
                        if complete || operation.last_draw.elapsed() >= Duration::from_millis(100) {
                            operation.last_draw = Instant::now();
                            let _ =
                                weak.upgrade_in_event_loop(|ui| ui.invoke_plugin_state_changed());
                        }
                    }
                })
                .await?;
            Ok(())
        });
    }

    fn change_enabled(&self, id: &str, enabled: bool) {
        if !enabled {
            self.close(Some(id));
            self.stop_runtime(id);
        }
        let Some(cancel) = self.begin(id, "") else {
            return;
        };
        let store = self.store.clone();
        let op_id = id.to_owned();
        let target_id = op_id.clone();
        self.finish(
            op_id,
            cancel,
            if enabled { "插件已启用" } else { "插件已停用" },
            async move {
                tokio::task::spawn_blocking(move || store.set_enabled(&target_id, enabled))
                    .await??;
                Ok(())
            },
        );
    }

    pub fn respond(&self, accepted: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        let id = self.uninstall.borrow_mut().take();
        ui.set_dialog_kind(0);
        let Some(id) = id.filter(|_| accepted) else {
            return;
        };
        self.close(Some(&id));
        self.stop_runtime(&id);
        let Some(cancel) = self.begin(&id, "") else {
            return;
        };
        debug_log::info("plugins", &format!("uninstall plugin={id}"));
        let store = self.store.clone();
        let target_id = id.clone();
        self.finish(id, cancel, "插件已卸载", async move {
            tokio::task::spawn_blocking(move || store.uninstall(&target_id)).await??;
            Ok(())
        });
    }

    fn begin(&self, id: &str, status: &'static str) -> Option<CancellationToken> {
        let cancel = CancellationToken::new();
        {
            let mut state = self.state.lock().unwrap();
            if state.closed || state.operations.contains_key(id) {
                return None;
            }
            state.operation_error.clear();
            state.status = "";
            state.operations.insert(
                id.to_owned(),
                Operation {
                    cancel: cancel.clone(),
                    progress: 0.,
                    status,
                    last_draw: Instant::now(),
                },
            );
        }
        self.render();
        Some(cancel)
    }

    fn finish(
        &self,
        id: String,
        cancel: CancellationToken,
        success: &'static str,
        task: impl Future<Output = Result<()>> + Send + 'static,
    ) {
        let weak = self.ui.clone();
        let state = self.state.clone();
        let store = self.store.clone();
        self.spawn(async move {
            let result = task.await;
            let toast_msg = {
                let mut state = state.lock().unwrap();
                state.operations.remove(&id);
                state.read_installed(&store);
                match result {
                    Ok(()) => {
                        debug_log::info(
                            "plugins",
                            &format!("finished plugin={id} status={success}"),
                        );
                        state.status = success;
                        Some(success)
                    }
                    Err(_) if cancel.is_cancelled() => {
                        debug_log::info("plugins", &format!("canceled plugin={id}"));
                        state.status = "已取消下载";
                        None
                    }
                    Err(error) => {
                        debug_log::error("plugins", &format!("failed plugin={id}: {error:#}"));
                        state.operation_error = error.to_string();
                        None
                    }
                }
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_plugin_state_changed();
                if let Some(msg) = toast_msg.filter(|m| !m.is_empty()) {
                    ui.invoke_notify(msg.into());
                }
            });
        });
    }

    fn plugin_busy(&self, id: &str) -> bool {
        let state = self.state.lock().unwrap();
        state.closed || state.operations.contains_key(id)
    }

    fn close(&self, id: Option<&str>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let active = ui.get_plugin_active_id();
        if !active.is_empty() && id.is_none_or(|id| id == active.as_str()) {
            ui.set_plugin_active_id("".into());
            ui.set_plugin_active_title("".into());
            ui.set_plugin_active_version("".into());
        }
    }

    fn render(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let english = ui.get_english();
        let filter = ui.get_plugin_filter();
        let active = ui.get_plugin_active_id();
        let (loading, status, error, rows, opened) = {
            let state = self.state.lock().unwrap();
            let rows = state.cards(filter, english);
            let status = state
                .operations
                .values()
                .map(|operation| operation.status)
                .find(|s| !s.is_empty())
                .unwrap_or(state.status);
            let error = [
                &state.operation_error,
                &state.local_error,
                &state.catalog_error,
            ]
            .into_iter()
            .find(|message| !message.is_empty())
            .map(|message| text(message, english))
            .unwrap_or_default();
            let opened = state
                .installed
                .iter()
                .find(|plugin| plugin.manifest.id == active.as_str() && can_open(plugin))
                .map(|plugin| plugin_name(&plugin.manifest, english).to_owned());
            (
                state.refresh.is_some(),
                text(status, english),
                error,
                rows,
                opened,
            )
        };
        if let Some(title) = opened {
            ui.set_plugin_active_title(title.into());
        } else if !active.is_empty() {
            self.close(None);
        }
        ui.set_plugin_loading(loading);
        ui.set_plugin_status(status.into());
        ui.set_plugin_error(error.into());
        ui.set_plugin_items(ModelRc::new(VecModel::from(rows)));
    }

    pub fn shutdown(&self) {
        {
            let mut state = self.state.lock().unwrap();
            state.closed = true;
            if let Some(cancel) = &state.refresh {
                cancel.cancel();
            }
            for operation in state.operations.values() {
                operation.cancel.cancel();
            }
        }
        self.close(None);
    }

    pub fn report(&self) -> Value {
        let state = self.state.lock().unwrap();
        json!({
            "catalogItems": state.catalog.len(),
            "loading": state.refresh.is_some(),
            "busy": !state.operations.is_empty(),
            "activeOperations": state.operations.len(),
            "activeDownloaderPlugin": state.active_downloader_id,
            "downloaderBackend": portcanvas_core::download::active_downloader_summary(),
            "installed": state.installed.iter().map(|p| json!({
                "id": p.manifest.id,
                "version": p.manifest.version,
                "enabled": p.enabled,
                "official": p.official()
            })).collect::<Vec<_>>()
        })
    }
}

impl State {
    fn read_installed(&mut self, store: &PluginStore) {
        match store.installed() {
            Ok(installed) => {
                self.installed = installed;
                self.active_downloader_id = store.active_downloader_plugin_id();
                self.local_error.clear();
            }
            Err(error) => self.local_error = error.to_string(),
        }
    }

    fn initial_filter(&self) -> i32 {
        if self.installed.is_empty() {
            DOWNLOADABLE
        } else {
            INSTALLED
        }
    }

    fn cards(&self, filter: i32, english: bool) -> Vec<PluginCard> {
        let ids: BTreeSet<_> = self
            .installed
            .iter()
            .map(|p| p.manifest.id.as_str())
            .chain(
                self.catalog
                    .iter()
                    .filter(|e| e.manifest.compatible() && e.current_package().is_some())
                    .map(|e| e.manifest.id.as_str()),
            )
            .collect();
        ids.into_iter()
            .filter_map(|id| {
                let installed = self.installed.iter().find(|p| p.manifest.id == id);
                let entry = self
                    .catalog
                    .iter()
                    .find(|e| {
                        e.manifest.id == id
                            && e.manifest.compatible()
                            && e.current_package().is_some()
                    });
                if installed.is_some() != (filter == INSTALLED) {
                    return None;
                }
                let manifest = installed
                    .map(|p| &p.manifest)
                    .or_else(|| entry.map(|e| &e.manifest))?;
                let candidate = entry
                    .filter(|e| installed.is_none_or(|p| e.manifest.version > p.manifest.version));
                let operation = self.operations.get(id);
                let permissions = format_permissions(&manifest.permissions, english);
                let bg = self.background_badges.get(id);
                let official = installed.map_or(manifest.official, InstalledPlugin::official);
                let downloader_active =
                    installed.is_some() && self.active_downloader_id.as_deref() == Some(id);
                Some(PluginCard {
                    id: id.into(),
                    name: plugin_name(manifest, english).into(),
                    version: candidate
                        .map_or(&manifest.version, |e| &e.manifest.version)
                        .to_string()
                        .into(),
                    installed_version: installed
                        .map(|p| p.manifest.version.to_string())
                        .unwrap_or_default()
                        .into(),
                    summary: localized(&manifest.summary, &manifest.summary_en, english).into(),
                    author: manifest.author.as_str().into(),
                    official,
                    local_badge: installed.is_some() && !official,
                    installed: installed.is_some(),
                    enabled: installed.is_some_and(|p| p.enabled),
                    update_available: installed.is_some() && candidate.is_some(),
                    can_open: installed.is_some_and(can_open),
                    busy: operation.is_some(),
                    progress: operation.map_or(0., |op| op.progress),
                    status_text: operation
                        .map(|op| text(op.status, english))
                        .unwrap_or_default()
                        .into(),
                    size_text: candidate
                        .and_then(|e| e.current_package())
                        .map(|pkg| bytes(pkg.size_bytes))
                        .unwrap_or_default()
                        .into(),
                    permissions: permissions.into(),
                    icon_kind: icon_kind_for(manifest),
                    background_running: bg.is_some_and(|b| b.active),
                    background_summary: bg.map(|b| b.summary.as_str()).unwrap_or("").into(),
                    downloader_active,
                })
            })
            .collect()
    }
}

fn format_permissions(perms: &[String], english: bool) -> String {
    perms
        .iter()
        .map(|permission| {
            text(
                match permission.as_str() {
                    "camera.stream" => "摄像头",
                    "pose.stream" => "定位",
                    "vision.watch" => "视觉监视",
                    "ssh.exec" => "SSH",
                    "local.exec" => "本机进程",
                    "local.capture" => "视频采集",
                    "local.network" => "网络连接",
                    "local.fs" => "本地读写",
                    other => other,
                },
                english,
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn icon_kind_for(manifest: &Manifest) -> i32 {
    if let Some(icon) = manifest.icon.as_deref() {
        match icon {
            "arena" => return 1,
            "vision" => return 2,
            "camera" | "capture" => return 3,
            "download" | "downloader" => return 4,
            _ => {}
        }
    }
    match manifest.id.as_str() {
        ARENA_ID => 1,
        VISION_ID => 2,
        VIDEO_CAPTURE_ID => 3,
        FAST_DOWNLOADER_ID => 4,
        _ => 0,
    }
}

fn localized<'a>(primary: &'a str, english_value: &'a str, english: bool) -> &'a str {
    if english && !english_value.is_empty() {
        english_value
    } else {
        primary
    }
}

fn plugin_name(manifest: &Manifest, english: bool) -> &str {
    localized(&manifest.name, &manifest.name_en, english)
}

fn can_open(plugin: &InstalledPlugin) -> bool {
    plugin.enabled
        && plugin.manifest.compatible()
        && plugin.manifest.platform_compatible()
        && plugin.manifest.is_declarative()
}

#[cfg(test)]
mod tests {
    use super::*;
    use portcanvas_core::plugins::{PluginRuntimeConfig, SURFACE_DECLARATIVE_V1};

    fn make_declarative(id: &str, name: &str, name_en: &str, icon: Option<&str>) -> Manifest {
        let mut m: Manifest = serde_json::from_value(json!({
            "schema": "portcanvas.plugin/1",
            "id": id,
            "name": name,
            "nameEn": name_en,
            "version": "0.1.0",
            "summary": "s",
            "summaryEn": "s",
            "category": "tools",
            "author": "PortCanvas",
            "official": true,
            "minAppVersion": "0.7.0",
            "hostSurface": SURFACE_DECLARATIVE_V1,
            "platform": "any",
            "requiresDevice": false,
            "lifecycle": "foreground",
            "ui": "ui.json",
            "runtime": { "kind": "ssh", "adapter": "generic" },
            "permissions": ["ssh.exec"]
        }))
        .unwrap();
        m.icon = icon.map(str::to_owned);
        m
    }

    #[test]
    fn any_declarative_plugin_opens_without_host_id_allowlist() -> Result<()> {
        let manifest = make_declarative("custom-oscilloscope", "示波器", "Oscilloscope", Some("camera"));
        let mut state = State::default();
        state.installed.push(InstalledPlugin {
            manifest,
            enabled: true,
            from_catalog: false,
        });
        let cards = state.cards(INSTALLED, true);
        assert_eq!(cards[0].name.as_str(), "Oscilloscope");
        assert_eq!(cards[0].icon_kind, 3);
        assert!(cards[0].can_open && !cards[0].official && cards[0].local_badge);
        state.installed[0].manifest.host_surface = "unknown".into();
        assert!(!state.cards(INSTALLED, false)[0].can_open);
        Ok(())
    }

    #[test]
    fn background_badge_and_downloader_override_surface_on_card() -> Result<()> {
        let mut manifest = make_declarative(
            FAST_DOWNLOADER_ID,
            "128线程下载器",
            "128-Thread Downloader",
            Some("download"),
        );
        manifest.lifecycle = "background".into();
        manifest.provides = vec!["downloader".into()];
        manifest.runtime = Some(PluginRuntimeConfig::LocalWorker {
            entry: "bin/worker.exe".into(),
            args: vec![],
            frame_channel: false,
        });
        let mut state = State::default();
        state.installed.push(InstalledPlugin {
            manifest,
            enabled: true,
            from_catalog: true,
        });
        state.active_downloader_id = Some(FAST_DOWNLOADER_ID.into());
        state.background_badges.insert(
            FAST_DOWNLOADER_ID.into(),
            BackgroundBadge {
                active: true,
                summary: "2 个任务 · 48.0 MB/s".into(),
            },
        );
        let cards = state.cards(INSTALLED, false);
        assert_eq!(cards.len(), 1);
        assert!(cards[0].can_open);
        assert!(cards[0].background_running);
        assert_eq!(cards[0].background_summary.as_str(), "2 个任务 · 48.0 MB/s");
        assert!(cards[0].downloader_active);
        Ok(())
    }
}

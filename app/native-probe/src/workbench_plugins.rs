use crate::{
    workbench::text,
    workbench_ui::{PluginCard, Workbench},
};
use anyhow::Result;
use portcanvas_core::plugins::{
    CatalogClient, CatalogEntry, InstalledPlugin, Manifest, PluginStore, ARENA_ID, VISION_ID,
};
use portcanvas_native_probe::{debug_log, file_browser::bytes};
use serde_json::{json, Value};
use slint::{winit_030::WinitWindowAccessor, ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    future::Future,
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
    status: &'static str,
    catalog_error: String,
    local_error: String,
    operation_error: String,
    closed: bool,
}

pub struct Controller {
    ui: slint::Weak<Workbench>,
    store: PluginStore,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    state: Arc<Mutex<State>>,
    uninstall: RefCell<Option<String>>,
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
        });
        ui.on_plugin_refresh({
            let controller = controller.clone();
            move || controller.refresh()
        });
        ui.on_plugin_import({
            let controller = controller.clone();
            move || controller.import()
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

    fn import(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if self.plugin_busy(IMPORT_OP_ID) || ui.get_dialog_kind() != 0 {
            return;
        }
        let path = ui
            .window()
            .with_winit_window(|window| {
                rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(text("导入插件包", ui.get_english()))
                    .add_filter("PortCanvas", &["pcplugin"])
                    .pick_file()
            })
            .flatten();
        ui.invoke_focus_main();
        let Some(path) = path else { return };
        debug_log::info("plugins", &format!("import start path={}", path.display()));
        let Some(cancel) = self.begin(IMPORT_OP_ID, "正在校验并安装…") else {
            return;
        };
        let store = self.store.clone();
        self.finish(IMPORT_OP_ID.to_owned(), cancel.clone(), INSTALLED_STATUS, async move {
            tokio::task::spawn_blocking(move || store.import(&path, &cancel)).await??;
            Ok(())
        });
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
                debug_log::info("plugins", &format!("open plugin={id} version={}", plugin.manifest.version));
                ui.set_plugin_active_title(plugin_name(&plugin.manifest, ui.get_english()).into());
                ui.set_plugin_active_version(plugin.manifest.version.to_string().into());
                ui.set_plugin_active_id(id.into());
            }
            ("enable" | "disable", Some(_)) => self.change_enabled(id, action == "enable"),
            ("uninstall", Some(plugin)) => {
                *self.uninstall.borrow_mut() = Some(id.to_owned());
                let name = plugin_name(&plugin.manifest, ui.get_english());
                ui.set_dialog_title(text("卸载插件", ui.get_english()).into());
                ui.set_dialog_message(
                    text(&format!("确认卸载「{name}」？"), ui.get_english()).into(),
                );
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
                    && installed
                        .as_ref()
                        .is_none_or(|plugin| entry.manifest.version > plugin.manifest.version)
            })
            .cloned();
        let Some(entry) = entry else { return };
        self.close(Some(id));
        let Some(cancel) = self.begin(id, "正在下载插件…") else {
            return;
        };
        debug_log::info("plugins", &format!("download start plugin={id} version={}", entry.manifest.version));
        let store = self.store.clone();
        let weak = self.ui.clone();
        let state = self.state.clone();
        let op_id = id.to_owned();
        let progress_id = op_id.clone();
        self.finish(op_id, cancel.clone(), INSTALLED_STATUS, async move {
            let size = entry.size_bytes;
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
            if enabled {
                "插件已启用"
            } else {
                "插件已停用"
            },
            async move {
                tokio::task::spawn_blocking(move || store.set_enabled(&target_id, enabled)).await??;
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
            {
                let mut state = state.lock().unwrap();
                state.operations.remove(&id);
                state.read_installed(&store);
                match result {
                    Ok(()) => {
                        debug_log::info("plugins", &format!("finished plugin={id} status={success}"));
                        state.status = success;
                    }
                    Err(_) if cancel.is_cancelled() => {
                        debug_log::info("plugins", &format!("canceled plugin={id}"));
                        state.status = "已取消下载";
                    }
                    Err(error) => {
                        debug_log::error("plugins", &format!("failed plugin={id}: {error:#}"));
                        state.operation_error = error.to_string();
                    }
                }
            }
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.invoke_plugin_state_changed();
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
        json!({"catalogItems":state.catalog.len(), "loading":state.refresh.is_some(), "busy":!state.operations.is_empty(), "activeOperations":state.operations.len(),
            "installed":state.installed.iter().map(|p| json!({"id":p.manifest.id, "version":p.manifest.version,
                "enabled":p.enabled,"official":p.official()})).collect::<Vec<_>>()})
    }
}

impl State {
    fn read_installed(&mut self, store: &PluginStore) {
        match store.installed() {
            Ok(installed) => {
                self.installed = installed;
                self.local_error.clear();
            }
            Err(error) => self.local_error = error.to_string(),
        }
    }

    fn initial_filter(&self) -> i32 {
        if self.installed.is_empty() { DOWNLOADABLE } else { INSTALLED }
    }

    fn cards(&self, filter: i32, english: bool) -> Vec<PluginCard> {
        let ids: BTreeSet<_> = self
            .installed
            .iter()
            .map(|p| p.manifest.id.as_str())
            .chain(
                self.catalog
                    .iter()
                    .filter(|e| e.manifest.compatible())
                    .map(|e| e.manifest.id.as_str()),
            )
            .collect();
        ids.into_iter()
            .filter_map(|id| {
                let installed = self.installed.iter().find(|p| p.manifest.id == id);
                let entry = self
                    .catalog
                    .iter()
                    .find(|e| e.manifest.id == id && e.manifest.compatible());
                if installed.is_some() != (filter == INSTALLED) {
                    return None;
                }
                let manifest = installed
                    .map(|p| &p.manifest)
                    .or_else(|| entry.map(|e| &e.manifest))?;
                let candidate = entry
                    .filter(|e| installed.is_none_or(|p| e.manifest.version > p.manifest.version));
                let operation = self.operations.get(id);
                let permissions = manifest
                    .permissions
                    .iter()
                    .map(|permission| {
                        text(
                            match permission.as_str() {
                                "camera.stream" => "摄像头",
                                "pose.stream" => "定位",
                                "vision.watch" => "视觉监视",
                                "ssh.exec" => "SSH",
                                other => other,
                            },
                            english,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
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
                    official: installed.map_or(manifest.official, InstalledPlugin::official),
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
                        .map(|e| bytes(e.size_bytes))
                        .unwrap_or_default()
                        .into(),
                    permissions: permissions.into(),
                    icon_kind: match id { ARENA_ID => 1, VISION_ID => 2, _ => 0 },
                })
            })
            .collect()
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
        && matches!((plugin.manifest.id.as_str(), plugin.manifest.host_surface.as_str()),
                    (ARENA_ID, "arena-v1") | (VISION_ID, "vision-v1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vision_cards_open_only_the_supported_and_enabled_surface() -> Result<()> {
        let manifest = serde_json::from_str(include_str!("../../../plugins/vision-studio/manifest.json"))?;
        let mut state = State::default();
        state.installed.push(InstalledPlugin { manifest, enabled: true, from_catalog: true });
        let cards = state.cards(INSTALLED, true);
        assert_eq!(cards[0].name.as_str(), "Vision Studio");
        assert_eq!(cards[0].icon_kind, 2);
        assert!(cards[0].can_open && cards[0].official);
        state.installed[0].manifest.host_surface = "unknown".into();
        assert!(!state.cards(INSTALLED, false)[0].can_open);
        Ok(())
    }

    #[test]
    fn cached_local_install_survives_catalog_failure_with_its_own_provenance() -> Result<()> {
        let mut manifest: Manifest =
            serde_json::from_str(include_str!("../../../plugins/arena-monitor/manifest.json"))?;
        let mut catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../../plugins/catalog.json"))?;
        catalog["plugins"][0]["version"] = json!("0.7.1");
        let entry: CatalogEntry = serde_json::from_value(catalog["plugins"][0].clone())?;
        let mut state = State {
            catalog: vec![entry],
            catalog_error: "offline".into(),
            ..State::default()
        };
        assert_eq!(state.initial_filter(), DOWNLOADABLE);
        let available = state.cards(DOWNLOADABLE, true);
        assert_eq!(available.len(), 1);
        assert!(available[0].official && !available[0].installed && !available[0].can_open);
        assert!(state.cards(INSTALLED, true).is_empty());

        state.installed.push(InstalledPlugin {
            manifest: manifest.clone(),
            enabled: true,
            from_catalog: false,
        });
        assert_eq!(state.initial_filter(), INSTALLED);
        let local = state.cards(INSTALLED, true);
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].name.as_str(), "Arena Monitor");
        assert!(local[0].can_open && local[0].update_available && !local[0].official);
        assert_eq!(local[0].version.as_str(), "0.7.1");
        assert!(state.cards(DOWNLOADABLE, true).is_empty());
        assert_eq!(state.cards(INSTALLED, false).len(), 1);
        state.installed[0].enabled = false;
        assert!(!state.cards(INSTALLED, false)[0].can_open);

        manifest.min_app_version.major += 1;
        state.catalog[0].manifest = manifest;
        assert!(!state.cards(INSTALLED, true)[0].update_available);
        Ok(())
    }

    #[test]
    fn concurrent_downloads_keep_independent_progress_and_installed_plugins_openable() -> Result<()> {
        let catalog: portcanvas_core::plugins::Catalog =
            serde_json::from_str(include_str!("../../../plugins/catalog.json"))?;
        let arena_manifest: Manifest =
            serde_json::from_str(include_str!("../../../plugins/arena-monitor/manifest.json"))?;
        let mut state = State {
            catalog: catalog.plugins,
            ..State::default()
        };
        state.operations.insert(
            ARENA_ID.into(),
            Operation {
                cancel: CancellationToken::new(),
                progress: 0.35,
                status: "正在下载插件…",
                last_draw: Instant::now(),
            },
        );
        state.operations.insert(
            VISION_ID.into(),
            Operation {
                cancel: CancellationToken::new(),
                progress: 0.80,
                status: "正在校验并安装…",
                last_draw: Instant::now(),
            },
        );
        let downloadable = state.cards(DOWNLOADABLE, false);
        assert_eq!(downloadable.len(), 2);
        let arena = downloadable.iter().find(|c| c.id == ARENA_ID).unwrap();
        let vision = downloadable.iter().find(|c| c.id == VISION_ID).unwrap();
        assert!(arena.busy && (arena.progress - 0.35).abs() < 0.01);
        assert!(vision.busy && (vision.progress - 0.80).abs() < 0.01);

        // When arena-monitor finishes while vision-studio is still downloading,
        // arena-monitor is installed, not busy, and immediately openable.
        state.operations.remove(ARENA_ID);
        state.installed.push(InstalledPlugin {
            manifest: arena_manifest,
            enabled: true,
            from_catalog: true,
        });
        let installed = state.cards(INSTALLED, false);
        assert_eq!(installed.len(), 1);
        assert!(!installed[0].busy && installed[0].can_open);
        let remaining = state.cards(DOWNLOADABLE, false);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id.as_str(), VISION_ID);
        assert!(remaining[0].busy);
        Ok(())
    }
}

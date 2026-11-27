//! Shared ownership and lifecycle. Callers supply platform paths and an event sink.
use crate::{
    desktop::DesktopService,
    files::FileService,
    logs::LogService,
    profiles::ProfileStore,
    session::{DeviceManager, EventSink},
    terminal_pool::TerminalPool,
};
use anyhow::Result;
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex, OnceLock, Weak},
};
use tokio_util::sync::CancellationToken;

pub struct Paths {
    pub data: PathBuf,
    pub plugins: Option<PathBuf>,
    pub documents: PathBuf,
    pub local_files: PathBuf,
    pub portable: bool,
}
pub struct Services {
    pub rebooting: AtomicBool,
    pub profiles: Arc<Mutex<ProfileStore>>,
    pub devices: DeviceManager,
    pub desktop: DesktopService,
    pub arena: crate::arena::ArenaService,
    pub pose: crate::arena_pose::PoseService,
    pub plugins: crate::plugins::PluginStore,
    pub vision: crate::vision::VisionService,
    pub quitting: AtomicBool,
    pub files: FileService,
    pub terminal: TerminalPool,
    pub logs: LogService,
    pub migrating: AtomicBool,
    pub default_log_root: PathBuf,
    pub preferences: Mutex<crate::preferences::PreferenceStore>,
    pub background_draft: Mutex<Option<crate::preferences::BackgroundDraft>>,
    pub background_import: Mutex<CancellationToken>,
    pub discovery: crate::discovery::Discovery,
}
impl Services {
    pub fn new(paths: Paths, notify: EventSink, namespace: Option<String>) -> Result<Arc<Self>> {
        if !paths.data.is_absolute() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "应用数据目录须为绝对路径",
            )
            .into());
        }
        anyhow::ensure!(
            paths.plugins.as_ref().is_none_or(|directory| directory.is_absolute()),
            "插件目录须为绝对路径。"
        );
        let preferences = crate::preferences::PreferenceStore {
            file: paths.data.join("settings.json"),
        };
        preferences.cleanup_drafts()?;
        // Events outlive individual UI callbacks; keep service ownership weak to avoid a cycle.
        let owner: Arc<OnceLock<Weak<Self>>> = Arc::new(OnceLock::new());
        let target = owner.clone();
        let events: EventSink = Arc::new(move |name, data| {
            if name == "device-state" {
                if let Some(state) = target.get().and_then(Weak::upgrade) {
                    if data["ssh"] != "connected" {
                        state.files.invalidate();
                        state.terminal.stop_all();
                        state
                            .logs
                            .detach("设备连接已关闭，未收到的输出保留为缺失。");
                    } else {
                        state.logs.refresh_status();
                    }
                }
            }
            notify(name, data);
        });
        let discovery = crate::discovery::Discovery::new(events.clone());
        let source = preferences
            .read()
            .map(|p| p.environment_source)
            .unwrap_or_default();
        let devices =
            DeviceManager::new(events.clone()).with_runtime_cache(paths.data.join("runtime-cache"));
        devices.set_environment_source(source);
        let desktop = DesktopService::new(devices.clone());
        let plugins = match paths.plugins {
            Some(root) => crate::plugins::PluginStore::from_root(root),
            None => crate::plugins::PluginStore::new(&paths.data),
        }
        .with_data_root(paths.data.join("plugin-data"))
        .with_global_downloader_sync(true);
        plugins.sync_downloader_provider();
        let arena = crate::arena::ArenaService::new(devices.clone(), plugins.clone());
        let vision = crate::vision::VisionService::new(devices.clone(), plugins.clone());
        let pose = crate::arena_pose::PoseService::new(devices.clone(), events, plugins.clone());
        let files = FileService::new(devices.clone(), paths.local_files);
        let legacy = paths.documents.join("Wheel Combat Studio/Logs");
        let default_log_root = if paths.portable || cfg!(target_os = "macos") {
            paths.data.join("logs")
        } else if legacy.is_dir() {
            legacy
        } else {
            paths.documents.join("PortCanvas/Logs")
        };
        let logs = LogService::new(
            devices.clone(),
            paths.data.join("log-settings.json"),
            default_log_root.clone(),
        )?;
        let terminal = TerminalPool::new(
            devices.clone(),
            logs.clone(),
            namespace,
            paths.data.join("terminals.json"),
        );
        let services = Arc::new(Self {
            rebooting: AtomicBool::new(false),
            profiles: Arc::new(Mutex::new(ProfileStore {
                directory: paths.data,
            })),
            devices,
            desktop,
            arena,
            pose,
            plugins,
            vision,
            quitting: AtomicBool::new(false),
            files,
            terminal,
            logs,
            migrating: AtomicBool::new(false),
            default_log_root,
            preferences: Mutex::new(preferences),
            background_draft: Mutex::new(None),
            background_import: Mutex::new(CancellationToken::new()),
            discovery,
        });
        let _ = owner.set(Arc::downgrade(&services));
        Ok(services)
    }
    pub async fn shutdown(&self) {
        self.discovery.cancel();
        self.background_import.lock().unwrap().cancel();
        self.terminal.stop_all();
        self.files.cancel();
        tokio::join!(self.arena.stop(None), self.pose.stop(None), self.vision.stop(None));
        self.desktop.stop(None).await;
        self.devices.disconnect().await;
        let _ = self.logs.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_data_directory_keeps_the_legacy_error_kind() {
        let mut input = paths(true);
        input.data = PathBuf::from("relative-core-data");
        let result = Services::new(input, Arc::new(|_, _| {}), None);
        let error = result.err().expect("relative paths must be rejected");
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }
    fn paths(portable: bool) -> Paths {
        let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated test root"))
            .join(format!("services-{}", uuid::Uuid::new_v4()));
        Paths {
            data: root.join("data"),
            plugins: None,
            documents: root.join("documents"),
            local_files: root.join("files"),
            portable,
        }
    }
    #[test]
    fn plugin_store_uses_the_supplied_absolute_root_or_the_data_default() -> Result<()> {
        use crate::plugins::{InstalledPlugin, ARENA_ID};
        let mut input = paths(true);
        let sandbox = tempfile::tempdir_in(std::env::var_os("WCS_TEST_ROOT").expect("isolated test root"))?;
        input.data = sandbox.path().join("data");
        let explicit = sandbox.path().join("install/plugins");
        let manifest = serde_json::from_str(include_str!("../../../plugins/arena-monitor/manifest.json"))?;
        let mut installed = InstalledPlugin { manifest, enabled: true, from_catalog: false };
        crate::profiles::atomic_json(&explicit.join("installed.json"), &[installed.clone()])?;
        installed.enabled = false;
        crate::profiles::atomic_json(&input.data.join("plugins/installed.json"), &[installed])?;

        input.plugins = Some("relative-plugins".into());
        assert!(Services::new(input, Arc::new(|_, _| {}), None).is_err());
        for (plugins, enabled) in [(Some(explicit), true), (None, false)] {
            let services = Services::new(Paths {
                data: sandbox.path().join("data"),
                plugins,
                documents: sandbox.path().to_path_buf(),
                local_files: sandbox.path().to_path_buf(),
                portable: true,
            }, Arc::new(|_, _| {}), None)?;
            let installed = services.plugins.installed()?;
            assert_eq!(installed.len(), 1);
            assert_eq!(installed[0].manifest.id, ARENA_ID);
            assert_eq!(installed[0].enabled, enabled);
        }
        Ok(())
    }
    #[tokio::test]
    async fn both_callers_share_data_and_credentials_without_a_window() -> Result<()> {
        let paths = paths(true);
        let data = paths.data.clone();
        let docs = paths.documents.clone();
        let service = Services::new(paths, Arc::new(|_, _| {}), None)?;
        let profile = crate::profiles::Profile {
            id: "native-core".into(),
            name: "本地测试".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "demo".into(),
            display: ":0".into(),
            xauthority: String::new(),
            auth_method: Default::default(),
            private_key_path: String::new(),
            remember_password: true,
            encrypted_secret: None,
        };
        service
            .profiles
            .lock()
            .unwrap()
            .save(profile, "synthetic-only")?;
        service
            .preferences
            .lock()
            .unwrap()
            .save_language(crate::preferences::Language::English)?;
        service.shutdown().await;
        drop(service);
        let reopened = Services::new(
            Paths {
                data: data.clone(),
                plugins: None,
                documents: docs.clone(),
                local_files: docs,
                portable: true,
            },
            Arc::new(|_, _| {}),
            None,
        )?;
        let profile = reopened.profiles.lock().unwrap().read()?.devices[0].clone();
        assert_eq!(
            reopened
                .profiles
                .lock()
                .unwrap()
                .connection_secret(&profile, zeroize::Zeroizing::new(String::new()))?
                .as_str(),
            "synthetic-only"
        );
        assert_eq!(
            reopened.preferences.lock().unwrap().info()?["language"],
            "en"
        );
        reopened.shutdown().await;
        drop(reopened);
        std::fs::remove_dir_all(data.parent().unwrap())?;
        Ok(())
    }
    #[tokio::test]
    async fn disconnect_observer_runs_without_tauri_and_does_not_keep_services_alive() -> Result<()>
    {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let output = seen.clone();
        let service = Services::new(
            paths(true),
            Arc::new(move |name, _| output.lock().unwrap().push(name.to_owned())),
            None,
        )?;
        let weak = Arc::downgrade(&service);
        let events = service.devices.events.clone();
        service.devices.disconnect().await;
        assert_eq!(service.devices.snapshot()["ssh"], "disconnected");
        assert_eq!(service.files.snapshot()["connection"], "disconnected");
        let names = seen.lock().unwrap();
        assert!(
            names.iter().position(|name| name == "file-state").unwrap()
                < names
                    .iter()
                    .position(|name| name == "device-state")
                    .unwrap()
        );
        drop(names);
        service.shutdown().await;
        drop(service);
        assert!(weak.upgrade().is_none());
        drop(events);
        Ok(())
    }
    #[test]
    fn supplied_paths_preserve_portable_and_legacy_log_defaults() -> Result<()> {
        let p = paths(true);
        let expected = p.data.join("logs");
        let services = Services::new(p, Arc::new(|_, _| {}), None)?;
        assert_eq!(services.default_log_root, expected);
        drop(services);
        let p = paths(false);
        let legacy = p.documents.join("Wheel Combat Studio/Logs");
        std::fs::create_dir_all(&legacy)?;
        let expected = if cfg!(target_os="macos") {p.data.join("logs")} else {legacy};
        let services = Services::new(p, Arc::new(|_, _| {}), None)?;
        assert_eq!(services.default_log_root, expected);
        Ok(())
    }
}

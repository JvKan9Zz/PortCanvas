//! Catalog packages and explicitly imported offline packages share the same installer.
use anyhow::{bail, ensure, Context, Result};
use flate2::read::GzDecoder;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

pub const SCHEMA: &str = "portcanvas.plugin/1";
pub const SURFACE_DECLARATIVE_V1: &str = "declarative-v1";
pub const ARENA_ID: &str = "arena-monitor";
pub const VISION_ID: &str = "vision-studio";
pub const VIDEO_CAPTURE_ID: &str = "video-capture";
pub const FAST_DOWNLOADER_ID: &str = "fast-downloader";
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;

pub fn current_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "darwin-universal"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "windows-x86_64"
    }
}

fn default_platform() -> String {
    "any".into()
}

fn is_default_platform(value: &str) -> bool {
    value.is_empty() || value == "any"
}

fn default_lifecycle() -> String {
    "foreground".into()
}

fn is_default_lifecycle(value: &str) -> bool {
    value.is_empty() || value == "foreground"
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PluginRuntimeConfig {
    Ssh {
        #[serde(default)]
        adapter: String,
    },
    LocalWorker {
        entry: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, rename = "frameChannel")]
        frame_channel: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_en: String,
    pub version: Version,
    pub summary: String,
    #[serde(default)]
    pub summary_en: String,
    pub category: String,
    pub author: String,
    #[serde(default)]
    pub official: bool,
    pub min_app_version: Version,
    pub host_surface: String,
    #[serde(default = "default_platform", skip_serializing_if = "is_default_platform")]
    pub platform: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_device: Option<bool>,
    #[serde(default = "default_lifecycle", skip_serializing_if = "is_default_lifecycle")]
    pub lifecycle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PluginRuntimeConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provides: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == SCHEMA, "不支持此插件包格式。");
        validate_id(&self.id)?;
        ensure!(
            matches!(
                self.platform.as_str(),
                "" | "any" | "windows-x86_64" | "darwin-universal"
            ),
            "不支持的插件平台标识：{}",
            self.platform
        );
        ensure!(
            matches!(self.lifecycle.as_str(), "" | "foreground" | "background"),
            "不支持的插件生命周期：{}",
            self.lifecycle
        );
        for path in self.assets.values().chain(self.scripts.values()) {
            validate_path(path)?;
        }
        if let Some(ui) = &self.ui {
            validate_path(ui)?;
        }
        if let Some(PluginRuntimeConfig::LocalWorker { entry, .. }) = &self.runtime {
            validate_path(entry)?;
        }
        if self.host_surface == SURFACE_DECLARATIVE_V1 {
            ensure!(
                self.ui.is_some() && self.runtime.is_some(),
                "声明式插件缺少 ui 或 runtime 定义。"
            );
        } else if self.id == ARENA_ID && self.host_surface == "arena-v1" {
            ensure!(
                self.assets.contains_key("platformImage")
                    && ["cameraList", "cameraStream", "poseStream"]
                        .iter()
                        .all(|key| self.scripts.contains_key(*key)),
                "赛台插件缺少所需图片或脚本。"
            );
        } else if self.id == VISION_ID && self.host_surface == "vision-v1" {
            ensure!(
                ["visionInspect", "visionStream"]
                    .iter()
                    .all(|key| self.scripts.contains_key(*key)),
                "视觉实验台插件缺少所需脚本。"
            );
        }
        Ok(())
    }

    pub fn compatible(&self) -> bool {
        self.min_app_version
            <= Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package version")
    }

    pub fn platform_compatible(&self) -> bool {
        self.platform.is_empty()
            || self.platform == "any"
            || self.platform == current_platform()
    }

    pub fn requires_device(&self) -> bool {
        if let Some(explicit) = self.requires_device {
            return explicit;
        }
        !matches!(self.runtime, Some(PluginRuntimeConfig::LocalWorker { .. }))
    }

    pub fn is_background(&self) -> bool {
        self.lifecycle == "background"
    }

    pub fn is_declarative(&self) -> bool {
        self.host_surface == SURFACE_DECLARATIVE_V1
    }

    pub fn provides_downloader(&self) -> bool {
        self.provides.iter().any(|cap| cap == "downloader")
    }

    pub fn matches_catalog_expected(&self, expected: &Manifest) -> bool {
        if self == expected {
            return true;
        }
        // Multi-platform catalog entries declare platform="any" (with per-platform packages),
        // while the downloaded platform package itself declares platform=current_platform() and entry path.
        let mut lhs = self.clone();
        let rhs = expected.clone();
        if is_default_platform(&rhs.platform) && lhs.platform_compatible() {
            lhs.platform = rhs.platform.clone();
        }
        if matches!(
            (&lhs.runtime, &rhs.runtime),
            (
                Some(PluginRuntimeConfig::LocalWorker { .. }),
                Some(PluginRuntimeConfig::LocalWorker { .. })
            )
        ) {
            lhs.runtime = rhs.runtime.clone();
        }
        lhs == rhs
    }
}

#[derive(Clone, Debug)]
pub struct ImportPreview {
    pub manifest: Manifest,
    pub previous_version: Option<Version>,
    pub runtime_kind: &'static str,
    pub is_local_unsigned: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    pub enabled: bool,
    pub from_catalog: bool,
}

impl InstalledPlugin {
    pub fn official(&self) -> bool {
        self.from_catalog && self.manifest.official
    }
}

#[derive(Clone)]
pub struct PluginStore {
    root: PathBuf,
    data_root: PathBuf,
    sync_global_downloader: bool,
    state: Arc<Mutex<BTreeMap<String, CancellationToken>>>,
}

impl PluginStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("plugins"),
            data_root: data_dir.join("plugin-data"),
            sync_global_downloader: false,
            state: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub(crate) fn from_root(root: PathBuf) -> Self {
        let data_root = root
            .parent()
            .unwrap_or(&root)
            .join("plugin-data");
        Self {
            root,
            data_root,
            sync_global_downloader: false,
            state: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn with_data_root(mut self, data_root: PathBuf) -> Self {
        self.data_root = data_root;
        self
    }

    pub fn with_global_downloader_sync(mut self, enabled: bool) -> Self {
        self.sync_global_downloader = enabled;
        self
    }

    pub fn active_downloader_plugin_id(&self) -> Option<String> {
        let _guard = self.state.lock().unwrap();
        self.find_active_downloader().map(|(id, _)| id)
    }

    fn find_active_downloader(&self) -> Option<(String, PathBuf)> {
        let installed = self.read().ok()?;
        for plugin in installed {
            if !plugin.enabled
                || !plugin.manifest.compatible()
                || !plugin.manifest.platform_compatible()
                || !plugin.manifest.provides_downloader()
            {
                continue;
            }
            if let Some(PluginRuntimeConfig::LocalWorker { entry, .. }) = &plugin.manifest.runtime {
                let exe = self.root.join(&plugin.manifest.id).join(entry);
                if exe.is_file() {
                    return Some((plugin.manifest.id, exe));
                }
            }
        }
        None
    }

    pub fn sync_downloader_provider(&self) {
        #[cfg(feature = "downloads")]
        {
            let provider = {
                let _guard = self.state.lock().unwrap();
                self.find_active_downloader().map(|(plugin_id, executable)| {
                    crate::download::ExternalDownloader {
                        plugin_id,
                        executable,
                        threads: 128,
                    }
                })
            };
            crate::download::set_external_downloader(provider);
        }
    }

    fn maybe_sync_downloader(&self) {
        if self.sync_global_downloader {
            self.sync_downloader_provider();
        }
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("installed.json")
    }

    fn read(&self) -> Result<Vec<InstalledPlugin>> {
        let data = match fs::read(self.state_path()) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => return Err(error.into()),
        };
        let installed: Vec<InstalledPlugin> =
            serde_json::from_slice(&data).context("已安装插件清单无效。")?;
        let mut seen = BTreeSet::new();
        for plugin in &installed {
            plugin.manifest.validate()?;
            ensure!(seen.insert(&plugin.manifest.id), "已安装插件 ID 重复。");
        }
        Ok(installed)
    }

    pub fn installed(&self) -> Result<Vec<InstalledPlugin>> {
        let _guard = self.state.lock().unwrap();
        self.read()
    }

    pub fn inspect_import(&self, path: &Path) -> Result<ImportPreview> {
        ensure!(
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pcplugin")),
            "请选择 .pcplugin 文件。"
        );
        let file = fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take(MAX_PACKAGE_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_PACKAGE_BYTES,
            "插件包超过 256 MiB。"
        );
        let scratch = tempfile::tempdir()?;
        let cancel = CancellationToken::new();
        extract(&bytes, scratch.path(), &cancel)?;
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(scratch.path().join("manifest.json"))?)
                .context("插件清单无效。")?;
        manifest.validate()?;
        ensure!(manifest.compatible(), "此插件需要更高版本的 PortCanvas。");
        ensure!(
            manifest.platform_compatible(),
            "此插件不支持当前操作系统或架构（需要 {}）。",
            manifest.platform
        );
        validate_extracted_files(&manifest, scratch.path())?;
        let previous_version = {
            let _guard = self.state.lock().unwrap();
            let installed = self.read()?;
            let prev = installed
                .iter()
                .find(|old| old.manifest.id == manifest.id)
                .map(|old| old.manifest.version.clone());
            if let Some(prev) = &prev {
                ensure!(manifest.version >= *prev, "插件版本早于已安装版本。");
            }
            prev
        };
        let runtime_kind = match &manifest.runtime {
            Some(PluginRuntimeConfig::LocalWorker { .. }) => "local-worker",
            Some(PluginRuntimeConfig::Ssh { .. }) | None => "ssh",
        };
        Ok(ImportPreview {
            manifest,
            previous_version,
            runtime_kind,
            is_local_unsigned: true,
        })
    }

    pub fn import(&self, path: &Path, cancel: &CancellationToken) -> Result<InstalledPlugin> {
        ensure!(
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pcplugin")),
            "请选择 .pcplugin 文件。"
        );
        let file = fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take(MAX_PACKAGE_BYTES + 1).read_to_end(&mut bytes)?;
        self.install(&bytes, None, cancel)
    }

    fn install(
        &self,
        bytes: &[u8],
        expected: Option<&Manifest>,
        cancel: &CancellationToken,
    ) -> Result<InstalledPlugin> {
        check_cancel(cancel)?;
        ensure!(
            bytes.len() as u64 <= MAX_PACKAGE_BYTES,
            "插件包超过 256 MiB。"
        );
        fs::create_dir_all(&self.root)?;
        let transaction = tempfile::tempdir_in(&self.root)?;
        let staged = transaction.path().join("new");
        fs::create_dir(&staged)?;
        extract(bytes, &staged, cancel)?;
        let manifest: Manifest = serde_json::from_slice(&fs::read(staged.join("manifest.json"))?)
            .context("插件清单无效。")?;
        manifest.validate()?;
        ensure!(manifest.compatible(), "此插件需要更高版本的 PortCanvas。");
        ensure!(
            manifest.platform_compatible(),
            "此插件不支持当前操作系统或架构。"
        );
        if let Some(expected) = expected {
            ensure!(
                manifest.matches_catalog_expected(expected),
                "插件包清单与下载目录不一致。"
            );
        }
        validate_extracted_files(&manifest, &staged)?;
        #[cfg(unix)]
        if let Some(PluginRuntimeConfig::LocalWorker { entry, .. }) = &manifest.runtime {
            use std::os::unix::fs::PermissionsExt;
            let entry_path = staged.join(entry);
            let mut perms = fs::metadata(&entry_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&entry_path, perms)?;
        }

        let item = {
            let mut runtime = self.state.lock().unwrap();
            check_cancel(cancel)?;
            let mut installed = self.read()?;
            let index = installed
                .iter()
                .position(|old| old.manifest.id == manifest.id);
            let previous = index.map(|index| installed.remove(index));
            if let Some(previous) = &previous {
                ensure!(
                    manifest.version >= previous.manifest.version,
                    "插件版本早于已安装版本。"
                );
            }
            let item = InstalledPlugin {
                enabled: previous.is_none_or(|old| old.enabled),
                from_catalog: expected.is_some(),
                manifest,
            };
            let target = self.root.join(&item.manifest.id);
            installed.push(item.clone());
            installed.sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
            replace_directory(transaction, &target, Some(&staged), || {
                crate::profiles::atomic_json(&self.state_path(), &installed)
            })?;
            cancel_running(&mut runtime, &item.manifest.id);
            item
        };
        self.maybe_sync_downloader();
        Ok(item)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        {
            let mut runtime = self.state.lock().unwrap();
            let mut installed = self.read()?;
            let plugin = installed
                .iter_mut()
                .find(|plugin| plugin.manifest.id == id)
                .context("插件尚未安装。")?;
            plugin.enabled = enabled;
            crate::profiles::atomic_json(&self.state_path(), &installed)?;
            if !enabled {
                cancel_running(&mut runtime, id);
            }
        }
        self.maybe_sync_downloader();
        Ok(())
    }

    pub fn uninstall(&self, id: &str) -> Result<()> {
        validate_id(id)?;
        {
            let mut runtime = self.state.lock().unwrap();
            let mut installed = self.read()?;
            let index = installed
                .iter()
                .position(|plugin| plugin.manifest.id == id)
                .context("插件尚未安装。")?;
            installed.remove(index);
            let transaction = tempfile::tempdir_in(&self.root)?;
            replace_directory(transaction, &self.root.join(id), None, || {
                crate::profiles::atomic_json(&self.state_path(), &installed)
            })?;
            cancel_running(&mut runtime, id);
        }
        self.maybe_sync_downloader();
        Ok(())
    }

    fn active(&self, id: &str) -> Result<Manifest> {
        let item = self
            .read()?
            .into_iter()
            .find(|item| item.manifest.id == id)
            .context("插件尚未安装。")?;
        ensure!(item.enabled, "插件已停用。");
        ensure!(
            item.manifest.compatible(),
            "此插件需要更高版本的 PortCanvas。"
        );
        ensure!(
            item.manifest.platform_compatible(),
            "此插件不支持当前操作系统或架构。"
        );
        Ok(item.manifest)
    }

    pub fn active_manifest(&self, id: &str) -> Result<Manifest> {
        let _guard = self.state.lock().unwrap();
        self.active(id)
    }

    pub fn plugin_dir(&self, id: &str) -> Result<PathBuf> {
        let _guard = self.state.lock().unwrap();
        let _ = self.active(id)?;
        Ok(self.root.join(id))
    }

    pub fn data_dir(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        let dir = self.data_root.join(id);
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn ui_spec_text(&self, id: &str) -> Result<String> {
        let _guard = self.state.lock().unwrap();
        let manifest = self.active(id)?;
        let ui_path = manifest.ui.as_deref().context("插件缺少 ui.json 声明。")?;
        fs::read_to_string(self.root.join(id).join(ui_path)).context("插件界面描述读取失败。")
    }

    pub fn worker_executable(
        &self,
        id: &str,
    ) -> Result<(PathBuf, Vec<String>, bool, CancellationToken)> {
        let mut runtime = self.state.lock().unwrap();
        let manifest = self.active(id)?;
        let Some(PluginRuntimeConfig::LocalWorker {
            entry,
            args,
            frame_channel,
        }) = manifest.runtime
        else {
            bail!("该插件不是本地 Worker 插件。");
        };
        let path = self.root.join(id).join(&entry);
        ensure!(path.is_file(), "本地插件程序不存在：{entry}");
        let active = runtime.entry(id.to_owned()).or_default().clone();
        Ok((path, args, frame_channel, active))
    }

    pub fn asset_path(&self, id: &str, key: &str) -> Result<PathBuf> {
        let _guard = self.state.lock().unwrap();
        let manifest = self.active(id)?;
        let path = manifest.assets.get(key).context("插件缺少所需资源。")?;
        Ok(self.root.join(id).join(path))
    }

    pub(crate) fn plugin_script(&self, id: &str, key: &str) -> Result<(String, CancellationToken)> {
        let mut runtime = self.state.lock().unwrap();
        let manifest = self.active(id)?;
        let path = manifest.scripts.get(key).context("插件缺少所需脚本。")?;
        let script = fs::read_to_string(self.root.join(id).join(path))
            .context("插件脚本读取失败。")?;
        let active = runtime.entry(id.to_owned()).or_default().clone();
        Ok((script, active))
    }
}

fn cancel_running(runtime: &mut BTreeMap<String, CancellationToken>, id: &str) {
    if let Some(token) = runtime.remove(id) {
        token.cancel();
    }
}

pub(crate) fn stop_with_plugin(
    plugin: CancellationToken,
    connection: &CancellationToken,
) -> CancellationToken {
    let cancel = connection.child_token();
    if plugin.is_cancelled() {
        cancel.cancel();
    } else {
        let stream = cancel.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = stream.cancelled() => {}
                _ = plugin.cancelled() => stream.cancel(),
            }
        });
    }
    cancel
}

fn validate_extracted_files(manifest: &Manifest, root: &Path) -> Result<()> {
    for path in manifest.assets.values().chain(manifest.scripts.values()) {
        ensure!(root.join(path).is_file(), "插件包缺少文件 {path}。");
    }
    if let Some(ui) = &manifest.ui {
        let ui_file = root.join(ui);
        ensure!(ui_file.is_file(), "插件包缺少界面文件 {ui}。");
        let text = fs::read_to_string(&ui_file).context("插件界面描述读取失败。")?;
        crate::plugin_runtime::UiSpec::parse(&text)?;
    }
    if let Some(PluginRuntimeConfig::LocalWorker { entry, .. }) = &manifest.runtime {
        ensure!(root.join(entry).is_file(), "插件包缺少可执行文件 {entry}。");
    }
    Ok(())
}

fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    ensure!(!cancel.is_cancelled(), "插件操作已取消。");
    Ok(())
}

fn replace_directory(
    transaction: TempDir,
    target: &Path,
    staged: Option<&Path>,
    persist: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let previous = transaction.path().join("previous");
    let saved = match fs::rename(target, &previous) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    let mut published = false;
    let result = (|| {
        if let Some(staged) = staged {
            fs::rename(staged, target)?;
            published = true;
        }
        persist()
    })();
    if let Err(error) = result {
        let rollback: io::Result<()> = (|| {
            if published {
                fs::rename(target, transaction.path().join("rejected"))?;
            }
            if saved {
                fs::rename(&previous, target)?;
            }
            Ok(())
        })();
        if let Err(rollback) = rollback {
            let recovery = transaction.keep();
            bail!(
                "{error}；插件恢复失败：{rollback}。临时文件保留于 {}",
                recovery.display()
            );
        }
        return Err(error);
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        id.len() <= 80
            && id.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
        "插件 ID 无效。"
    );
    validate_path(id)
}

fn validate_path(path: &str) -> Result<()> {
    ensure!(!path.is_empty() && path.len() <= 512, "插件归档路径无效。");
    for part in path.split('/') {
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit());
        ensure!(
            !reserved
                && !part.is_empty()
                && !part.ends_with('.')
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
            "插件归档路径无效：{path}"
        );
    }
    Ok(())
}

fn extract(bytes: &[u8], root: &Path, cancel: &CancellationToken) -> Result<()> {
    let mut archive = tar::Archive::new(GzDecoder::new(bytes).take(MAX_PACKAGE_BYTES));
    let mut entries = BTreeSet::new();
    for entry in archive.entries()? {
        check_cancel(cancel)?;
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file() || kind.is_dir(),
            "插件包只允许普通文件和目录。"
        );
        let path = entry.path_bytes();
        let path = std::str::from_utf8(&path)?;
        let path = if kind.is_dir() {
            path.trim_end_matches('/')
        } else {
            path
        };
        validate_path(path)?;
        ensure!(
            entries.insert(path.to_ascii_lowercase()) && entries.len() <= 2048,
            "插件归档路径重复或条目过多。"
        );
        let destination = root.join(path);
        if kind.is_dir() {
            fs::create_dir_all(destination)?;
        } else {
            fs::create_dir_all(destination.parent().context("插件归档路径无效。")?)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            let copied = io::copy(&mut entry, &mut file)?;
            ensure!(copied == entry.size(), "插件包中文件未完整解压。");
            #[cfg(unix)]
            if let Ok(mode) = entry.header().mode() {
                if mode & 0o111 != 0 {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&destination, fs::Permissions::from_mode(0o755));
                }
            }
        }
    }
    // Read the gzip trailer too, so checksum errors cannot be hidden by tar's end markers.
    let mut stream = archive.into_inner();
    io::copy(&mut stream, &mut io::sink())?;
    ensure!(stream.limit() > 0, "插件解压数据超过 256 MiB。");
    Ok(())
}

#[cfg(feature = "downloads")]
pub use remote::{Catalog, CatalogClient, CatalogEntry, CatalogPackage};

#[cfg(feature = "downloads")]
#[path = "plugins_catalog.rs"]
mod remote;

#[cfg(test)]
mod tests;

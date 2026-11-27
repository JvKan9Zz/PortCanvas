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
pub const ARENA_ID: &str = "arena-monitor";
pub const VISION_ID: &str = "vision-studio";
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;

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
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
}

impl Manifest {
    fn validate(&self) -> Result<()> {
        ensure!(self.schema == SCHEMA, "不支持此插件包格式。");
        validate_id(&self.id)?;
        for path in self.assets.values().chain(self.scripts.values()) {
            validate_path(path)?;
        }
        if self.id == ARENA_ID {
            ensure!(
                self.host_surface == "arena-v1"
                    && self.assets.contains_key("platformImage")
                    && ["cameraList", "cameraStream", "poseStream"]
                        .iter()
                        .all(|key| self.scripts.contains_key(*key)),
                "赛台插件缺少所需图片或脚本。"
            );
        }
        if self.id == VISION_ID {
            ensure!(
                self.host_surface == "vision-v1"
                    && ["visionInspect", "visionStream"].iter().all(|key| self.scripts.contains_key(*key)),
                "视觉实验台插件缺少所需脚本。"
            );
        }
        Ok(())
    }

    pub fn compatible(&self) -> bool {
        self.min_app_version
            <= Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package version")
    }
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
    state: Arc<Mutex<BTreeMap<String, CancellationToken>>>,
}

impl PluginStore {
    pub fn new(data_dir: &Path) -> Self {
        Self::from_root(data_dir.join("plugins"))
    }

    pub(crate) fn from_root(root: PathBuf) -> Self {
        Self { root, state: Arc::new(Mutex::new(BTreeMap::new())) }
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
        if let Some(expected) = expected {
            ensure!(&manifest == expected, "插件包清单与下载目录不一致。");
        }
        for path in manifest.assets.values().chain(manifest.scripts.values()) {
            ensure!(staged.join(path).is_file(), "插件包缺少文件 {path}。");
        }

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
        Ok(item)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
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
        Ok(())
    }

    pub fn uninstall(&self, id: &str) -> Result<()> {
        validate_id(id)?;
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
        Ok(item.manifest)
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
                .open(destination)?;
            let copied = io::copy(&mut entry, &mut file)?;
            ensure!(copied == entry.size(), "插件包中文件未完整解压。");
        }
    }
    // Read the gzip trailer too, so checksum errors cannot be hidden by tar's end markers.
    let mut stream = archive.into_inner();
    io::copy(&mut stream, &mut io::sink())?;
    ensure!(stream.limit() > 0, "插件解压数据超过 256 MiB。");
    Ok(())
}

#[cfg(feature = "downloads")]
pub use remote::{Catalog, CatalogClient, CatalogEntry};

#[cfg(feature = "downloads")]
#[path = "plugins_catalog.rs"]
mod remote;

#[cfg(test)]
mod tests;

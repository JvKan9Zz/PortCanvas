//! Remove retired application files while retaining the user's data directories.
use anyhow::{ensure, Context, Result};
use std::{fs, io::ErrorKind, path::{Component, Path, PathBuf}};

fn owned_path(root: &Path, relative: &Path) -> Result<Option<PathBuf>> {
    ensure!(root.is_absolute(), "清理根目录必须是绝对路径。");
    let base = fs::canonicalize(root)?;
    let mut target = base.clone();
    for part in relative.components() {
        ensure!(matches!(part, Component::Normal(_)), "清理目标不能离开应用目录。");
        target.push(part);
        let meta = match fs::symlink_metadata(&target) {
            Ok(meta) => meta,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        #[cfg(windows)]
        { use std::os::windows::fs::MetadataExt;
          ensure!(meta.file_attributes() & 0x400 == 0, "清理目标包含重定向目录：{}", target.display()); }
        ensure!(!meta.file_type().is_symlink(), "清理目标不能是符号链接。");
    }
    ensure!(target.starts_with(&base) && target != base, "清理目标不能是应用根目录。");
    Ok(Some(target))
}

fn cache(root: &Path, relative: &str) -> Result<()> {
    if let Some(path) = owned_path(root, Path::new(relative))? {
        ensure!(path.is_dir(), "旧缓存路径不是目录：{}", path.display());
        // std::fs does not traverse symlinks/junctions within the removed tree.
        fs::remove_dir_all(&path).with_context(|| format!("旧缓存仍被占用，原用户资料保留：{}", path.display()))?;
    }
    Ok(())
}

pub fn clean(install: &Path, local_data: &Path) -> Result<()> {
    // These are the actual WebView data locations used by the retired Tauri app.
    cache(local_data, "io.portcanvas.app/EBWebView")?;
    if owned_path(install, Path::new("data/devices.json"))?.is_some() {
        cache(install, "data/webview")?;
    }
    for entry in fs::read_dir(install)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let version_icon = name.strip_prefix("portcanvas-").and_then(|s| s.strip_suffix(".ico"))
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit() || c == b'.'));
        if version_icon {
            if let Some(path) = owned_path(install, Path::new(name))? {
                ensure!(path.is_file(), "旧图标路径不是文件。");
                fs::remove_file(path)?;
            }
        }
        if name.starts_with(".portcanvas-update-") && entry.file_type()?.is_dir() {
            clean_portable_stage(install, name)?;
        }
    }
    // The old bundle used LICENSE without an extension. Preserve a user's file
    // if it differs from the bundled license rather than deleting by name alone.
    if let Some(path) = owned_path(install, Path::new("LICENSE"))? {
        if path.is_file() && fs::read(&path)? == include_bytes!("../../../LICENSE") {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn clean_portable_stage(install: &Path, name: &str) -> Result<()> {
    let Some(plan) = owned_path(install, &Path::new(name).join("plan.json"))? else { return Ok(()) };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&fs::read(plan)?) else { return Ok(()) };
    let Some(target) = value["target"].as_str() else { return Ok(()) };
    if fs::canonicalize(target).ok() != Some(fs::canonicalize(install.join("PortCanvas.exe"))?) {
        return Ok(());
    }
    for file in ["previous.exe", "PortCanvas.exe", "plan.json"] {
        if let Some(path) = owned_path(install, &Path::new(name).join(file))? {
            ensure!(path.is_file(), "旧更新暂存内容类型异常。");
            fs::remove_file(path)?;
        }
    }
    let path = install.join(name);
    if fs::read_dir(&path)?.next().is_none() { fs::remove_dir(path)?; }
    Ok(())
}

/// Called after starting the installed app; a still-running installer is retried
/// on the next start. There is only one fixed package name, never a version pile.
pub fn finish_update(data: &Path) {
    finish_update_at(data, &std::env::temp_dir());
}

fn finish_update_at(data: &Path, temp: &Path) {
    if let Ok(Some(path)) = owned_path(data, Path::new("updates/PortCanvas-update.exe")) {
        if path.is_file() { let _ = fs::remove_file(path); }
    }
    // The retired Tauri updater used tempdir.keep() and exited without Drop.
    // Remove only its exact installer filename, retaining unknown contents.
    let _ = clean_updater_temp(temp);
}

fn clean_updater_temp(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some((version, suffix)) = name.strip_prefix("PortCanvas-").and_then(|s| s.split_once("-updater-")) else { continue };
        if suffix.is_empty() || !suffix.bytes().all(|c| c.is_ascii_alphanumeric()) { continue; }
        let Ok(version_number) = semver::Version::parse(version) else { continue };
        if version_number > semver::Version::parse(env!("CARGO_PKG_VERSION"))? { continue; }
        // A linked or occupied legacy cache is left for a subsequent start.
        let Ok(Some(directory)) = owned_path(root, Path::new(name)) else { continue };
        if !directory.is_dir() { continue; }
        let relative = Path::new(name).join(format!("PortCanvas-{version}-installer.exe"));
        if let Ok(Some(file)) = owned_path(root, &relative) {
            if file.is_file() { let _ = fs::remove_file(file); }
        }
        if fs::read_dir(&directory)?.next().is_none() { let _ = fs::remove_dir(directory); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Result<PathBuf> {
        let path = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").context("isolated test root required")?)
            .join(format!("cleanup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(path.join("install/data"))?;
        fs::create_dir_all(path.join("local"))?;
        Ok(path)
    }
    fn write(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
        let path=root.join(name);fs::create_dir_all(path.parent().unwrap())?;fs::write(path, bytes)?;Ok(())
    }
    #[test]
    fn upgrade_removes_runtime_caches_and_old_resources_but_preserves_mixed_user_data() -> Result<()> {
        let root=fixture()?;let install=root.join("install");let local=root.join("local");
        let preserved=["data/devices.json","data/settings.json","data/terminals.json","data/log-settings.json",
            "data/backgrounds/user.png","data/logs/output.log","data/migration-backups/user.json","my-program.py","portcanvas-custom.ico"];
        for name in preserved {write(&install,name,b"user bytes including credentials")?;}
        write(&install,"PortCanvas.exe",b"new native app")?;
        write(&install,"LICENSE",include_bytes!("../../../LICENSE"))?;
        write(&install,"portcanvas-0.5.8.ico",b"old icon")?;
        write(&install,"data/webview/EBWebView/Cache/old",b"old html cache")?;
        write(&local,"io.portcanvas.app/EBWebView/Default/Cache/old",b"old html cache")?;
        write(&local,"PortCanvas/Stable/devices.json",b"current user data")?;
        let stage=".portcanvas-update-test";
        write(&install,&format!("{stage}/plan.json"),&serde_json::to_vec(&serde_json::json!({"target":install.join("PortCanvas.exe")}))?)?;
        write(&install,&format!("{stage}/previous.exe"),b"retired app")?;
        write(&install,&format!("{stage}/PortCanvas.exe"),b"old staged app")?;
        clean(&install,&local)?;
        for name in preserved {assert_eq!(fs::read(install.join(name))?,b"user bytes including credentials");}
        assert_eq!(fs::read(install.join("PortCanvas.exe"))?,b"new native app");
        assert_eq!(fs::read(local.join("PortCanvas/Stable/devices.json"))?,b"current user data");
        for name in ["LICENSE","portcanvas-0.5.8.ico","data/webview",stage] {assert!(!install.join(name).exists());}
        assert!(!local.join("io.portcanvas.app/EBWebView").exists());
        write(&install,"updates/PortCanvas-update.exe",b"installed package")?;
        finish_update_at(&install,&local);
        assert!(!install.join("updates/PortCanvas-update.exe").exists());
        clean(&install,&local)?;
        fs::remove_dir_all(root)?;Ok(())
    }
    #[test]
    fn retired_updater_packages_are_removed_without_touching_future_updates_or_unknown_files() -> Result<()> {
        let root=fixture()?;let temp=root.join("temp");fs::create_dir(&temp)?;
        write(&temp,"PortCanvas-0.6.1-updater-owned/PortCanvas-0.6.1-installer.exe",b"old package")?;
        write(&temp,"PortCanvas-0.6.1-updater-mixed/PortCanvas-0.6.1-installer.exe",b"old package")?;
        write(&temp,"PortCanvas-0.6.1-updater-mixed/user.txt",b"keep")?;
        write(&temp,"PortCanvas-99.0.0-updater-future/PortCanvas-99.0.0-installer.exe",b"future package")?;
        write(&temp,"Other-0.6.1-updater-owned/Other-0.6.1-installer.exe",b"other app")?;
        clean_updater_temp(&temp)?;
        assert!(!temp.join("PortCanvas-0.6.1-updater-owned").exists());
        assert!(!temp.join("PortCanvas-0.6.1-updater-mixed/PortCanvas-0.6.1-installer.exe").exists());
        assert_eq!(fs::read(temp.join("PortCanvas-0.6.1-updater-mixed/user.txt"))?,b"keep");
        assert_eq!(fs::read(temp.join("PortCanvas-99.0.0-updater-future/PortCanvas-99.0.0-installer.exe"))?,b"future package");
        assert_eq!(fs::read(temp.join("Other-0.6.1-updater-owned/Other-0.6.1-installer.exe"))?,b"other app");
        fs::remove_dir_all(root)?;Ok(())
    }
    #[test]
    fn unknown_files_in_a_staging_directory_and_a_changed_license_are_retained() -> Result<()> {
        let root=fixture()?;let install=root.join("install");
        write(&install,"PortCanvas.exe",b"native")?;write(&install,"LICENSE",b"user notes")?;
        write(&install,".portcanvas-update-user/plan.json",b"not an application plan")?;
        write(&install,".portcanvas-update-user/notes.txt",b"user notes")?;
        clean(&install,&root.join("local"))?;
        assert_eq!(fs::read(install.join("LICENSE"))?,b"user notes");
        assert_eq!(fs::read(install.join(".portcanvas-update-user/notes.txt"))?,b"user notes");
        assert!(owned_path(&install,Path::new("../outside")).is_err());
        fs::remove_dir_all(root)?;Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn a_locked_cache_is_reported_without_deleting_user_data_or_old_icons() -> Result<()> {
        use std::os::windows::fs::OpenOptionsExt;
        let root=fixture()?;let install=root.join("install");let local=root.join("local");
        write(&install,"data/devices.json",b"credentials")?;write(&install,"portcanvas-0.5.8.ico",b"old")?;
        write(&local,"io.portcanvas.app/EBWebView/locked",b"cache")?;
        let lock=fs::OpenOptions::new().read(true).share_mode(1).open(local.join("io.portcanvas.app/EBWebView/locked"))?;
        assert!(clean(&install,&local).is_err());
        assert_eq!(fs::read(install.join("data/devices.json"))?,b"credentials");
        assert!(install.join("portcanvas-0.5.8.ico").exists());
        drop(lock);clean(&install,&local)?;fs::remove_dir_all(root)?;Ok(())
    }
}

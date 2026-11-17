//! OS directories. Pure path calculation never creates or migrates user data.
use anyhow::{ensure, Context, Result};
use portcanvas_core::services::Paths;
use std::path::{Path, PathBuf};
#[derive(Clone, Copy)]
pub enum Channel { Stable, Beta }
impl Channel {
    pub fn name(self) -> &'static str { match self { Self::Stable => "Stable", Self::Beta => "Beta" } }
    pub fn current() -> Self { if env!("CARGO_PKG_VERSION").contains("-beta.") { Self::Beta } else { Self::Stable } }
}
pub fn macos_paths(home: &Path, data: Option<PathBuf>, channel: Channel) -> Result<Paths> {
    ensure!(home.is_absolute(), "用户目录须为绝对路径。");
    let portable = data.is_some();
    let data = data.unwrap_or_else(||home.join("Library/Application Support/PortCanvas").join(channel.name()));
    ensure!(data.is_absolute(), "应用数据目录须为绝对路径。");
    Ok(Paths { data, documents: home.join("Documents"), local_files: home.join("Documents"), portable })
}
pub fn current() -> Result<Paths> {
    #[cfg(target_os="macos")]
    let mut paths = {
        let home = std::env::var_os("HOME").context("无法确定用户目录。")?;
        macos_paths(Path::new(&home), std::env::var_os("WCS_DATA_DIR").map(PathBuf::from), Channel::current())?
    };
    #[cfg(windows)]
    let mut paths = {
        use windows::Win32::UI::Shell::{FOLDERID_Documents, FOLDERID_LocalAppData};
        fn known_folder(id: &windows::core::GUID) -> Result<PathBuf> {
            use windows::Win32::{System::Com::CoTaskMemFree, UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT}};
            unsafe {
                let value = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
                let text = value.to_string(); CoTaskMemFree(Some(value.0 as *const _)); Ok(PathBuf::from(text?))
            }
        }
        let data = std::env::var_os("WCS_DATA_DIR").map(PathBuf::from).unwrap_or(known_folder(&FOLDERID_LocalAppData)?.join("PortCanvas").join(Channel::current().name()));
        ensure!(data.is_absolute(), "应用数据目录须为绝对路径。");
        let documents = known_folder(&FOLDERID_Documents)?;
        Paths { data, documents: documents.clone(), local_files: documents, portable: true }
    };
    #[cfg(not(any(windows, target_os="macos")))]
    anyhow::bail!("此原生客户端仅支持 Windows 与 macOS。");
    #[cfg(any(windows, target_os="macos"))]
    {
        if let Some(root) = std::env::var_os("WCS_TEST_ROOT").map(PathBuf::from) {
            ensure!(root.is_absolute(), "测试目录须为绝对路径。"); paths.local_files = root.join("local-files");
        }
        Ok(paths)
    }
}

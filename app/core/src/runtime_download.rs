use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub(crate) struct Plan {
    pub os: String,
    pub release: String,
    pub architecture: String,
    pub packages: Vec<Package>,
}
#[derive(Deserialize)]
pub(crate) struct Package {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub filename: String,
    pub size: u64,
    pub sha256: String,
}
impl Plan {
    pub fn validate(&self) -> Result<u64> {
        anyhow::ensure!(self.os == "ubuntu" && self.release == "20.04" && self.architecture == "arm64",
            "Beta 自动下载目前支持 Ubuntu 20.04 ARM64 控制器；其他环境请先手动准备组件。");
        anyhow::ensure!(!self.packages.is_empty() && self.packages.len() <= 128, "组件清单数量无效。");
        let mut total = 0;
        for package in &self.packages {
            anyhow::ensure!(matches!(package.architecture.as_str(), "arm64" | "all") &&
                !package.name.is_empty() && package.name.len() <= 256 && package.version.len() <= 256,
                "组件身份或架构无效。");
            anyhow::ensure!(package.filename.starts_with("pool/") && package.filename.ends_with(".deb") &&
                package.filename.len() <= 1024 &&
                package.filename.bytes().all(|c| c.is_ascii_alphanumeric() || b"/._+~-".contains(&c)) &&
                package.filename.split('/').all(|part| !part.is_empty() && part != "." && part != ".."),
                "组件下载路径无效。");
            anyhow::ensure!(package.sha256.len() == 64 && package.sha256.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) &&
                package.size > 0 && package.size <= 256 * 1024 * 1024, "组件校验信息无效。");
            total += package.size;
        }
        anyhow::ensure!(total <= 512 * 1024 * 1024, "所需组件超过 512 MiB，请手动准备。");
        Ok(total)
    }
}
impl Package {
    pub fn valid(&self, bytes: &[u8]) -> bool {
        bytes.len() as u64 == self.size && format!("{:x}", Sha256::digest(bytes)) == self.sha256
    }
}

pub(crate) async fn cached(
    package: &Package,
    directory: &Path,
    progress: impl Fn(usize, Option<u64>, usize) + Send + Sync,
) -> Result<PathBuf> {
    let path = directory.join(format!("{}.deb", package.sha256));
    if let Ok(bytes) = tokio::fs::read(&path).await {
        if package.valid(&bytes) {
            progress(bytes.len(), Some(package.size), 0);
            return Ok(path);
        }
    }
    #[cfg(not(feature = "downloads"))]
    anyhow::bail!("此构建不包含电脑下载组件，请使用桌面版。");
    #[cfg(feature = "downloads")]
    {
        let url = format!("https://ports.ubuntu.com/ubuntu-ports/{}", package.filename).parse()?;
        let client = crate::download::client_builder()
            .connect_timeout(std::time::Duration::from_secs(20))
            .timeout(std::time::Duration::from_secs(300))
            .https_only(true).build()?;
        let bytes = crate::download::fetch(&client, &url, progress).await
            .with_context(|| format!("电脑下载 {} 失败，请检查电脑联网后重试。", package.name))?;
        anyhow::ensure!(package.valid(&bytes), "组件 {} 的大小或 SHA-256 校验失败，请重新下载。", package.name);
        tokio::fs::create_dir_all(directory).await.context("无法创建电脑组件缓存目录。")?;
        let temporary = directory.join(format!(".download-{}", uuid::Uuid::new_v4()));
        // The synchronous commit is short and cannot leave a half-written published cache on cancellation.
        let result = (|| -> Result<()> {
            std::fs::write(&temporary, bytes)?;
            if path.exists() { std::fs::remove_file(&path)?; }
            std::fs::rename(&temporary, &path)?;
            Ok(())
        })();
        let _ = std::fs::remove_file(&temporary);
        result.context("无法保存电脑组件缓存，请检查磁盘空间和权限。")?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Plan {
        serde_json::from_value(serde_json::json!({"os":"ubuntu","release":"20.04","architecture":"arm64","packages":[{
            "name":"tmux","version":"3.0a-2","architecture":"arm64","filename":"pool/main/t/tmux/tmux_3.0a-2_arm64.deb","size":3,
            "sha256":format!("{:x}",Sha256::digest(b"abc"))}]})).unwrap()
    }
    #[test]
    fn package_metadata_cannot_supply_arbitrary_urls_or_paths() {
        assert_eq!(plan().validate().unwrap(),3);
        for filename in ["https://evil/file.deb","pool/../file.deb","pool/%2e/file.deb","pool/a?x.deb","pool//a.deb"] {
            let mut p=plan();p.packages[0].filename=filename.into();assert!(p.validate().is_err());
        }
        let mut p=plan();p.architecture="amd64".into();assert!(p.validate().is_err());
    }
    #[tokio::test]
    async fn valid_cache_is_reused_and_corruption_is_rejected() {
        let p=plan();let package=&p.packages[0];
        assert!(package.valid(b"abc"));assert!(!package.valid(b"abd"));assert!(!package.valid(b"ab"));
        let directory=std::env::temp_dir().join(format!("pc-cache-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(format!("{}.deb",package.sha256)),b"abc").unwrap();
        assert!(cached(package,&directory,|n,_,connections|{assert_eq!(n,3);assert_eq!(connections,0);}).await.unwrap().is_file());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(feature="downloads")]
    #[tokio::test]
    #[ignore = "Explicit controller integration: requires a captured APT plan and PC Internet"]
    async fn download_controller_plan_on_pc() {
        let source=std::path::PathBuf::from(std::env::var_os("PORTCANVAS_RUNTIME_PLAN").expect("explicit integration plan"));
        let plan: Plan=serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();plan.validate().unwrap();
        let directory=source.parent().unwrap().join("pc-cache");
        let mut records=Vec::new();
        for package in &plan.packages {
            let connections=std::sync::atomic::AtomicUsize::new(0);
            let path=cached(package,&directory,|_,_,count|{connections.fetch_max(count,std::sync::atomic::Ordering::Relaxed);}).await.unwrap();
            records.push(serde_json::json!({"name":package.name,"sha256":package.sha256,"size":package.size,"path":path,"connections":connections.load(std::sync::atomic::Ordering::Relaxed)}));
        }
        std::fs::write(source.with_extension("downloaded.json"),serde_json::to_vec_pretty(&records).unwrap()).unwrap();
    }
}

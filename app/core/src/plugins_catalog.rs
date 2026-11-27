use super::*;
use crate::download::{client_builder, fetch};
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use reqwest::{Client, Url};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub const DEFAULT_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/JvKan9Zz/PortCanvas/main/plugins/catalog.json";
const MAX_CATALOG_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub schema: String,
    pub plugins: Vec<CatalogEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPackage {
    pub download_url: String,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    #[serde(flatten)]
    pub manifest: Manifest,
    #[serde(default)]
    pub download_url: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub packages: BTreeMap<String, CatalogPackage>,
}

impl CatalogEntry {
    pub fn package_for_platform(&self, platform: &str) -> Option<CatalogPackage> {
        if !self.packages.is_empty() {
            if let Some(pkg) = self.packages.get(platform) {
                return Some(pkg.clone());
            }
            if let Some(pkg) = self.packages.get("any") {
                return Some(pkg.clone());
            }
            return None;
        }
        if !self.download_url.is_empty()
            && (self.manifest.platform.is_empty()
                || self.manifest.platform == "any"
                || self.manifest.platform == platform)
        {
            return Some(CatalogPackage {
                download_url: self.download_url.clone(),
                size_bytes: self.size_bytes,
                sha256: self.sha256.clone(),
                signature: self.signature.clone(),
            });
        }
        None
    }

    pub fn current_package(&self) -> Option<CatalogPackage> {
        self.package_for_platform(current_platform())
    }
}

pub struct CatalogClient {
    endpoint: Url,
    loopback: bool,
    public_key: String,
    client: Client,
}

impl CatalogClient {
    /// Only debug or explicitly opted-in builds accept a loopback test catalog.
    pub fn from_environment(public_key: &str) -> Result<Self> {
        #[cfg(any(debug_assertions, feature = "loopback-plugin-catalog"))]
        if let Ok(url) = std::env::var("PORTCANVAS_PLUGIN_CATALOG_URL") {
            return Self::new(&url, true, public_key);
        }
        Self::new(DEFAULT_CATALOG_URL, false, public_key)
    }

    fn new(endpoint: &str, loopback: bool, public_key: &str) -> Result<Self> {
        let endpoint = checked_url(endpoint, loopback)?;
        let mut builder = client_builder()
            .https_only(!loopback)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300));
        if loopback {
            builder = builder
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none());
        }
        Ok(Self {
            endpoint,
            loopback,
            public_key: public_key.trim().into(),
            client: builder.build()?,
        })
    }

    pub async fn refresh(&self, cancel: &CancellationToken) -> Result<Catalog> {
        let receive = async {
            let mut response = self
                .client
                .get(self.endpoint.clone())
                .timeout(Duration::from_secs(20))
                .send()
                .await?
                .error_for_status()?;
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                ensure!(
                    body.len() + chunk.len() <= MAX_CATALOG_BYTES,
                    "插件目录超过 1 MiB。"
                );
                body.extend_from_slice(&chunk);
            }
            let catalog: Catalog = serde_json::from_slice(&body).context("插件目录格式无效。")?;
            ensure!(catalog.schema == SCHEMA, "不支持此插件目录格式。");
            let mut seen = BTreeSet::new();
            for entry in &catalog.plugins {
                self.validate_entry(entry)?;
                ensure!(seen.insert(&entry.manifest.id), "插件目录有重复 ID。");
            }
            Ok(catalog)
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => bail!("插件操作已取消。"),
            result = receive => result,
        }
    }

    fn validate_package(&self, pkg: &CatalogPackage) -> Result<Url> {
        ensure!(
            (1..=MAX_PACKAGE_BYTES).contains(&pkg.size_bytes),
            "插件包大小无效。"
        );
        ensure!(
            pkg.sha256.len() == 64 && pkg.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "插件包 SHA-256 无效。"
        );
        checked_url(&pkg.download_url, self.loopback)
    }

    fn validate_entry(&self, entry: &CatalogEntry) -> Result<()> {
        entry.manifest.validate()?;
        if entry.packages.is_empty() {
            let pkg = CatalogPackage {
                download_url: entry.download_url.clone(),
                size_bytes: entry.size_bytes,
                sha256: entry.sha256.clone(),
                signature: entry.signature.clone(),
            };
            self.validate_package(&pkg)?;
        } else {
            for (platform, pkg) in &entry.packages {
                ensure!(
                    matches!(
                        platform.as_str(),
                        "any" | "windows-x86_64" | "darwin-universal"
                    ),
                    "不支持的目录平台标识：{platform}"
                );
                self.validate_package(pkg)?;
            }
        }
        Ok(())
    }

    pub async fn download_install(
        &self,
        store: &PluginStore,
        entry: &CatalogEntry,
        cancel: &CancellationToken,
        progress: impl Fn(usize, Option<u64>, usize) + Send + Sync,
    ) -> Result<InstalledPlugin> {
        self.validate_entry(entry)?;
        let pkg = entry
            .current_package()
            .context("此插件暂未提供当前操作系统的安装包。")?;
        let url = self.validate_package(&pkg)?;
        ensure!(
            entry.manifest.compatible(),
            "此插件需要更高版本的 PortCanvas。"
        );
        let bytes = tokio::select! {
            biased;
            _ = cancel.cancelled() => bail!("插件操作已取消。"),
            result = fetch(&self.client, &url, progress) => result?,
        };
        let store = store.clone();
        let entry = entry.clone();
        let cancel = cancel.clone();
        let public_key = self.public_key.clone();
        tokio::task::spawn_blocking(move || {
            check_cancel(&cancel)?;
            verify_package(&bytes, &pkg, &public_key)?;
            store.install(&bytes, Some(&entry.manifest), &cancel)
        })
        .await?
    }
}

fn checked_url(value: &str, loopback: bool) -> Result<Url> {
    let url = Url::parse(value)?;
    ensure!(
        url.host().is_some() && url.username().is_empty() && url.password().is_none(),
        "插件地址无效。"
    );
    if loopback {
        ensure!(
            url.scheme() == "http" && url.host_str() == Some("127.0.0.1"),
            "插件测试地址必须是本机回环地址。"
        );
    } else {
        ensure!(url.scheme() == "https", "插件地址必须使用 HTTPS。");
    }
    Ok(url)
}

fn minisign_text(value: &str) -> Result<String> {
    if value.starts_with("untrusted comment:") {
        Ok(value.to_owned())
    } else {
        Ok(String::from_utf8(STANDARD.decode(value)?)?)
    }
}

fn verify_package(bytes: &[u8], pkg: &CatalogPackage, public_key: &str) -> Result<()> {
    ensure!(bytes.len() as u64 == pkg.size_bytes, "插件包下载不完整。");
    ensure!(
        format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(&pkg.sha256),
        "插件包 SHA-256 校验失败。"
    );
    if let Some(signature) = &pkg.signature {
        let key = PublicKey::decode(&minisign_text(public_key)?)?;
        let signature = Signature::decode(&minisign_text(signature)?)?;
        key.verify(bytes, &signature, true)
            .context("插件包签名校验失败。")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "plugins/catalog_tests.rs"]
mod tests;

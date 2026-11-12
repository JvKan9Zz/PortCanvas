//! Native installer updates using the shared HTTPS transport and signed version contract.
use anyhow::{ensure, Context, Result};
use portcanvas_core::download::{client_builder, fetch};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const BETA_RELEASES: &str = "https://api.github.com/repos/JvKan9Zz/PortCanvas/releases?per_page=20";

#[derive(Clone)]
pub struct Channel {
    pub public_key: String,
    endpoint: String,
    loopback: bool,
}
impl Channel {
    pub fn beta(public_key: &str) -> Self {
        Self { public_key: public_key.trim().into(), endpoint: BETA_RELEASES.into(), loopback: false }
    }
    /// Disposable updater lab only. These hooks are absent from release builds.
    #[cfg(debug_assertions)]
    pub fn from_environment(public_key: &str) -> Result<Self> {
        let Ok(endpoint) = std::env::var("PORTCANVAS_UPDATE_TEST_ENDPOINT") else { return Ok(Self::beta(public_key)); };
        let key = std::env::var_os("PORTCANVAS_UPDATE_TEST_PUBLIC_KEY").context("更新实验缺少测试公钥文件。")?;
        let channel = Self { public_key: std::fs::read_to_string(key)?.trim().into(), endpoint, loopback: true };
        channel.url(&channel.endpoint)?;
        Ok(channel)
    }
    fn url(&self, value: &str) -> Result<()> {
        let url = reqwest::Url::parse(value)?;
        if self.loopback {
            ensure!(url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.username().is_empty() && url.password().is_none(), "更新测试地址必须是本机回环地址。");
        } else { secure_url(value)?; }
        Ok(())
    }
    fn client(&self, timeout: Duration) -> Result<reqwest::Client> {
        let builder = client_builder().https_only(!self.loopback).timeout(timeout);
        // The debug HTTP fixture cannot redirect outside its loopback boundary.
        Ok(if self.loopback { builder.no_proxy().redirect(reqwest::redirect::Policy::none()) } else { builder }.build()?)
    }
    pub async fn check(&self, current: &str, cancel: &CancellationToken) -> Result<Option<Plan>> {
        self.url(&self.endpoint)?;
        let client = self.client(Duration::from_secs(20))?;
        let operation = async {
            let response = client.get(&self.endpoint).send().await?.error_for_status()?;
            let value: Value = serde_json::from_str(&response.text().await?)?;
            let manifest = if self.loopback { value } else {
                let Some(url) = beta_manifest(&value, current)? else { return Ok(None); };
                let response = client.get(url).send().await?.error_for_status()?;
                serde_json::from_str(&response.text().await?)?
            };
            self.plan(&manifest, current)
        };
        tokio::select! { biased; _ = cancel.cancelled() => anyhow::bail!("更新已取消。"), result = operation => result }
    }
    fn plan(&self, value: &Value, current: &str) -> Result<Option<Plan>> {
        let Some(plan) = Plan::parse(value, current)? else { return Ok(None) };
        self.url(&plan.url)?;
        Ok(Some(plan))
    }
    pub async fn download(&self, plan: &Plan, current: &str, cancel: &CancellationToken, progress: impl Fn(usize, Option<u64>, usize) + Send + Sync) -> Result<Vec<u8>> {
        self.url(&plan.url)?;
        let client = self.client(Duration::from_secs(300))?;
        let url = plan.url.parse()?;
        let bytes = tokio::select! { biased; _ = cancel.cancelled() => anyhow::bail!("更新已取消。"), result = fetch(&client, &url, progress) => result? };
        ensure!(!cancel.is_cancelled(), "更新已取消。");
        crate::update::verify(&bytes, &self.public_key, &plan.signature, current, &plan.version)?;
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub signature: String,
}

fn secure_url(value: &str) -> Result<()> {
    ensure!(value.starts_with("https://"), "更新下载地址必须使用 HTTPS。");
    Ok(())
}

impl Plan {
    pub fn from_manifest(value: &Value, current: &str) -> Result<Option<Self>> {
        Channel::beta("").plan(value, current)
    }
    fn parse(value: &Value, current: &str) -> Result<Option<Self>> {
        let version = value["version"].as_str().context("更新信息缺少版本。")?;
        if Version::parse(version.trim_start_matches('v'))? <= Version::parse(current)? {
            return Ok(None);
        }
        let platform = &value["platforms"]["windows-x86_64"];
        let url = platform["url"].as_str().context("更新信息缺少 Windows 安装器。")?;
        Ok(Some(Self {
            version: version.trim_start_matches('v').to_owned(),
            notes: value["notes"].as_str().unwrap_or_default().to_owned(),
            url: url.to_owned(),
            signature: platform["signature"].as_str().context("更新信息缺少签名。")?.to_owned(),
        }))
    }

    pub async fn download(
        &self,
        public_key: &str,
        current: &str,
        cancel: &CancellationToken,
        progress: impl Fn(usize, Option<u64>, usize) + Send + Sync,
    ) -> Result<Vec<u8>> {
        Channel::beta(public_key).download(self, current, cancel, progress).await
    }
}

pub fn beta_manifest(releases: &Value, current: &str) -> Result<Option<String>> {
    let current = Version::parse(current)?;
    let mut selected = None;
    for release in releases.as_array().context("更新发布列表无效。")? {
        if release["draft"] == true || release["prerelease"] != true { continue; }
        let Some(tag) = release["tag_name"].as_str() else { continue };
        let Ok(version) = Version::parse(tag.trim_start_matches('v')) else { continue };
        if version <= current || !version.pre.as_str().starts_with("beta.") { continue; }
        let Some(assets) = release["assets"].as_array() else { continue };
        if let Some(url) = assets.iter().find(|a| a["name"] == "beta.json")
            .and_then(|a| a["browser_download_url"].as_str()) {
            secure_url(url)?;
            if selected.as_ref().is_none_or(|(latest, _)| version > *latest) {
                selected = Some((version, url.to_owned()));
            }
        }
    }
    Ok(selected.map(|(_, url)| url))
}

pub async fn check_beta(current: &str, cancel: &CancellationToken) -> Result<Option<Plan>> {
    Channel::beta("").check(current, cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn manifests_reject_downgrades_and_insecure_packages() {
        let value = json!({"version":"0.6.0-beta.2","platforms":{"windows-x86_64":{"url":"https://example.test/setup.exe","signature":"signed"}}});
        assert!(Plan::from_manifest(&value, "0.6.0-beta.1").unwrap().is_some());
        assert!(Plan::from_manifest(&value, "0.6.0-beta.2").unwrap().is_none());
        assert!(Plan::from_manifest(&value, "0.6.0").unwrap().is_none());
        let mut bad=value.clone();bad["platforms"]["windows-x86_64"]["url"]=json!("http://example.test/setup.exe");
        assert!(Plan::from_manifest(&bad,"0.6.0-beta.1").is_err());
    }
    #[test]
    fn beta_channel_never_selects_stable_draft_or_older_previews() {
        let release=|version:&str,pre:bool,draft:bool| json!({"tag_name":version,"prerelease":pre,"draft":draft,"assets":[{"name":"beta.json","browser_download_url":format!("https://example.test/{version}/beta.json")} ]});
        let releases=json!([release("v0.7.0",false,false),release("v0.6.0-beta.5",true,true),release("v0.6.0-beta.2",true,false),release("v0.6.0-beta.3",true,false)]);
        assert_eq!(beta_manifest(&releases,"0.6.0-beta.1").unwrap().as_deref(),Some("https://example.test/v0.6.0-beta.3/beta.json"));
        assert!(beta_manifest(&releases,"0.6.0-beta.3").unwrap().is_none());
    }
    #[tokio::test]
    async fn cancelled_check_has_no_network_dependency() {
        let cancel=CancellationToken::new();cancel.cancel();
        assert!(check_beta("0.6.0-beta.1",&cancel).await.unwrap_err().to_string().contains("取消"));
    }
    async fn fixture(tampered: bool, announced: &str) -> (Channel, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let f: Value = serde_json::from_str(include_str!("../tests/fixtures/update.json")).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/manifest", listener.local_addr().unwrap());
        let key = f["publicKey"].as_str().unwrap().to_owned();
        let manifest = json!({"version":announced,"notes":"native signed updater fixture","platforms":{"windows-x86_64":{"url":endpoint.replace("/manifest","/package"),"signature":f["signature"]}}}).to_string().into_bytes();
        let payload = if tampered { b"tampered package".to_vec() } else { f["payload"].as_str().unwrap().as_bytes().to_vec() };
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let manifest = manifest.clone(); let payload = payload.clone();
                tokio::spawn(async move {
                    let mut request = vec![];
                    while !request.ends_with(b"\r\n\r\n") { let b = stream.read_u8().await.unwrap(); request.push(b); }
                    let data = if request.starts_with(b"GET /manifest ") { manifest } else { payload };
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
                    stream.write_all(head.as_bytes()).await.unwrap();
                    let _ = stream.write_all(&data).await;
                });
            }
        });
        (Channel { public_key: key, endpoint, loopback: true }, task)
    }
    #[tokio::test]
    async fn signed_loopback_transport_checks_payload_version_and_downgrade() {
        for (tampered, version, valid) in [(false,"0.6.0",true),(true,"0.6.0",false),(false,"0.6.1",false)] {
            let (channel, task) = fixture(tampered, version).await;
            let cancel = CancellationToken::new();
            let plan = channel.check("0.5.95", &cancel).await.unwrap().unwrap();
            assert_eq!(channel.download(&plan,"0.5.95",&cancel,|_,_,_|{}).await.is_ok(), valid);
            assert!(channel.check("0.7.0", &cancel).await.unwrap().is_none());
            task.abort();
        }
    }
    #[tokio::test]
    async fn cancellation_during_download_does_not_return_an_installable_package() {
        let (channel, task) = fixture(false, "0.6.0").await;
        let cancel = CancellationToken::new();
        let plan = channel.check("0.5.95", &cancel).await.unwrap().unwrap();
        let stop = cancel.clone();
        let result = channel.download(&plan,"0.5.95",&cancel,move|_,_,_|stop.cancel()).await;
        assert!(result.is_err());
        task.abort();
    }
    #[test]
    fn loopback_lab_cannot_fetch_external_or_credentialed_urls() {
        let channel = Channel { public_key: String::new(), endpoint: String::new(), loopback: true };
        for url in ["http://example.test/package", "https://127.0.0.1/package", "http://user@127.0.0.1/package", "http://[::1]/package"] { assert!(channel.url(url).is_err()); }
        assert!(channel.url("http://127.0.0.1:1234/package").is_ok());
    }
}

use crate::profiles::{AuthMethod, Profile};
use anyhow::{bail, Context, Result};
use russh::{client, Channel, ChannelMsg};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub type SshChannel = Channel<client::Msg>;
pub type ConnectionProgress = Arc<dyn Fn(&'static str) + Send + Sync>;
pub type HostCheck = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send>>
        + Send
        + Sync,
>;
pub struct ClientHandler {
    pub check: HostCheck,
    pub rejected: Arc<Mutex<Option<String>>>,
    confirming: tokio::sync::watch::Sender<bool>,
    progress: ConnectionProgress,
}
impl client::Handler for ClientHandler {
    type Error = anyhow::Error;
    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool> {
        (self.progress)("identity");
        if key.certificate().is_some() {
            bail!("此版本尚未支持 SSH 主机证书认证，请使用受支持的主机公钥。");
        }
        let fingerprint = key
            .public_key()
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string();
        let _ = self.confirming.send(true);
        let result = (self.check)(fingerprint).await;
        let _ = self.confirming.send(false);
        match result {
            Ok(true) => { (self.progress)("secure"); Ok(true) },
            Ok(false) => {
                *self.rejected.lock().unwrap() = Some("主机身份未确认，已停止连接。".into());
                Ok(false)
            }
            Err(error) => {
                *self.rejected.lock().unwrap() = Some(error.to_string());
                Ok(false)
            }
        }
    }
}
pub struct SshConnection {
    pub handle: Arc<client::Handle<ClientHandler>>,
    pub profile: Profile,
    pub cancel: CancellationToken,
}
impl SshConnection {
    pub async fn connect(
        profile: Profile,
        secret: Zeroizing<String>,
        check: HostCheck,
        cancel: CancellationToken,
    ) -> Result<Self> {
        Self::connect_with_progress(profile, secret, check, cancel, Arc::new(|_| {})).await
    }
    pub async fn connect_with_progress(
        profile: Profile,
        secret: Zeroizing<String>,
        check: HostCheck,
        cancel: CancellationToken,
        progress: ConnectionProgress,
    ) -> Result<Self> {
        progress("prepare");
        let profile = profile.validate()?;
        let operation = async {
        let key = if profile.auth_method == AuthMethod::Key {
            let path = profile.private_key_path.clone();
            let passphrase = secret.clone();
            Some(tokio::task::spawn_blocking(move || {
                let meta=std::fs::metadata(&path).context("无法读取私钥文件，请检查路径和权限。")?;
                anyhow::ensure!(meta.is_file() && meta.len()<=1024*1024,"请选择最多1MiB的SSH私钥文件。");
                russh::keys::load_secret_key(&path, if passphrase.is_empty(){None}else{Some(passphrase.as_str())})
                    .map_err(|_|anyhow::anyhow!("无法解析私钥，请确认密钥格式及口令；支持OpenSSH、PEM和PuTTY PPK。"))
            }).await.context("私钥读取线程异常。")??)
        } else {None};
        let config = client::Config {
            keepalive_interval: Some(Duration::from_secs(5)),
            keepalive_max: 2,
            nodelay: true,
            channel_buffer_size: 32,
            ..Default::default()
        };
        let rejected = Arc::new(Mutex::new(None));
        let (confirming, confirmation) = tokio::sync::watch::channel(false);
        let handler = ClientHandler {
            check,
            rejected: rejected.clone(),
            confirming,
            progress: progress.clone(),
        };
        let network = async {
            progress("resolve");
            let addresses = tokio::net::lookup_host((profile.host.as_str(), profile.port))
                .await.context("设备地址解析失败，请检查主机名。")?.collect::<Vec<_>>();
            anyhow::ensure!(!addresses.is_empty(), "设备地址未解析到可用的 IP。");
            progress("tcp");
            let stream = tokio::net::TcpStream::connect(addresses.as_slice()).await
                .map_err(|error| anyhow::anyhow!(friendly(&error.to_string())))?;
            stream.set_nodelay(true)?;
            progress("handshake");
            let mut handle = client::connect_stream(
                Arc::new(config),
                stream,
                handler,
            )
            .await
            .map_err(|e| {
                anyhow::anyhow!(rejected
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| friendly(&e.to_string())))
            })?;
            progress("authenticate");
            let authenticated=if let Some(key)=key {
                let hash=handle.best_supported_rsa_hash().await?.flatten();
                handle.authenticate_publickey(&profile.username,russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key),hash)).await?.success()
            } else {handle.authenticate_password(&profile.username,secret.as_str()).await?.success()};
            if !authenticated { bail!("{}", if profile.auth_method==AuthMethod::Key {"密钥认证失败：请检查用户名、公钥授权和私钥。"}else{"认证失败：请检查用户名和密码。"}); }
            Ok(Self {
                handle: Arc::new(handle),
                profile,
                cancel: cancel.clone(),
            })
        };
        network_timeout(network,confirmation).await
        };
        tokio::select! {biased; _ = cancel.cancelled()=>bail!("连接已取消。"),result=operation=>result}
    }
    pub async fn channel(&self) -> Result<SshChannel> {
        tokio::select! {_ = self.cancel.cancelled()=>bail!("设备已断开。"),result=tokio::time::timeout(Duration::from_secs(10),self.handle.channel_open_session())=>Ok(result.context("SSH通道打开超时。")??)}
    }
    pub async fn exec(
        &self,
        command: &str,
        limit: usize,
        timeout: Duration,
    ) -> Result<(String, i32)> {
        let mut channel = self.channel().await?;
        channel.exec(true, command).await?;
        let operation = async {
            let mut output = Vec::new();
            let mut diagnostic = Vec::new();
            let mut status = None;
            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Data { data } => {
                        if output.len() + data.len() > limit {
                            bail!("远端响应超过允许大小。");
                        }
                        output.extend_from_slice(&data);
                    }
                    ChannelMsg::ExtendedData { data, .. } => {
                        if diagnostic.len() < 6000 {
                            diagnostic.extend_from_slice(
                                &data[..data.len().min(6000 - diagnostic.len())],
                            );
                        }
                    }
                    ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status as i32),
                    _ => {}
                }
            }
            let status = status.context("远端命令未返回退出状态。")?;
            if status != 0 && output.is_empty() {
                bail!("{}", String::from_utf8_lossy(&diagnostic).trim());
            }
            Ok((
                String::from_utf8(output).context("远端输出不是 UTF-8 文本。")?,
                status,
            ))
        };
        let result = tokio::select! {_ = self.cancel.cancelled()=>Err(anyhow::anyhow!("设备连接已断开。")),result=tokio::time::timeout(timeout,operation)=>result.context("远端命令超时。")?};
        let _ = channel.close().await;
        result
    }
    pub async fn sftp(&self) -> Result<russh_sftp::client::SftpSession> {
        let channel = self.channel().await?;
        channel.request_subsystem(true, "sftp").await?;
        Ok(russh_sftp::client::SftpSession::new(channel.into_stream()).await?)
    }
    pub async fn disconnect(&self) {
        self.cancel.cancel();
        let _ = self
            .handle
            .disconnect(russh::Disconnect::ByApplication, "Studio closed", "en")
            .await;
    }
}
async fn network_timeout<T>(
    operation: impl std::future::Future<Output = Result<T>>,
    mut confirmation: tokio::sync::watch::Receiver<bool>,
) -> Result<T> {
    tokio::pin!(operation);
    let deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(deadline);
    let mut open = true;
    loop {
        tokio::select! {biased;
            result=&mut operation=>return result,
            changed=confirmation.changed(),if open=>{open=changed.is_ok();if open && !*confirmation.borrow(){deadline.as_mut().reset(tokio::time::Instant::now()+Duration::from_secs(30));}},
            _=&mut deadline,if !*confirmation.borrow()=>bail!("连接超时：请检查网络和设备。")
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn identity_confirmation_does_not_consume_network_deadline() {
        let (confirming, receiver) = tokio::sync::watch::channel(false);
        let operation = async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            confirming.send(true)?;
            tokio::time::sleep(Duration::from_secs(65)).await;
            confirming.send(false)?;
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(7)
        };
        assert_eq!(network_timeout(operation, receiver).await.unwrap(), 7);
        let (_confirming, receiver) = tokio::sync::watch::channel(false);
        let stalled = std::future::pending::<Result<()>>();
        assert!(network_timeout(stalled, receiver)
            .await
            .unwrap_err()
            .to_string()
            .contains("连接超时"));
    }
}
pub fn friendly(message: &str) -> String {
    if message.to_lowercase().contains("auth") {
        "认证失败：请检查用户名和密码。".into()
    } else if message.contains("10061") || message.to_lowercase().contains("refused") {
        "连接被拒绝：请确认 SSH 服务和端口。".into()
    } else if message.contains("10060") || message.to_lowercase().contains("timed out") {
        "连接超时：请检查设备与网络。".into()
    } else {
        message.to_string()
    }
}
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

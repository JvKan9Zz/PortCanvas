use crate::{desktop::python, ssh::{quote, SshConnection}};
use anyhow::{Context, Result};
use russh::ChannelMsg;
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashSet, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
pub enum Feature { Desktop, Terminal, Camera, Files }
impl Feature {
    fn name(self) -> &'static str {
        match self { Self::Desktop => "desktop", Self::Terminal => "terminal", Self::Camera => "camera", Self::Files => "files" }
    }
}
#[derive(Default)]
pub(crate) struct State {
    completed: HashSet<Feature>,
    bin_dir: Option<String>,
    tmux: Option<String>,
    pub sftp: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    ready: bool,
    feature: Option<String>,
    #[serde(default)] message: String,
    bin_dir: Option<String>,
    tmux: Option<String>,
    sftp: Option<String>,
}
impl SshConnection {
    pub fn remote_command(&self, command: &str) -> String {
        let state = self.environment.lock().unwrap();
        let mut prefix = String::new();
        if let Some(directory) = &state.bin_dir { prefix += &format!("export PATH={}:\"$PATH\"; ", quote(directory)); }
        if let Some(tmux) = &state.tmux { prefix += &format!("export PORTCANVAS_TMUX={}; ", quote(tmux)); }
        prefix + command
    }
    pub async fn prepare(&self, feature: Feature, cancel: &CancellationToken) -> Result<()> {
        if !self.auto_prepare { return Ok(()); }
        let _guard = tokio::select! {
            _ = cancel.cancelled() => anyhow::bail!("运行环境准备已取消。"),
            guard = self.preparation.lock() => guard,
        };
        if self.environment.lock().unwrap().completed.contains(&feature) { return Ok(()); }
        let command = python(include_str!("../resources/prepare_environment.py"), json!(["environment", json!({"feature":feature.name()}).to_string()]))?;
        let mut channel = self.channel().await?;
        let operation = async {
            anyhow::ensure!(!cancel.is_cancelled(), "运行环境准备已取消。");
            channel.exec(true, command).await?;
            let mut output = Vec::new();
            let mut diagnostic = Vec::new();
            let mut status = None;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
            loop {
                let message = tokio::select! {
                    _ = heartbeat.tick() => { channel.data(&b"."[..]).await?; continue; },
                    message = channel.wait() => message,
                };
                match message {
                    Some(ChannelMsg::Data { data }) => {
                        anyhow::ensure!(output.len() + data.len() <= 65536, "运行环境检查响应过长。");
                        output.extend_from_slice(&data);
                    },
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let count = data.len().min(4000 - diagnostic.len());
                        diagnostic.extend_from_slice(&data[..count]);
                    },
                    Some(ChannelMsg::ExitStatus { exit_status }) => status = Some(exit_status),
                    None => break,
                    _ => {},
                }
            }
            let text = std::str::from_utf8(&output)?;
            let packet = text.lines().find_map(|line| line.strip_prefix("PORTCANVAS_ENV="))
                .with_context(|| format!("无法准备远端运行环境，请确认 Python 3 可用。{}", String::from_utf8_lossy(&diagnostic).trim()))?;
            let reply: Reply = serde_json::from_str(packet).context("运行环境检查响应无效。")?;
            anyhow::ensure!(reply.ready, "{}", reply.message);
            anyhow::ensure!(status == Some(0) && reply.feature.as_deref() == Some(feature.name()), "运行环境准备未正常完成。");
            for path in [&reply.bin_dir, &reply.tmux, &reply.sftp].into_iter().flatten() {
                anyhow::ensure!(path.starts_with('/') && path.len() <= 4096 && !path.contains('\0'), "运行环境路径无效。");
            }
            Ok::<_, anyhow::Error>(reply)
        };
        let result = tokio::select! {
            _ = cancel.cancelled() => Err(anyhow::anyhow!("运行环境准备已取消。")),
            result = tokio::time::timeout(Duration::from_secs(180), operation) => result.context("运行环境准备超时，请检查树莓派联网和软件源。").and_then(|result| result),
        };
        let _ = channel.eof().await;
        let _ = tokio::time::timeout(Duration::from_secs(3), async { while channel.wait().await.is_some() {} }).await;
        let _ = channel.close().await;
        let reply = result?;
        let mut state = self.environment.lock().unwrap();
        state.bin_dir = reply.bin_dir;
        state.tmux = reply.tmux;
        state.sftp = reply.sftp;
        state.completed.insert(feature);
        Ok(())
    }
}

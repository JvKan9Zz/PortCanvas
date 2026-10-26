use crate::{desktop::python, runtime_download::{self, Plan}, ssh::{quote, SshChannel, SshConnection}};
use anyhow::{Context, Result};
use russh::ChannelMsg;
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashSet, sync::Mutex, time::{Duration, Instant}};
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
    fn environment_event(&self, feature: Feature, value: serde_json::Value) {
        let mut value = value;
        value["feature"] = json!(feature.name());
        (self.environment_progress)("environment-progress", value);
    }
    async fn send_packages(&self, channel: &mut SshChannel, feature: Feature, plan: Plan) -> Result<()> {
        let total = plan.validate()?;
        let directory = self.runtime_cache.as_deref().context("电脑组件缓存未配置，请使用桌面版。")?;
        let mut paths = Vec::new();
        let mut completed = 0u64;
        for package in &plan.packages {
            self.environment_event(feature, json!({"phase":"downloading","package":package.name,"done":completed,"total":total,"connections":null}));
            let counter = Mutex::new((completed, Instant::now() - Duration::from_secs(1), 0usize));
            let future = runtime_download::cached(package, directory, |length, _, connections| {
                let mut counter = counter.lock().unwrap();
                counter.0 += length as u64;
                counter.2 = connections;
                if counter.1.elapsed() >= Duration::from_millis(100) {
                    self.environment_event(feature, json!({"phase":"downloading","package":package.name,"done":counter.0,"total":total,"connections":connections}));
                    counter.1 = Instant::now();
                }
            });
            tokio::pin!(future);
            let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
            let path = loop {
                tokio::select! {
                    result = &mut future => break result?,
                    _ = heartbeat.tick() => channel.data(&b".\n"[..]).await?,
                }
            };
            paths.push(path);
            completed += package.size;
            self.environment_event(feature, json!({"phase":"downloading","package":package.name,"done":completed,"total":total,"connections":counter.lock().unwrap().2}));
        }
        let mut sent = 0u64;
        let mut last = Instant::now() - Duration::from_secs(1);
        for (index, (package, path)) in plan.packages.iter().zip(paths).enumerate() {
            let bytes = tokio::fs::read(path).await.context("无法读取电脑组件缓存。")?;
            anyhow::ensure!(package.valid(&bytes), "电脑组件缓存已变化，请重新准备。");
            let header = format!("{}\n", json!({"index":index,"size":package.size}));
            channel.data(header.as_bytes()).await?;
            for chunk in bytes.chunks(64 * 1024) {
                channel.data(chunk).await?;
                sent += chunk.len() as u64;
                if last.elapsed() >= Duration::from_millis(100) || sent == total {
                    self.environment_event(feature, json!({"phase":"transferring","package":package.name,"done":sent,"total":total}));
                    last = Instant::now();
                }
            }
        }
        self.environment_event(feature, json!({"phase":"preparing"}));
        Ok(())
    }
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
        let user_cancel = CancellationToken::new();
        *self.environment_cancel.lock().unwrap() = user_cancel.clone();
        let command = python(include_str!("../resources/prepare_environment.py"), json!(["environment", json!({"feature":feature.name()}).to_string()]))?;
        let mut channel = self.channel().await?;
        let operation = async {
            anyhow::ensure!(!cancel.is_cancelled(), "运行环境准备已取消。");
            channel.exec(true, command).await?;
            let mut output = Vec::new();
            let mut diagnostic = Vec::new();
            let mut status = None;
            let mut planned = false;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
            loop {
                let message = tokio::select! {
                    _ = heartbeat.tick() => { channel.data(&b".\n"[..]).await?; continue; },
                    message = channel.wait() => message,
                };
                match message {
                    Some(ChannelMsg::Data { data }) => {
                        anyhow::ensure!(output.len() + data.len() <= 65536, "运行环境检查响应过长。");
                        output.extend_from_slice(&data);
                        if !planned {
                            let text = String::from_utf8_lossy(&output);
                            if let Some(packet) = text.split_inclusive('\n').filter(|line| line.ends_with('\n'))
                                .find_map(|line| line.trim_end().strip_prefix("PORTCANVAS_ENV_PLAN=")) {
                                let plan: Plan = serde_json::from_str(packet).context("组件下载清单无效。")?;
                                planned = true;
                                self.send_packages(&mut channel, feature, plan).await?;
                            }
                        }
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
            _ = self.cancel.cancelled() => Err(anyhow::anyhow!("设备已断开。")),
            _ = user_cancel.cancelled() => Err(anyhow::anyhow!("运行环境准备已取消。")),
            result = tokio::time::timeout(Duration::from_secs(900), operation) => result.context("运行环境准备超时，请检查电脑网络和 SSH 连接。").and_then(|result| result),
        };
        let _ = channel.eof().await;
        let _ = tokio::time::timeout(Duration::from_secs(3), async { while channel.wait().await.is_some() {} }).await;
        let _ = channel.close().await;
        self.environment_event(feature, match &result {
            Ok(_) => json!({"phase":"ready"}),
            Err(error) => json!({"phase":if user_cancel.is_cancelled() || cancel.is_cancelled() {"canceled"} else {"error"},"message":error.to_string()}),
        });
        let reply = result?;
        let mut state = self.environment.lock().unwrap();
        state.bin_dir = reply.bin_dir;
        state.tmux = reply.tmux;
        if feature == Feature::Files { state.sftp = reply.sftp; }
        state.completed.insert(feature);
        Ok(())
    }
}

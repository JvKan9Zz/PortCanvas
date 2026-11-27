//! Vision Studio owns one generation-bound SSH stream and its latest JPEG.
use crate::{
    arena::{JpegFrames, NativeCameraFrame},
    desktop::python,
    plugins::{stop_with_plugin, PluginStore, VISION_ID},
    session::DeviceManager,
    ssh::SshChannel,
};
use anyhow::{bail, ensure, Context, Result};
use russh::{ChannelMsg, Sig};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const MAX_CONTROL: usize = 8192;
const TELEMETRY: &str = "PORTCANVAS_VISION:";

struct Running {
    cancel: CancellationToken,
    done: CancellationToken,
    config: watch::Sender<Value>,
}

struct State {
    id: u64,
    generation: u64,
    phase: &'static str,
    telemetry: Value,
    message: String,
    sequence: u64,
    jpeg: Option<Arc<Vec<u8>>>,
    viewer: Instant,
    running: Option<Running>,
}

#[derive(Clone)]
pub struct VisionService {
    devices: DeviceManager,
    plugins: PluginStore,
    state: Arc<Mutex<State>>,
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl VisionService {
    pub fn new(devices: DeviceManager, plugins: PluginStore) -> Self {
        Self {
            devices,
            plugins,
            state: Arc::new(Mutex::new(State {
                id: 0,
                generation: 0,
                phase: "idle",
                telemetry: Value::Null,
                message: String::new(),
                sequence: 0,
                jpeg: None,
                viewer: Instant::now(),
                running: None,
            })),
            gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn inspect_for_generation(
        &self,
        expected: u64,
        request: &CancellationToken,
    ) -> Result<Value> {
        let (script, plugin) = self.plugins.plugin_script(VISION_ID, "visionInspect")?;
        let (generation, connection) = self.devices.connection_with_generation()?;
        ensure!(expected == generation, "设备连接已变化。");
        let pid = {
            let state = self.state.lock().unwrap();
            (state.generation == generation && state.running.is_some())
                .then(|| state.telemetry["pid"].as_u64())
                .flatten()
        };
        let command = python(
            &script,
            json!([
                "vision-inspect",
                pid.map(|pid| pid.to_string()).unwrap_or_default()
            ]),
        )?;
        let cancel = stop_with_plugin(plugin, request);
        let _guard = cancel.clone().drop_guard();
        let (body, _) = connection
            .exec_cancellable(&command, 160_000, Duration::from_secs(6), &cancel)
            .await?;
        ensure!(
            expected == self.devices.generation() && !cancel.is_cancelled(),
            "设备连接已变化。"
        );
        serde_json::from_str(&body).context("视觉巡检数据无效。")
    }

    pub async fn start(
        &self,
        config: Value,
        expected: u64,
        request: CancellationToken,
    ) -> Result<u64> {
        check_config(&config)?;
        let _gate = tokio::select! {
            biased;
            _ = request.cancelled() => bail!("视觉采集已取消。"),
            gate = self.gate.lock() => gate,
        };
        let (inspector, inspector_token) =
            self.plugins.plugin_script(VISION_ID, "visionInspect")?;
        let (stream, plugin) = self.plugins.plugin_script(VISION_ID, "visionStream")?;
        ensure!(!inspector_token.is_cancelled(), "视觉插件已关闭。");
        self.stop_inner(None).await;
        let (generation, connection) = self.devices.connection_with_generation()?;
        ensure!(
            generation == expected && !request.is_cancelled(),
            "设备连接已变化。"
        );
        let command = python(
            &format!("{inspector}\n{stream}"),
            json!(["vision-stream", config.to_string()]),
        )?;
        let cancel = stop_with_plugin(plugin, &connection.cancel);
        let done = CancellationToken::new();
        let (send_config, recv_config) = watch::channel(config);
        let id = {
            let mut state = self.state.lock().unwrap();
            let id = state.id.wrapping_add(1);
            *state = State {
                id,
                generation,
                phase: "connecting",
                message: String::new(),
                telemetry: Value::Null,
                sequence: 0,
                jpeg: None,
                viewer: Instant::now(),
                running: Some(Running {
                    cancel: cancel.clone(),
                    done: done.clone(),
                    config: send_config,
                }),
            };
            id
        };

        let active = self.clone();
        tokio::spawn(async move {
            let _done = done.drop_guard();
            let _cancel = cancel.clone().drop_guard();
            let result = async {
                let mut channel = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Ok(()),
                    _ = request.cancelled() => return Ok(()),
                    channel = connection.channel() => channel?,
                };
                let command = connection.remote_command(&format!("exec {command}"));
                let result = active
                    .stream(id, &mut channel, command, recv_config, &cancel, &request)
                    .await;
                close(&mut channel).await;
                result
            }
            .await;
            let mut state = active.state.lock().unwrap();
            if state.id == id && state.running.is_some() {
                state.running = None;
                state.jpeg = None;
                state.telemetry = Value::Null;
                state.phase = if cancel.is_cancelled() || request.is_cancelled() {
                    "idle"
                } else {
                    "error"
                };
                state.message = result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default();
            }
        });
        Ok(id)
    }

    pub fn update_config(&self, config: Value) -> Result<()> {
        check_config(&config)?;
        let state = self.state.lock().unwrap();
        ensure!(
            state.generation == self.devices.generation(),
            "设备连接已变化。"
        );
        state
            .running
            .as_ref()
            .context("视觉采集未开启。")?
            .config
            .send_replace(config);
        Ok(())
    }

    pub fn native_frame(
        &self,
        id: u64,
        after: u64,
        generation: u64,
    ) -> Result<Option<NativeCameraFrame>> {
        let mut state = self.state.lock().unwrap();
        ensure!(
            state.id == id
                && state.generation == generation
                && generation == self.devices.generation(),
            "设备连接已变化。"
        );
        state.viewer = Instant::now();
        Ok(state
            .jpeg
            .as_ref()
            .filter(|_| state.sequence > after)
            .map(|jpeg| NativeCameraFrame {
                sequence: state.sequence,
                generation,
                jpeg: jpeg.clone(),
            }))
    }

    pub fn status(&self) -> Value {
        let state = self.state.lock().unwrap();
        let current = state.generation == self.devices.generation();
        json!({"id":state.id,"generation":state.generation,
            "phase":if current {state.phase}else{"idle"},
            "message":if current {state.message.as_str()}else{""},
            "sequence":state.sequence,"telemetry":if current {&state.telemetry}else{&Value::Null}})
    }

    pub async fn stop(&self, id: Option<u64>) {
        let _gate = self.gate.lock().await;
        self.stop_inner(id).await;
    }

    async fn stop_inner(&self, id: Option<u64>) {
        let old = {
            let mut state = self.state.lock().unwrap();
            if id.is_some_and(|id| id != state.id) {
                return;
            }
            state.phase = "idle";
            state.jpeg = None;
            state.telemetry = Value::Null;
            state.message.clear();
            state.running.take()
        };
        if let Some(old) = old {
            old.cancel.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(4), old.done.cancelled()).await;
        }
    }

    async fn stream(
        &self,
        id: u64,
        channel: &mut SshChannel,
        command: String,
        mut config: watch::Receiver<Value>,
        cancel: &CancellationToken,
        request: &CancellationToken,
    ) -> Result<()> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(()),
            _ = request.cancelled() => return Ok(()),
            result = tokio::time::timeout(Duration::from_secs(3), channel.exec(true, command)) => result??,
        }
        let mut parser = JpegFrames::default();
        let mut diagnostics = Diagnostics::default();
        let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
        let deadline = tokio::time::sleep(Duration::from_secs(12));
        tokio::pin!(deadline);
        loop {
            let output = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                _ = request.cancelled() => return Ok(()),
                _ = &mut deadline => bail!("视觉采集响应超时。"),
                message = channel.wait() => match message {
                    Some(ChannelMsg::Data { data }) => {
                        let frames = parser.push(&data)?;
                        if let Some(jpeg) = frames.into_iter().last() {
                            deadline.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(12));
                            let mut state = self.state.lock().unwrap();
                            if state.id != id || state.running.is_none() { return Ok(()); }
                            state.sequence += 1;
                            state.jpeg = Some(Arc::new(jpeg));
                            state.phase = "connected";
                            Some(vec![b'r'])
                        } else { None }
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        if let Some(value) = diagnostics.feed(&data)? {
                            deadline.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(12));
                            let mut state = self.state.lock().unwrap();
                            if state.id != id || state.running.is_none() { return Ok(()); }
                            if value["width"] == 0 {
                                state.phase = "waiting";
                                state.jpeg = None;
                            }
                            state.telemetry = value;
                        }
                        None
                    }
                    Some(ChannelMsg::ExitStatus { .. }) | None => {
                        let message = diagnostics.message.trim();
                        bail!("{}", if message.is_empty() { "视觉采集已结束。" } else { message });
                    }
                    _ => None,
                },
                _ = heartbeat.tick() => {
                    if self.state.lock().unwrap().viewer.elapsed() > Duration::from_secs(6) {
                        return Ok(());
                    }
                    Some(vec![b'.'])
                }
                changed = config.changed() => {
                    if changed.is_err() { return Ok(()); }
                    Some(config_line(&config.borrow_and_update())?)
                }
            };
            if let Some(output) = output {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Ok(()),
                    _ = request.cancelled() => return Ok(()),
                    sent = tokio::time::timeout(Duration::from_secs(2), channel.data(&output[..])) => sent??,
                }
            }
        }
    }
}

async fn close(channel: &mut SshChannel) {
    if tokio::time::timeout(Duration::from_secs(2), async {
        let _ = channel.eof().await;
        while channel.wait().await.is_some() {}
    })
    .await
    .is_err()
    {
        let _ = tokio::time::timeout(Duration::from_millis(500), channel.signal(Sig::TERM)).await;
    }
    let _ = tokio::time::timeout(Duration::from_millis(500), channel.close()).await;
}

fn config_line(config: &Value) -> Result<Vec<u8>> {
    ensure!(config.is_object(), "视觉配置无效。");
    let mut bytes = serde_json::to_vec(&json!({"cmd":"config","config":config}))?;
    bytes.push(b'\n');
    ensure!(bytes.len() <= MAX_CONTROL, "视觉控制消息过长。");
    Ok(bytes)
}

fn check_config(config: &Value) -> Result<()> {
    config_line(config).map(|_| ())
}

#[derive(Default)]
struct Diagnostics {
    pending: Vec<u8>,
    message: String,
}

impl Diagnostics {
    fn feed(&mut self, bytes: &[u8]) -> Result<Option<Value>> {
        let mut latest = None;
        for piece in bytes.split_inclusive(|byte| *byte == b'\n') {
            ensure!(
                self.pending.len() + piece.len() <= 16_384,
                "视觉遥测数据过长。"
            );
            self.pending.extend_from_slice(piece);
            if piece.ends_with(b"\n") {
                let line = String::from_utf8_lossy(&self.pending);
                if let Some(value) = line.trim_end().strip_prefix(TELEMETRY) {
                    let value: Value = serde_json::from_str(value).context("视觉遥测数据无效。")?;
                    ensure!(value.is_object(), "视觉遥测数据无效。");
                    latest = Some(value);
                } else if !line.trim().is_empty() {
                    self.message = line.trim().chars().take(320).collect();
                }
                self.pending.clear();
            }
        }
        Ok(latest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_and_diagnostics_survive_protocol_chunk_boundaries() -> Result<()> {
        let line = config_line(&json!({"algo": 3, "source": "watch"}))?;
        assert!(line.ends_with(b"\n"));
        let decoded: Value = serde_json::from_slice(&line)?;
        assert_eq!(decoded["config"]["algo"], 3);
        assert!(config_line(&json!({"large":"x".repeat(MAX_CONTROL)})).is_err());

        let mut diagnostics = Diagnostics::default();
        assert!(diagnostics.feed(b"notice\nPORTCANVAS_VIS")?.is_none());
        let packet = diagnostics
            .feed(b"ION:{\"fps\":15}\nPORTCANVAS_VISION:{\"fps\":8}\n")?
            .unwrap();
        assert_eq!(diagnostics.message, "notice");
        assert_eq!(packet["fps"], 8);
        Ok(())
    }
}

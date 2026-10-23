use crate::{desktop::python, profiles::Profile, session::DeviceManager, ssh::SshConnection};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

pub type OutputSink = Arc<dyn Fn(Vec<u8>) -> Result<()> + Send + Sync>;
pub fn session_name(profile: &Profile, namespace: Option<&str>) -> String {
    let base = profile.terminal_name();
    let base = if let Some(namespace) = namespace {
        let hash = Sha256::digest(format!("{base}:{namespace}").as_bytes());
        format!(
            "wcs-{}",
            hash.iter()
                .take(8)
                .map(|n| format!("{n:02x}"))
                .collect::<String>()
        )
    } else {
        base
    };
    format!("{base}-logs")
}
pub fn dimensions(cols: u32, rows: u32) -> Result<()> {
    if !(2..=1000).contains(&cols) || !(1..=500).contains(&rows) {
        bail!("终端尺寸无效。");
    }
    Ok(())
}
pub struct Flow {
    credits: Arc<Semaphore>,
    pending: Mutex<BTreeMap<u64, OwnedSemaphorePermit>>,
    next: AtomicU64,
}
impl Flow {
    fn new() -> Self {
        Self {
            credits: Arc::new(Semaphore::new(256 * 1024)),
            pending: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(0),
        }
    }
    async fn output(
        &self,
        id: u64,
        bytes: &[u8],
        sink: &OutputSink,
        cancel: &CancellationToken,
    ) -> Result<()> {
        for chunk in bytes.chunks(32768) {
            if chunk.is_empty() {
                continue;
            }
            let permit = tokio::select! {_=cancel.cancelled()=>bail!("终端显示已断开。"),result=tokio::time::timeout(Duration::from_secs(15),self.credits.clone().acquire_many_owned(chunk.len() as u32))=>result.context("终端显示处理超时，远端会话保留，可重新连接。")??};
            let sequence = self.next.fetch_add(1, Ordering::Relaxed) + 1;
            self.pending.lock().unwrap().insert(sequence, permit);
            let mut frame = Vec::with_capacity(16 + chunk.len());
            frame.extend_from_slice(&id.to_le_bytes());
            frame.extend_from_slice(&sequence.to_le_bytes());
            frame.extend_from_slice(chunk);
            sink(frame)?;
        }
        Ok(())
    }
    fn ack(&self, sequence: u64) {
        self.pending.lock().unwrap().remove(&sequence);
    }
    fn clear(&self) {
        self.pending.lock().unwrap().clear();
    }
}
enum Input {
    Text(String),
    Resize(u32, u32),
}
struct Inner {
    id: u64,
    state: Value,
    cancel: CancellationToken,
    tx: Option<mpsc::Sender<Input>>,
    flow: Arc<Flow>,
    pending: Option<watch::Sender<Option<std::result::Result<Value, String>>>>,
}
#[derive(Clone)]
pub struct TerminalService {
    inner: Arc<Mutex<Inner>>,
    devices: DeviceManager,
    namespace: Option<String>,
    logs: Option<crate::logs::LogService>,
}
impl TerminalService {
    pub fn new(
        devices: DeviceManager,
        namespace: Option<String>,
        logs: Option<crate::logs::LogService>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                id: 0,
                state: json!({"phase":"disconnected","id":0,"message":"连接设备后可打开终端。"}),
                cancel: CancellationToken::new(),
                tx: None,
                flow: Arc::new(Flow::new()),
                pending: None,
            })),
            devices,
            namespace,
            logs,
        }
    }
    pub fn snapshot(&self) -> Value {
        self.inner.lock().unwrap().state.clone()
    }
    pub fn name(&self, profile: &Profile) -> String {
        session_name(profile, self.namespace.as_deref())
    }
    fn emit(&self, state: Value) {
        (self.devices.events)("terminal-state", state);
    }
    pub fn stop(&self) {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            inner.cancel.cancel();
            inner.id += 1;
            inner.tx = None;
            inner.flow.clear();
            if let Some(pending) = inner.pending.take() {
                pending.send_replace(Some(Err("终端连接已取消。".into())));
            }
            inner.state = json!({"phase":"disconnected","id":inner.id,"generation":self.devices.generation(),"message":"终端已断开，远端会话保留。"});
            inner.state.clone()
        };
        self.emit(state);
    }
    pub async fn start(&self, cols: u32, rows: u32, sink: OutputSink) -> Result<Value> {
        dimensions(cols, rows)?;
        let (generation, connection) = self.devices.connection_with_generation()?;
        let (mut receiver, owner) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.state["phase"] == "connected"
                && !inner.cancel.is_cancelled()
                && inner.state["generation"] == generation
            {
                return Ok(inner.state.clone());
            }
            if let Some(pending) = &inner.pending {
                (pending.subscribe(), None)
            } else {
                inner.cancel.cancel();
                inner.id += 1;
                let id = inner.id;
                let cancel = connection.cancel.child_token();
                let flow = Arc::new(Flow::new());
                let (tx, rx) = mpsc::channel(64);
                let (pending, receiver) = watch::channel(None);
                inner.pending = Some(pending);
                inner.cancel = cancel.clone();
                inner.flow = flow.clone();
                inner.tx = Some(tx);
                inner.state = json!({"phase":"connecting","id":id,"generation":generation,"session":self.name(&connection.profile),"message":"正在恢复远端终端…"});
                (receiver, Some((id, cancel, flow, rx, inner.state.clone())))
            }
        };
        if let Some((id, cancel, flow, rx, state)) = owner {
            self.emit(state);
            let service = self.clone();
            tokio::spawn(async move {
                let result = service
                    .run(
                        connection,
                        generation,
                        id,
                        cols,
                        rows,
                        cancel.clone(),
                        flow,
                        rx,
                        sink,
                    )
                    .await;
                service.finished(id, result, cancel.is_cancelled());
            });
        }
        loop {
            let result = { receiver.borrow().clone() };
            if let Some(result) = result {
                return result.map_err(anyhow::Error::msg);
            }
            receiver.changed().await.context("终端准备通道已关闭。")?;
        }
    }
    fn connected(&self, id: u64, generation: u64) -> Result<()> {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            if inner.id != id
                || inner.cancel.is_cancelled()
                || generation != self.devices.generation()
            {
                bail!("终端连接已取消。");
            }
            inner.state["phase"] = json!("connected");
            inner.state["message"] = json!("终端已连接，收起和切页保留会话。");
            if let Some(pending) = inner.pending.take() {
                pending.send_replace(Some(Ok(inner.state.clone())));
            }
            inner.state.clone()
        };
        self.emit(state);
        Ok(())
    }
    fn finished(&self, id: u64, result: Result<()>, canceled: bool) {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            if inner.id != id {
                return;
            }
            let error = result.err().map(|e| e.to_string());
            inner.cancel.cancel();
            inner.tx = None;
            inner.flow.clear();
            if let Some(pending) = inner.pending.take() {
                pending.send_replace(Some(Err(error
                    .clone()
                    .unwrap_or_else(|| "终端在就绪前关闭。".into()))));
            }
            inner.id += 1;
            inner.state["id"] = json!(inner.id);
            inner.state["generation"] = json!(self.devices.generation());
            inner.state["phase"] = json!(if error.is_some() && !canceled {
                "error"
            } else {
                "disconnected"
            });
            inner.state["message"] = json!(if canceled {
                "设备或终端已断开，远端会话保留。".into()
            } else {
                error.unwrap_or_else(|| "终端已退出，可重新连接。".into())
            });
            inner.state.clone()
        };
        self.emit(state);
    }
    async fn run(
        &self,
        connection: Arc<SshConnection>,
        generation: u64,
        id: u64,
        cols: u32,
        rows: u32,
        cancel: CancellationToken,
        flow: Arc<Flow>,
        mut input: mpsc::Receiver<Input>,
        sink: OutputSink,
    ) -> Result<()> {
        let name = self.name(&connection.profile);
        let command=python(include_str!("../resources/log-bootstrap.py"),json!(["logs",json!({"session":name,"source":include_str!("../resources/log-shell.bash").replace("\r\n","\n")}).to_string()]))?;
        let (output, _) = tokio::select! {_=cancel.cancelled()=>bail!("终端连接已取消。"),result=connection.exec(&command,250000,Duration::from_secs(12))=>result?};
        let info: Value = serde_json::from_str(output.trim())?;
        anyhow::ensure!(info["session"] == name, "远端终端身份不匹配。");
        if let Some(logs) = &self.logs {
            let _ = logs.attach(connection.clone(), generation, info).await;
        }
        if cancel.is_cancelled() {
            bail!("终端连接已取消。");
        }
        let mut channel = connection.channel().await?;
        channel
            .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
            .await?;
        channel
            .exec(
                true,
                format!("exec tmux -L wcs-terminal -f /dev/null attach-session -t {name}"),
            )
            .await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut success = 0;
        let mut live = false;
        let operation=async{loop{tokio::select!{
            _=cancel.cancelled()=>break,
            _=tokio::time::sleep_until(deadline),if !live=>bail!("终端连接超时。"),
            request=input.recv()=>{match request{Some(Input::Text(text))=>channel.data(text.as_bytes()).await?,Some(Input::Resize(cols,rows))=>channel.window_change(cols,rows,0,0).await?,None=>break,}},
            message=channel.wait()=>match message{Some(russh::ChannelMsg::Success)=>{success+=1;if success>=2&&!live{self.connected(id,generation)?;live=true;}},Some(russh::ChannelMsg::Failure)=>bail!("远端拒绝PTY或终端启动请求。"),Some(russh::ChannelMsg::Data{data})|Some(russh::ChannelMsg::ExtendedData{data,..})=>flow.output(id,&data,&sink,&cancel).await?,Some(russh::ChannelMsg::ExitStatus{exit_status})if exit_status!=0=>bail!("终端进程已退出，状态码 {exit_status}。"),None=>break,_=>{}}
        }}Ok(())}.await;
        let _ = channel.eof().await;
        let _ = channel.close().await;
        operation
    }
    fn sender(&self, id: u64) -> Result<mpsc::Sender<Input>> {
        let inner = self.inner.lock().unwrap();
        if inner.id != id || inner.state["phase"] != "connected" || inner.cancel.is_cancelled() {
            bail!("终端连接已失效，请重新连接。");
        }
        inner.tx.clone().context("终端连接已关闭。")
    }
    pub async fn write(&self, id: u64, text: String) -> Result<()> {
        if text.len() > 65536 {
            bail!("单次输入过长，请分段粘贴。");
        }
        tokio::time::timeout(
            Duration::from_secs(2),
            self.sender(id)?.send(Input::Text(text)),
        )
        .await
        .context("终端输入队列繁忙。")??;
        Ok(())
    }
    pub async fn resize(&self, id: u64, cols: u32, rows: u32) -> Result<()> {
        dimensions(cols, rows)?;
        tokio::time::timeout(
            Duration::from_secs(2),
            self.sender(id)?.send(Input::Resize(cols, rows)),
        )
        .await
        .context("终端尺寸队列繁忙。")??;
        Ok(())
    }
    pub fn ack(&self, id: u64, sequence: u64) {
        let inner = self.inner.lock().unwrap();
        if inner.id == id {
            inner.flow.ack(sequence);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_preserves_default_and_isolates_tests() {
        let data: crate::profiles::ProfileData =
            serde_json::from_str(include_str!("../tests/fixtures/devices.json")).unwrap();
        let p = data.devices[0].clone();
        assert_eq!(session_name(&p, None), "wcs-a96d1d05b24621e4-logs");
        assert_ne!(session_name(&p, None), session_name(&p, Some("test")));
    }
    #[tokio::test]
    async fn binary_frames_and_ack_release_only_owned_credits() {
        let flow = Flow::new();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let target = bytes.clone();
        let sink: OutputSink = Arc::new(move |data| {
            target.lock().unwrap().push(data);
            Ok(())
        });
        flow.output(7, b"abc", &sink, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(flow.credits.available_permits(), 256 * 1024 - 3);
        assert_eq!(&bytes.lock().unwrap()[0][16..], b"abc");
        flow.ack(999);
        assert_eq!(flow.credits.available_permits(), 256 * 1024 - 3);
        flow.ack(1);
        assert_eq!(flow.credits.available_permits(), 256 * 1024);
        assert!(dimensions(1, 30).is_err());
    }
}

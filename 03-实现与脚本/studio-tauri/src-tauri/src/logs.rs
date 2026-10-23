use crate::{
    log_protocol::{control_output, packet, Event, Marker, OutputParser},
    log_store::{LogStore, Query},
    profiles::atomic_json,
    session::DeviceManager,
    ssh::SshConnection,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub root: PathBuf,
    #[serde(default)]
    pub projects: BTreeMap<String, String>,
}
impl Settings {
    pub fn validate(self) -> Result<Self> {
        if !self.root.is_absolute() {
            bail!("日志根目录须为电脑本地绝对路径。");
        }
        if self
            .projects
            .values()
            .any(|v| v.chars().count() > 100 || v.chars().any(|c| c < ' '))
        {
            bail!("日志项目设置无效。");
        }
        Ok(self)
    }
}
struct Observer {
    serial: u64,
    generation: u64,
    name: String,
    cancel: CancellationToken,
    done: CancellationToken,
    _snapshot: mpsc::Sender<Marker>,
}
struct Inner {
    settings: Settings,
    file: PathBuf,
    invalid: bool,
    store: Arc<Mutex<LogStore>>,
    state: Value,
    serial: u64,
    observer: Option<Observer>,
}
#[derive(Clone)]
pub struct LogService {
    inner: Arc<Mutex<Inner>>,
    lifecycle: Arc<tokio::sync::Mutex<()>>,
    devices: DeviceManager,
}
#[derive(Clone)]
struct Capture {
    device: String,
    context: Option<Marker>,
    manual: String,
    current: Option<(String, String)>,
    session: Option<(String, String)>,
    segment: u64,
    last: Option<(String, u64, u8)>,
}
impl Capture {
    fn key(&self, marker: &Marker) -> String {
        format!("{}:{}:{}", self.device, marker.shell, marker.sequence)
    }
    fn marker(
        &mut self,
        store: &mut LogStore,
        marker: Marker,
        manual: &str,
        recovered: bool,
    ) -> Result<bool> {
        let sequence = marker.sequence.parse::<u64>().unwrap_or(0);
        let rank = match marker.kind.as_str() {
            "ready" => 0,
            "begin" => 1,
            _ => 2,
        };
        if let Some((shell, last, previous)) = &self.last {
            if shell == &marker.shell && (sequence < *last || sequence == *last && rank < *previous)
                || recovered && shell != &marker.shell
            {
                return Ok(false);
            }
        }
        self.last = Some((marker.shell.clone(), sequence, rank));
        if marker.kind == "ready" {
            if let Some((_, id)) = self.current.take() {
                store.finish(&id, None, Some("Shell会话已变化。"))?;
            }
            let mut context = marker;
            context.project_root.clear();
            self.context = Some(context);
            self.manual = manual.into();
            return Ok(false);
        }
        let key = self.key(&marker);
        self.context = Some(marker.clone());
        if marker.kind == "begin" {
            if self
                .current
                .as_ref()
                .is_some_and(|(source, _)| source == &key)
            {
                return Ok(false);
            }
            self.manual = manual.into();
            if let Some((_, id)) = self.current.take() {
                store.finish(&id, None, Some("未收到上一命令的结束标记。"))?;
            }
            if let Some(id) = store.begin(&marker, &self.device, manual, recovered, "command")? {
                if recovered {
                    if let Some(original) = store.manual_for(&id) {
                        self.manual = original;
                    }
                }
                self.current = Some((key, id));
            }
        } else if self
            .current
            .as_ref()
            .is_some_and(|(source, _)| source == &key)
        {
            let (_, id) = self.current.take().unwrap();
            store.finish(&id, Some(&marker), None)?;
        } else {
            store.recover_ended(&marker, &self.device, manual)?;
        }
        Ok(true)
    }
    fn events(
        &mut self,
        store: &mut LogStore,
        events: Vec<Event>,
        manual: &str,
        recovered: bool,
    ) -> Result<bool> {
        let mut changed = false;
        for event in events {
            match event {
                Event::Marker(marker) => {
                    changed |= self.marker(store, marker, manual, recovered)?
                }
                Event::Text(text) => {
                    if let Some((_, id)) = &self.current {
                        store.append(id, &text)?;
                    }
                    let context = self.context.clone().unwrap_or(Marker {
                        kind: "ready".into(),
                        shell: "0-0".into(),
                        sequence: "0".into(),
                        code: None,
                        command: String::new(),
                        cwd: String::new(),
                        project_root: String::new(),
                        end_cwd: String::new(),
                        background_jobs: vec![],
                    });
                    let group = crate::log_store::project(&context, &self.manual)["key"]
                        .as_str()
                        .unwrap()
                        .to_string();
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|(current, _)| current != &group)
                    {
                        let (_, id) = self.session.take().unwrap();
                        store.finish(&id, None, None)?;
                        changed = true;
                    }
                    if self.session.is_none() {
                        self.segment += 1;
                        let marker = Marker {
                            shell: format!(
                                "session-{}-{}",
                                std::process::id(),
                                chrono::Utc::now().timestamp_millis()
                            ),
                            sequence: self.segment.to_string(),
                            command: "[终端会话输出]".into(),
                            ..context
                        };
                        if let Some(id) =
                            store.begin(&marker, &self.device, &self.manual, false, "session")?
                        {
                            self.session = Some((group, id));
                            changed = true;
                        }
                    }
                    if let Some((_, id)) = &self.session {
                        store.append(id, &text)?;
                    }
                }
            }
        }
        store.flush()?;
        Ok(changed)
    }
}
impl LogService {
    pub fn new(devices: DeviceManager, file: PathBuf, default_root: PathBuf) -> Result<Self> {
        let mut invalid = false;
        let mut message = "连接终端后自动记录，日志保存在电脑本地。".to_string();
        let settings = if file.exists() {
            match fs::metadata(&file)
                .map_err(anyhow::Error::from)
                .and_then(|meta| {
                    if !meta.is_file() || meta.len() > 1024 * 1024 {
                        bail!("日志设置文件无效或超过1MiB。");
                    }
                    Ok(fs::read(&file)?)
                })
                .map_err(anyhow::Error::from)
                .and_then(|b| serde_json::from_slice::<Settings>(&b).map_err(Into::into))
                .and_then(Settings::validate)
            {
                Ok(settings) => settings,
                Err(_) => {
                    invalid = true;
                    message = "日志设置读取失败，当前使用默认目录；原文件保留。".into();
                    Settings {
                        root: default_root,
                        projects: BTreeMap::new(),
                    }
                }
            }
        } else {
            Settings {
                root: default_root,
                projects: BTreeMap::new(),
            }
        };
        let store = Arc::new(Mutex::new(LogStore::new(settings.root.clone())?));
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                settings,
                file,
                invalid,
                store,
                state: json!({"phase":if invalid{"error"}else{"idle"},"message":message,"active":0,"notice":""}),
                serial: 0,
                observer: None,
            })),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            devices,
        })
    }
    pub fn settings(&self) -> Settings {
        self.inner.lock().unwrap().settings.clone()
    }
    pub fn refresh_status(&self) {
        self.update(None, json!({}));
    }
    pub fn snapshot(&self) -> Value {
        let key = self
            .devices
            .connection()
            .ok()
            .map(|c| c.profile.terminal_name())
            .unwrap_or_default();
        let inner = self.inner.lock().unwrap();
        let mut state = inner.state.clone();
        state["root"] = json!(inner.settings.root);
        state["project"] = json!(inner
            .settings
            .projects
            .get(&key)
            .cloned()
            .unwrap_or_default());
        state
    }
    fn update(&self, serial: Option<u64>, patch: Value) {
        let snapshot = {
            let mut inner = self.inner.lock().unwrap();
            if serial.is_some_and(|serial| serial != inner.serial) {
                return;
            }
            for (k, v) in patch.as_object().unwrap() {
                inner.state[k] = v.clone();
            }
            drop(inner);
            self.snapshot()
        };
        (self.devices.events)("logs-state", snapshot);
    }
    fn changed(&self) {
        (self.devices.events)("logs-changed", Value::Null);
    }
    async fn store<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut LogStore) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let store = self.inner.lock().unwrap().store.clone();
        tokio::task::spawn_blocking(move || operation(&mut store.lock().unwrap()))
            .await
            .context("本地日志工作线程异常。")?
    }
    fn manual(&self, device: &str) -> String {
        self.inner
            .lock()
            .unwrap()
            .settings
            .projects
            .get(device)
            .cloned()
            .unwrap_or_default()
    }
    pub fn detach(&self, reason: &str) {
        let inner = self.inner.lock().unwrap();
        if let Some(observer) = &inner.observer {
            observer.cancel.cancel();
        }
        let failed = inner.state["phase"] == "error";
        drop(inner);
        if failed {
            self.refresh_status();
        } else {
            self.update(None, json!({"phase":"idle","message":reason}));
        }
    }
    async fn stop_observer(&self) -> Result<()> {
        let observer = {
            let inner = self.inner.lock().unwrap();
            inner
                .observer
                .as_ref()
                .map(|o| (o.serial, o.cancel.clone(), o.done.clone()))
        };
        if let Some((serial, cancel, done)) = observer {
            cancel.cancel();
            tokio::time::timeout(Duration::from_secs(10), done.cancelled())
                .await
                .context("旧日志仍在封存，请稍后重试。")?;
            let mut inner = self.inner.lock().unwrap();
            if inner.observer.as_ref().is_some_and(|o| o.serial == serial) {
                inner.observer = None;
            }
        }
        Ok(())
    }
    pub async fn close(&self) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        if let Err(error) = self.stop_observer().await {
            self.update(None, json!({"phase":"error","message":error.to_string()}));
            return Err(error);
        }
        self.store(|store| store.close("软件或设备连接已关闭。"))
            .await?;
        if self.inner.lock().unwrap().state["phase"] != "error" {
            self.update(
                None,
                json!({"phase":"idle","active":0,"message":"记录已封存。"}),
            );
        }
        Ok(())
    }
    pub async fn attach(
        &self,
        connection: Arc<SshConnection>,
        generation: u64,
        info: Value,
    ) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        if generation != self.devices.generation() || connection.cancel.is_cancelled() {
            bail!("日志连接已变化。");
        }
        if self.inner.lock().unwrap().invalid {
            bail!("日志设置无效，请先在日志页修复目录或设置；终端仍可使用。");
        }
        let name = info["session"]
            .as_str()
            .context("日志会话缺失")?
            .to_string();
        let pane = info["pane"].as_str().context("日志pane缺失")?.to_string();
        if !pane.starts_with('%')
            || pane.len() < 2
            || !pane[1..].bytes().all(|b| b.is_ascii_digit())
        {
            bail!("日志pane无效。");
        }
        {
            let inner = self.inner.lock().unwrap();
            if inner.observer.as_ref().is_some_and(|o| {
                o.generation == generation && o.name == name && !o.cancel.is_cancelled()
            }) {
                return Ok(());
            }
        }
        if let Err(error) = self.stop_observer().await {
            self.update(None, json!({"phase":"error","message":error.to_string()}));
            return Err(error);
        }
        if connection.cancel.is_cancelled() {
            bail!("日志连接已取消。");
        }
        let serial = {
            let mut inner = self.inner.lock().unwrap();
            inner.serial += 1;
            inner.serial
        };
        self.update(
            Some(serial),
            json!({"phase":"connecting","message":"正在准备本地运行记录…"}),
        );
        let opened = connection.channel().await;
        let mut channel = match opened {
            Ok(channel) => channel,
            Err(error) => {
                if generation == self.devices.generation() {
                    self.update(Some(serial),json!({"phase":"error","message":format!("日志通道无法打开：{error}。终端仍可使用。")}));
                }
                return Err(error);
            }
        };
        let executed = channel
            .exec(
                true,
                format!("exec tmux -L wcs-terminal -C attach-session -t {name}"),
            )
            .await;
        if let Err(error) = executed {
            let _ = channel.close().await;
            self.update(Some(serial),json!({"phase":"error","message":format!("日志通道启动失败：{error}。终端仍可使用。" )}));
            return Err(error.into());
        }
        let cancel = connection.cancel.child_token();
        let done = CancellationToken::new();
        let (tx, mut snapshots) = mpsc::channel::<Marker>(2);
        let (ready, receiver) = oneshot::channel();
        let device = connection.profile.terminal_name();
        let context = info["packet"].as_str().and_then(packet);
        let service = self.clone();
        let worker_cancel = cancel.clone();
        let worker_done = done.clone();
        self.inner.lock().unwrap().observer = Some(Observer {
            serial,
            generation,
            name,
            cancel: cancel.clone(),
            done: done.clone(),
            _snapshot: tx.clone(),
        });
        let observed_pane = pane.clone();
        tokio::spawn(async move {
            let mut ready = Some(ready);
            let mut parser = OutputParser::default();
            let mut buffer = Vec::new();
            let initial_manual = service.manual(&device);
            let mut capture = Capture {
                device: device.clone(),
                context,
                manual: initial_manual,
                current: None,
                session: None,
                segment: 0,
                last: None,
            };
            let operation=async{loop{tokio::select!{_=worker_cancel.cancelled()=>break,
                snapshot=snapshots.recv()=>{if let Some(snapshot)=snapshot{let events=vec![Event::Marker(snapshot)];let manual=service.manual(&device);let mut next=capture.clone();let (updated,changed,active)=service.store(move|store|{let changed=next.events(store,events,&manual,true)?;Ok((next,changed,store.active()))}).await?;capture=updated;service.update(Some(serial),json!({"active":active}));if changed{service.changed();}}},
                message=channel.wait()=>match message{Some(russh::ChannelMsg::Data{data})=>{buffer.extend_from_slice(&data);if buffer.len()>2*1024*1024{bail!("日志控制行过长，已停止本次记录。");}let mut events=Vec::new();while let Some(end)=buffer.iter().position(|b|*b==b'\n'){let mut line=buffer.drain(..=end).collect::<Vec<_>>();line.pop();if line.last()==Some(&b'\r'){line.pop();}if line.starts_with(b"%end "){if let Some(ready)=ready.take(){let _=ready.send(());}}else if line.starts_with(b"%error "){bail!("设备拒绝日志通道。");}else if line.starts_with(b"%output "){let rest=&line[8..];if let Some(space)=rest.iter().position(|b|*b==b' '){if &rest[..space]==observed_pane.as_bytes(){events.extend(parser.push(&control_output(&rest[space+1..])));}}}}
                    if !events.is_empty(){let manual=service.manual(&device);let mut next=capture.clone();let (updated,changed,active)=service.store(move|store|{let changed=next.events(store,events,&manual,false)?;Ok((next,changed,store.active()))}).await?;capture=updated;if changed{service.update(Some(serial),json!({"active":active}));service.changed();}}},Some(russh::ChannelMsg::Failure)=>bail!("日志通道启动被拒绝。"),None=>bail!("日志通道已关闭，缺失输出不会补造。"),_=>{}}
            }}Ok::<_,anyhow::Error>(())}.await;
            let events = parser.flush();
            let manual = service.manual(&device);
            let mut tail = capture;
            let reason = operation
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "软件或设备连接已关闭，未收到的输出保留为缺失。".into());
            let final_reason = reason.clone();
            let ending = service
                .store(move |store| {
                    let tail_result = tail.events(store, events, &manual, false);
                    let close = store.close(&final_reason);
                    tail_result?;
                    close
                })
                .await;
            let _ = channel.eof().await;
            let _ = channel.close().await;
            let canceled = worker_cancel.is_cancelled();
            let ending_failed = ending.is_err();
            worker_cancel.cancel();
            let error = operation.err().or_else(|| ending.err());
            if !canceled || ending_failed {
                service.update(Some(serial),json!({"phase":"error","active":0,"message":format!("本地记录已停止：{}。终端仍可使用。",error.map(|e|e.to_string()).unwrap_or(reason))}));
            } else {
                service.update(Some(serial),json!({"phase":"idle","active":0,"message":"连接已结束，日志已封存；中断期间输出未采集。"}));
            }
            service.changed();
            worker_done.cancel();
        });
        let setup = tokio::time::timeout(Duration::from_secs(15), async {
            tokio::time::timeout(Duration::from_secs(8), receiver)
                .await
                .context("日志通道准备超时。")??;
            let command = format!(
                "tmux -L wcs-terminal show-option -wqv -t {} @wcs_run | base64 -d",
                crate::ssh::quote(&pane)
            );
            let (mut value, _) = connection
                .exec(&command, 250000, Duration::from_secs(8))
                .await?;
            let mut marker = packet(value.trim());
            for _ in 0..20 {
                if marker.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                value = connection
                    .exec(&command, 250000, Duration::from_secs(8))
                    .await?
                    .0;
                marker = packet(value.trim());
            }
            let marker = marker.context("日志Shell标记未就绪。")?;
            tx.send(marker).await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("日志通道准备超时。")
        .and_then(|result| result);
        if let Err(error) = &setup {
            cancel.cancel();
            if generation == self.devices.generation() {
                self.update(
                Some(serial),
                json!({"phase":"error","message":format!("日志未就绪：{error}。终端仍可使用。" )}),
            );
            }
            return setup;
        }
        if cancel.is_cancelled() || generation != self.devices.generation() {
            bail!("日志通道已结束。");
        }
        self.update(
            Some(serial),
            json!({"phase":"ready","message":"自动记录已就绪，日志保存在电脑本地。"}),
        );
        Ok(())
    }
    pub async fn page(&self, query: Query) -> Result<Value> {
        self.store(move |s| s.list(query)).await
    }
    pub async fn detail(&self, id: String) -> Result<Value> {
        self.store(move |s| s.detail(&id)).await
    }
    pub async fn removable(&self, ids: Vec<String>) -> Result<Vec<Value>> {
        self.store(move |s| s.removable(&ids)).await
    }
    pub async fn remove(&self, ids: Vec<String>) -> Result<Value> {
        let result = self.store(move |s| s.remove(&ids)).await?;
        self.changed();
        Ok(result)
    }
    fn save_settings(inner: &mut Inner, settings: &Settings) -> Result<()> {
        if inner.invalid && inner.file.exists() {
            let backup = inner.file.with_file_name(format!(
                "log-settings.invalid-{}.json",
                uuid::Uuid::new_v4()
            ));
            fs::copy(&inner.file, backup)?;
        }
        atomic_json(&inner.file, settings)?;
        inner.invalid = false;
        Ok(())
    }
    pub async fn set_project(&self, project: String) -> Result<Value> {
        let key = self.devices.connection()?.profile.terminal_name();
        let project = project.trim().to_string();
        if project.chars().count() > 100 || project.chars().any(|c| c < ' ') {
            bail!("项目名最多100字，不能包含控制字符。");
        }
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut inner = inner.lock().unwrap();
            let mut settings = inner.settings.clone();
            settings.projects.insert(key, project);
            let repaired = inner.invalid;
            Self::save_settings(&mut inner, &settings)?;
            inner.settings = settings;
            if repaired {
                inner.state["phase"] = json!("idle");
                inner.state["message"] =
                    json!("日志设置已修复，重新连接终端后开始记录；原设置已备份。");
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        self.update(None, json!({}));
        Ok(self.snapshot())
    }
    pub fn can_change_root(&self) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        if inner
            .observer
            .as_ref()
            .is_some_and(|o| !o.done.is_cancelled())
            || inner.store.lock().unwrap().active() > 0
        {
            bail!("请先断开设备并等待记录封存，再更换日志位置。");
        }
        Ok(())
    }
    pub async fn set_root(&self, path: PathBuf) -> Result<Value> {
        self.can_change_root()?;
        if !path.is_dir() {
            bail!("请选择电脑本地文件夹。");
        }
        let mut settings = self.settings();
        settings.root = dunce::canonicalize(path)?;
        self.replace_settings(settings, true).await?;
        Ok(self.snapshot())
    }
    pub async fn replace_settings(&self, settings: Settings, persist: bool) -> Result<()> {
        self.can_change_root()?;
        let settings = settings.validate()?;
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut inner = inner.lock().unwrap();
            if persist {
                Self::save_settings(&mut inner, &settings)?;
            }
            inner.store = Arc::new(Mutex::new(LogStore::new(settings.root.clone())?));
            inner.settings = settings;
            inner.invalid = false;
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        self.update(
            None,
            json!({"phase":"idle","notice":"","message":"日志位置已更新，原目录文件保留。"}),
        );
        self.changed();
        Ok(())
    }
}

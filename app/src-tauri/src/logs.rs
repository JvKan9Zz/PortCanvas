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
    #[serde(default, rename = "maxBytes", skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
}

#[cfg(test)]
mod multi_tests {
    use super::*;
    #[test]
    fn closing_one_capture_keeps_other_writers_active(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());
        let mut store=LogStore::new(root).unwrap();
        let marker=|shell:&str|Marker{kind:"begin".into(),shell:shell.into(),sequence:"1".into(),code:None,command:"test-output".into(),cwd:"/tmp/project".into(),project_root:"/tmp/project".into(),end_cwd:String::new(),background_jobs:vec![]};
        let a=store.begin(&marker("one"),"device","",false,"command").unwrap().unwrap();
        let b=store.begin(&marker("two"),"device","",false,"command").unwrap().unwrap();
        let mut capture=Capture{device:"device".into(),context:None,manual:String::new(),current:Some(("one".into(),a.clone())),session:None,segment:0,last:None};
        capture.close(&mut store,"restart only one").unwrap();
        assert_eq!(store.active(),1);assert_eq!(store.detail(&a).unwrap()["status"],"interrupted");
        store.append(&b,"SECOND_STILL_RUNNING").unwrap();store.flush().unwrap();assert_eq!(store.detail(&b).unwrap()["status"],"running");
        assert_eq!(project_key("device","main"),"device");assert_ne!(project_key("device","other"),"device");
        store.close("test completed").unwrap();
    }
}
impl Settings {
    pub fn validate(self) -> Result<Self> {
        if !self.root.is_absolute() {
            bail!("日志根目录须为电脑本地绝对路径。");
        }
        if self.max_bytes.is_some_and(|bytes| !(16 * 1024 * 1024..=1024u64.pow(4)).contains(&bytes) || bytes % (1024 * 1024) != 0) {
            bail!("日志上限应为 16–1048576 MiB，留空表示不设上限。");
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
fn project_key(device:&str,key:&str)->String {
    if key=="main"{device.into()}else{format!("{device}:{key}")}
}
struct Observer {
    serial: u64,
    generation: u64,
    name: String,
    cancel: CancellationToken,
    done: CancellationToken,
    _snapshot: mpsc::Sender<Marker>,
    reason: Arc<Mutex<String>>,
}
struct Inner {
    settings: Settings,
    file: PathBuf,
    invalid: bool,
    store: Arc<Mutex<LogStore>>,
    state: Value,
    serial: u64,
    observers: BTreeMap<String, Observer>,
    states: BTreeMap<String, Value>,
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
    fn active(&self) -> usize { usize::from(self.current.is_some())+usize::from(self.session.is_some()) }
    // A failed write can still have created writers. Return their ownership so
    // the observer seals them instead of abandoning records marked as running.
    fn apply(mut self, store: &mut LogStore, events: Vec<Event>, manual: &str, recovered: bool) -> (Self, Result<bool>, usize) {
        let result = self.events(store, events, manual, recovered);
        let active = self.active();
        (self, result, active)
    }
    fn close(&mut self, store: &mut LogStore, reason: &str) -> Result<()> {
        let mut error=None;
        for id in self.current.take().map(|(_,id)|id).into_iter().chain(self.session.take().map(|(_,id)|id)) {
            if let Err(e)=store.finish(&id,None,Some(reason)){error=Some(e);}
        }
        if let Some(error)=error {return Err(error);} Ok(())
    }
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
                        max_bytes: None,
                    }
                }
            }
        } else {
            Settings {
                root: default_root,
                projects: BTreeMap::new(),
                max_bytes: None,
            }
        };
        let store = Arc::new(Mutex::new(Self::configured_store(&settings)?));
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                settings,
                file,
                invalid,
                store,
                state: json!({"phase":if invalid{"error"}else{"idle"},"message":message,"active":0,"notice":""}),
                serial: 0,
                observers: BTreeMap::new(),
                states: BTreeMap::new(),
            })),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            devices,
        })
    }
    pub fn settings(&self) -> Settings {
        self.inner.lock().unwrap().settings.clone()
    }
    fn configured_store(settings: &Settings) -> Result<LogStore> {
        let mut store = LogStore::new(settings.root.clone())?;
        store.configure_max_bytes(settings.max_bytes);
        Ok(store)
    }
    pub fn refresh_status(&self) {
        self.update(None, json!({}));
    }
    pub fn snapshot(&self) -> Value {
        let device=self.devices.connection().ok().map(|c|c.profile.terminal_name()).unwrap_or_default();
        let inner=self.inner.lock().unwrap();
        let mut state=inner.state.clone();
        let terminals=inner.states.iter().map(|(key,value)|{
            let mut value=value.clone();value["key"]=json!(key);
            value["project"]=json!(inner.settings.projects.get(&project_key(&device,key)).cloned().unwrap_or_default());value
        }).collect::<Vec<_>>();
        if !inner.invalid && state["phase"]!="error" {
            if let Some(error)=terminals.iter().find(|s|s["phase"]=="error") {
                state["phase"]=json!("error");state["message"]=error["message"].clone();
            } else {
                let count=terminals.iter().filter(|s|s["phase"]=="ready").count();
                if count>0 {state["phase"]=json!("ready");state["message"]=json!(format!("正在为 {count} 个终端记录，日志保存在电脑本地。"));}
                else if terminals.iter().any(|s|s["phase"]=="connecting") {state["phase"]=json!("connecting");state["message"]=json!("正在准备本地运行记录…");}
                else if !terminals.is_empty(){state["phase"]=json!("idle");state["message"]=json!("终端记录已封存。连接后可继续记录。");}
            }
        }
        state["root"]=json!(inner.settings.root);state["active"]=json!(inner.store.lock().unwrap().active());
        state["maxBytes"]=json!(inner.settings.max_bytes);
        state["notice"]=json!(inner.store.lock().unwrap().notice);
        state["configError"]=json!(inner.invalid||inner.state["phase"]=="error");
        state["terminals"]=json!(terminals);state
    }
    pub fn terminal_snapshot(&self,key:&str) -> Value {
        let device=self.devices.connection().ok().map(|c|c.profile.terminal_name()).unwrap_or_default();
        let inner=self.inner.lock().unwrap();
        let config_error=inner.invalid||inner.state["phase"]=="error";
        let mut state=if config_error{inner.state.clone()}else{inner.states.get(key).filter(|state|state["deviceKey"]==device).cloned().unwrap_or_else(||json!({"phase":"idle","message":"连接终端后自动记录。","active":0,"deviceKey":device}))};
        state["configError"]=json!(config_error);
        state["key"]=json!(key);state["root"]=json!(inner.settings.root);
        state["project"]=json!(inner.settings.projects.get(&project_key(&device,key)).cloned().unwrap_or_default());state
    }
    fn update(&self, _serial: Option<u64>, patch: Value) {
        {let mut inner=self.inner.lock().unwrap();for(k,v) in patch.as_object().unwrap(){inner.state[k]=v.clone();}}
        (self.devices.events)("logs-state",self.snapshot());
    }
    fn update_terminal(&self,key:&str,serial:u64,patch:Value){
        {let mut inner=self.inner.lock().unwrap();let Some(state)=inner.states.get_mut(key) else{return;};
        if state["serial"]!=serial{return;}for(k,v) in patch.as_object().unwrap(){state[k]=v.clone();}}
        (self.devices.events)("logs-state",self.snapshot());
    }
    fn changed(&self) {
        (self.devices.events)("logs-changed", Value::Null);
    }
    async fn store<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut LogStore) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let store = self.inner.lock().unwrap().store.clone();
        let (result, cleaned) = tokio::task::spawn_blocking(move || {
            let mut store = store.lock().unwrap();
            let before = store.cleaned;
            let result = operation(&mut store);
            (result, store.cleaned != before)
        })
            .await
            .context("本地日志工作线程异常。")?;
        if cleaned { self.changed(); }
        result
    }
    fn manual(&self, device: &str, key: &str) -> String {
        self.inner
            .lock()
            .unwrap()
            .settings
            .projects
            .get(&project_key(device,key))
            .cloned()
            .unwrap_or_default()
    }
    pub fn detach(&self, reason: &str) {
        {let mut inner=self.inner.lock().unwrap();
        for observer in inner.observers.values(){*observer.reason.lock().unwrap()=reason.into();observer.cancel.cancel();}
        for state in inner.states.values_mut(){if state["phase"]!="error"{state["phase"]=json!("idle");state["message"]=json!(reason);}}}
        self.update(None,json!({}));
    }
    async fn stop_observer(&self,key:&str,reason:&str) -> Result<()> {
        let observer={let inner=self.inner.lock().unwrap();inner.observers.get(key).map(|o|{
            *o.reason.lock().unwrap()=reason.into();(o.serial,o.cancel.clone(),o.done.clone())
        })};
        if let Some((serial,cancel,done))=observer {
            cancel.cancel();tokio::time::timeout(Duration::from_secs(10),done.cancelled()).await.context("旧日志仍在封存，请稍后重试。")?;
            let mut inner=self.inner.lock().unwrap();if inner.observers.get(key).is_some_and(|o|o.serial==serial){inner.observers.remove(key);}
        } Ok(())
    }
    pub async fn close_terminal(&self,key:&str,reason:&str) -> Result<()> {
        let _lifecycle=self.lifecycle.lock().await;self.stop_observer(key,reason).await
    }
    pub fn forget_terminal(&self,key:&str){
        self.inner.lock().unwrap().states.remove(key);self.update(None,json!({}));
    }
    pub async fn close(&self) -> Result<()> {
        let _lifecycle=self.lifecycle.lock().await;
        let keys=self.inner.lock().unwrap().observers.keys().cloned().collect::<Vec<_>>();let mut error=None;
        for key in keys {if let Err(e)=self.stop_observer(&key,"软件或设备连接已关闭。").await{error=Some(e);}}
        if let Some(error)=error{self.update(None,json!({"phase":"error","message":error.to_string()}));return Err(error);}
        self.store(|store|store.close("软件或设备连接已关闭。")).await?;
        self.update(None,json!({"phase":"idle","active":0,"message":"记录已封存。"}));Ok(())
    }
    pub async fn attach(
        &self,
        connection: Arc<SshConnection>,
        generation: u64,
        info: Value,
        key: String,
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
            if inner.observers.get(&key).is_some_and(|o| {
                o.generation == generation && o.name == name && !o.cancel.is_cancelled()
            }) {
                return Ok(());
            }
        }
        if let Err(error) = self.stop_observer(&key,"终端日志通道已更换。").await {
            self.update(None, json!({"phase":"error","message":error.to_string()}));
            return Err(error);
        }
        if connection.cancel.is_cancelled() {
            bail!("日志连接已取消。");
        }
        let serial = {
            let mut inner = self.inner.lock().unwrap();
            inner.serial += 1;
            let serial=inner.serial;
            inner.states.insert(key.clone(),json!({"serial":serial,"key":key,"deviceKey":connection.profile.terminal_name(),"generation":generation,"phase":"connecting","message":"正在准备本地运行记录…","active":0}));
            serial
        };
        self.update_terminal(
            &key, serial,
            json!({"phase":"connecting","message":"正在准备本地运行记录…"}),
        );
        let opened = connection.channel().await;
        let mut channel = match opened {
            Ok(channel) => channel,
            Err(error) => {
                if generation == self.devices.generation() {
                    self.update_terminal(&key,serial,json!({"phase":"error","message":format!("日志通道无法打开：{error}。终端仍可使用。")}));
                }
                return Err(error);
            }
        };
        let executed = channel
            .exec(
                true,
                connection.remote_command(&format!("exec tmux -L wcs-terminal -C attach-session -t {name}")),
            )
            .await;
        if let Err(error) = executed {
            let _ = channel.close().await;
            self.update_terminal(&key,serial,json!({"phase":"error","message":format!("日志通道启动失败：{error}。终端仍可使用。" )}));
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
        let reason=Arc::new(Mutex::new("软件或设备连接已关闭，未收到的输出保留为缺失。".to_string()));
        self.inner.lock().unwrap().observers.insert(key.clone(),Observer {
            serial,
            generation,
            name,
            cancel: cancel.clone(),
            done: done.clone(),
            _snapshot: tx.clone(),
            reason: reason.clone(),
        });
        let observed_pane = pane.clone();
        let observer_key=key.clone();
        tokio::spawn(async move {
            let key=observer_key;
            let mut ready = Some(ready);
            let mut parser = OutputParser::default();
            let mut buffer = Vec::new();
            let initial_manual = service.manual(&device,&key);
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
                snapshot=snapshots.recv()=>{if let Some(snapshot)=snapshot{let events=vec![Event::Marker(snapshot)];let manual=service.manual(&device,&key);let next=capture.clone();let (updated,result,active)=service.store(move|store|Ok(next.apply(store,events,&manual,true))).await?;capture=updated;let changed=result?;service.update_terminal(&key,serial,json!({"active":active}));if changed{service.changed();}}},
                message=channel.wait()=>match message{Some(russh::ChannelMsg::Data{data})=>{buffer.extend_from_slice(&data);if buffer.len()>2*1024*1024{bail!("日志控制行过长，已停止本次记录。");}let mut events=Vec::new();while let Some(end)=buffer.iter().position(|b|*b==b'\n'){let mut line=buffer.drain(..=end).collect::<Vec<_>>();line.pop();if line.last()==Some(&b'\r'){line.pop();}if line.starts_with(b"%end "){if let Some(ready)=ready.take(){let _=ready.send(());}}else if line.starts_with(b"%error "){bail!("设备拒绝日志通道。");}else if line.starts_with(b"%output "){let rest=&line[8..];if let Some(space)=rest.iter().position(|b|*b==b' '){if &rest[..space]==observed_pane.as_bytes(){events.extend(parser.push(&control_output(&rest[space+1..])));}}}}
                    if !events.is_empty(){let manual=service.manual(&device,&key);let next=capture.clone();let (updated,result,active)=service.store(move|store|Ok(next.apply(store,events,&manual,false))).await?;capture=updated;let changed=result?;if changed{service.update_terminal(&key,serial,json!({"active":active}));service.changed();}}},Some(russh::ChannelMsg::Failure)=>bail!("日志通道启动被拒绝。"),None=>bail!("日志通道已关闭，缺失输出不会补造。"),_=>{}}
            }}Ok::<_,anyhow::Error>(())}.await;
            let events = parser.flush();
            let manual = service.manual(&device,&key);
            let mut tail = capture;
            let reason = operation
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| reason.lock().unwrap().clone());
            let final_reason = reason.clone();
            let ending = service
                .store(move |store| {
                    let tail_result = tail.events(store, events, &manual, false);
                    let close = tail.close(store, &final_reason);
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
                service.update_terminal(&key,serial,json!({"phase":"error","active":0,"message":format!("本地记录已停止：{}。终端仍可使用。",error.map(|e|e.to_string()).unwrap_or(reason))}));
            } else {
                service.update_terminal(&key,serial,json!({"phase":"idle","active":0,"message":"连接已结束，日志已封存；中断期间输出未采集。"}));
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
                self.update_terminal(
                &key, serial,
                json!({"phase":"error","message":format!("日志未就绪：{error}。终端仍可使用。" )}),
            );
            }
            return setup;
        }
        if cancel.is_cancelled() || generation != self.devices.generation() {
            bail!("日志通道已结束。");
        }
        self.update_terminal(
            &key, serial,
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
    pub async fn set_max_bytes(&self, max_bytes: Option<u64>) -> Result<Value> {
        let _lifecycle = self.lifecycle.lock().await;
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut inner = inner.lock().unwrap();
            let mut settings = inner.settings.clone();
            settings.max_bytes = max_bytes;
            let settings = settings.validate()?;
            let repaired = inner.invalid;
            Self::save_settings(&mut inner, &settings)?;
            inner.settings = settings;
            if repaired { inner.state = json!({"phase":"idle","message":"日志设置已修复，重新连接终端后开始记录；原设置已备份。"}); }
            let mut store = inner.store.lock().unwrap();
            store.notice.clear();
            if let Err(error) = store.set_max_bytes(max_bytes) { store.notice = format!("上限已保存。{error}"); }
            Ok::<_, anyhow::Error>(())
        }).await??;
        self.update(None, json!({}));
        self.changed();
        Ok(self.snapshot())
    }
    pub async fn set_project(&self, project: String) -> Result<Value> {self.set_project_for("main",project).await}
    pub async fn set_project_for(&self, terminal_key:&str, project: String) -> Result<Value> {
        let key = project_key(&self.devices.connection()?.profile.terminal_name(),terminal_key);
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
        Ok(self.terminal_snapshot(terminal_key))
    }
    pub fn can_change_root(&self) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        if inner
            .observers
            .values()
            .any(|o| !o.done.is_cancelled())
            || inner.store.lock().unwrap().active() > 0
        {
            bail!("请先断开设备并等待记录封存，再更换日志位置。");
        }
        Ok(())
    }
    pub async fn set_root(&self, path: PathBuf) -> Result<Value> {
        let _lifecycle = self.lifecycle.lock().await;
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
            inner.store = Arc::new(Mutex::new(Self::configured_store(&settings)?));
            inner.settings = settings;
            inner.states.clear();inner.observers.clear();
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

#[cfg(test)]
mod retention_tests {
    use super::*;
    fn root() -> PathBuf {
        PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string())
    }
    #[tokio::test]
    async fn limit_is_opt_in_validated_persisted_and_can_be_disabled() {
        let root = root();
        let file = root.join("settings.json");
        let logs = root.join("logs");
        fs::create_dir_all(&root).unwrap();
        atomic_json(&file, &json!({"root":logs,"projects":{}})).unwrap();
        let devices = DeviceManager::new(Arc::new(|_, _| {}));
        let service = LogService::new(devices.clone(), file.clone(), logs.clone()).unwrap();
        assert!(service.settings().max_bytes.is_none());
        let before = fs::read(&file).unwrap();
        for limit in [0, 15 * 1024 * 1024, 16 * 1024 * 1024 + 1, 1024u64.pow(4) + 1] {
            assert!(service.set_max_bytes(Some(limit)).await.is_err());
            assert_eq!(fs::read(&file).unwrap(), before);
        }
        let limit = 16 * 1024 * 1024;
        assert_eq!(service.set_max_bytes(Some(limit)).await.unwrap()["maxBytes"], limit);
        let reopened = LogService::new(devices, file.clone(), logs).unwrap();
        assert_eq!(reopened.settings().max_bytes, Some(limit));
        reopened.set_max_bytes(None).await.unwrap();
        assert!(reopened.settings().max_bytes.is_none());
        assert!(serde_json::from_slice::<Value>(&fs::read(file).unwrap()).unwrap().get("maxBytes").is_none());
    }
    #[test]
    fn quota_failure_returns_writer_ownership_for_sealing() {
        let mut store = LogStore::new(root()).unwrap();
        store.set_max_bytes(Some(4096)).unwrap();
        let capture = Capture { device:"fixture".into(), context:None, manual:String::new(), current:None,
            session:None, segment:0, last:None };
        let marker = Marker { kind:"begin".into(), shell:"1-1".into(), sequence:"1".into(), code:None,
            command:"synthetic-command".into(), cwd:"/fixture".into(), project_root:String::new(),
            end_cwd:String::new(), background_jobs:vec![] };
        let (mut capture, result, active) = capture.apply(&mut store,
            vec![Event::Marker(marker), Event::Text("x".repeat(8192))], "", false);
        assert!(result.is_err());
        assert_eq!(active, 1);
        assert_eq!(store.active(), 1);
        capture.close(&mut store, "synthetic quota failure").unwrap();
        assert_eq!(store.active(), 0);
        let page = store.list(Query::default()).unwrap();
        assert_eq!(page["records"][0]["status"], "interrupted");
        assert_eq!(page["records"][0]["complete"], false);
    }
    #[tokio::test]
    async fn changing_root_and_limit_keeps_both_settings() {
        let root = root();
        let file = root.join("settings.json");
        let target = root.join("new-logs");
        fs::create_dir_all(&target).unwrap();
        let service = LogService::new(DeviceManager::new(Arc::new(|_, _| {})), file.clone(), root.join("logs")).unwrap();
        let (limit, directory) = tokio::join!(service.set_max_bytes(Some(16 * 1024 * 1024)), service.set_root(target.clone()));
        limit.unwrap();
        directory.unwrap();
        let saved: Settings = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        assert_eq!(saved.max_bytes, Some(16 * 1024 * 1024));
        assert_eq!(saved.root, dunce::canonicalize(target).unwrap());
    }
}

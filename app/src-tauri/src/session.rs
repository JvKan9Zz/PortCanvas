use crate::{
    profiles::Profile,
    ssh::{HostCheck, SshConnection},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
pub type EventSink = Arc<dyn Fn(&str, Value) + Send + Sync>;
struct Inner {
    generation: u64,
    cancel: CancellationToken,
    connection: Option<Arc<SshConnection>>,
    state: Value,
}
#[derive(Clone)]
pub struct DeviceManager {
    inner: Arc<Mutex<Inner>>,
    pub events: EventSink,
    auto_prepare: bool,
}
impl DeviceManager {
    pub fn new(events: EventSink) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                generation: 0,
                cancel: CancellationToken::new(),
                connection: None,
                state: json!({"ssh":"disconnected","desktop":"disconnected","generation":0,"message":"尚未连接设备"}),
            })),
            events,
            auto_prepare: false,
        }
    }
    pub fn with_environment(mut self) -> Self { self.auto_prepare = true; self }
    pub fn snapshot(&self) -> Value {
        self.inner.lock().unwrap().state.clone()
    }
    pub fn generation(&self) -> u64 {
        self.inner.lock().unwrap().generation
    }
    pub fn connection_with_generation(&self) -> Result<(u64, Arc<SshConnection>)> {
        let inner = self.inner.lock().unwrap();
        match &inner.connection {
            Some(c) if !c.cancel.is_cancelled() && !c.handle.is_closed() => {
                Ok((inner.generation, c.clone()))
            }
            _ => bail!("请先连接设备。"),
        }
    }
    pub fn connection(&self) -> Result<Arc<SshConnection>> {
        self.connection_with_generation().map(|(_, c)| c)
    }
    pub fn update(&self, generation: u64, patch: Value) {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation {
                return;
            }
            for (k, v) in patch.as_object().unwrap() {
                inner.state[k] = v.clone();
            }
            inner.state.clone()
        };
        (self.events)("device-state", state);
    }
    fn connection_progress(&self, generation: u64, stage: &'static str) {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation || inner.state["ssh"] != "connecting" {
                return;
            }
            inner.state["connectionStage"] = json!(stage);
            inner.state.clone()
        };
        (self.events)("device-state", state);
    }
    pub async fn connect(
        &self,
        profile: Profile,
        password: zeroize::Zeroizing<String>,
        make_check: impl FnOnce(CancellationToken) -> HostCheck,
    ) -> Result<Value> {
        let profile = profile.validate()?;
        let (generation, cancel, old, state) = {
            let mut inner = self.inner.lock().unwrap();
            inner.cancel.cancel();
            let old = inner.connection.take();
            inner.generation += 1;
            inner.cancel = CancellationToken::new();
            inner.state = json!({"generation":inner.generation,"deviceId":profile.id,"device":profile.name,"ssh":"connecting","desktop":"disconnected","connectionStage":"prepare","connectionTarget":profile.address(),"message":format!("正在连接 {}…",profile.host)});
            (
                inner.generation,
                inner.cancel.clone(),
                old,
                inner.state.clone(),
            )
        };
        if let Some(old) = old {
            tokio::spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(2), old.disconnect()).await;
            });
        }
        (self.events)("device-state", state);
        let progress_manager = self.clone();
        let result = SshConnection::connect_with_progress(
            profile,
            password,
            make_check(cancel.clone()),
            cancel.clone(),
            Arc::new(move |stage| progress_manager.connection_progress(generation,stage)),
        )
        .await;
        let connection = match result {
            Ok(mut c) => { c.auto_prepare = self.auto_prepare; Arc::new(c) },
            Err(e) => {
                cancel.cancel();
                self.update(
                    generation,
                    json!({"ssh":"error","desktop":"disconnected","message":e.to_string()}),
                );
                return Err(e);
            }
        };
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation || cancel.is_cancelled() {
                drop(inner);
                connection.cancel.cancel();
                bail!("连接已取消。");
            }
            inner.connection = Some(connection.clone());
        }
        self.update(
            generation,
            json!({"ssh":"connected","connectionStage":"ready","message":"设备连接成功。"}),
        );
        let manager = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(1))=>{if connection.handle.is_closed(){manager.closed(generation);break;}}}
            }
        });
        Ok(self.snapshot())
    }
    fn closed(&self, generation: u64) {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation {
                return;
            }
            inner.cancel.cancel();
            inner.connection.take();
            inner.generation += 1;
            inner.state = json!({"generation":inner.generation,"ssh":"disconnected","desktop":"disconnected","message":"设备连接已关闭，请重新连接。"});
            inner.state.clone()
        };
        (self.events)("device-state", state);
    }
    pub async fn disconnect(&self) {
        let (old, state) = {
            let mut inner = self.inner.lock().unwrap();
            inner.cancel.cancel();
            inner.generation += 1;
            let old = inner.connection.take();
            inner.state = json!({"generation":inner.generation,"ssh":"disconnected","desktop":"disconnected","message":"设备已断开"});
            (old, inner.state.clone())
        };
        (self.events)("device-state", state);
        if let Some(old) = old {
            let _ = tokio::time::timeout(Duration::from_secs(2), old.disconnect()).await;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_updates_cannot_change_new_session() {
        let m = DeviceManager::new(Arc::new(|_, _| {}));
        m.closed(0);
        m.update(0, json!({"ssh":"connected"}));
        assert_eq!(m.snapshot()["ssh"], "disconnected");
        assert_eq!(m.generation(), 1);
        m.connection_progress(0,"authenticate");
        assert!(m.snapshot()["connectionStage"].is_null());
    }
    #[test]
    fn failure_keeps_stage_and_late_progress_cannot_replace_it() {
        let m=DeviceManager::new(Arc::new(|_,_|{}));
        m.update(0,json!({"ssh":"connecting","connectionStage":"prepare"}));
        m.connection_progress(0,"tcp");
        m.update(0,json!({"ssh":"error","message":"timeout"}));
        m.connection_progress(0,"authenticate");
        assert_eq!(m.snapshot()["connectionStage"],"tcp");
        assert_eq!(m.snapshot()["ssh"],"error");
    }
}

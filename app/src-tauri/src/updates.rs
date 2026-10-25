use crate::session::EventSink;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio_util::sync::CancellationToken;

struct Pending {
    state: Value,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
    cancel: CancellationToken,
}
pub struct UpdateService {
    inner: Mutex<Pending>,
    operation: tokio::sync::Mutex<()>,
    events: EventSink,
    pub portable: bool,
}
impl UpdateService {
    pub fn new(events: EventSink, portable: bool) -> Self {
        Self {
            inner: Mutex::new(Pending {
                state: json!({"status":"idle"}),
                update: None,
                bytes: None,
                cancel: CancellationToken::new(),
            }),
            operation: tokio::sync::Mutex::new(()),
            events,
            portable,
        }
    }
    pub fn snapshot(&self) -> Value {
        self.inner.lock().unwrap().state.clone()
    }
    fn publish(&self, patch: Value) -> Value {
        let value = {
            let mut inner = self.inner.lock().unwrap();
            for (key, value) in patch.as_object().unwrap() {
                inner.state[key] = value.clone();
            }
            inner.state.clone()
        };
        (self.events)("update-state", value.clone());
        value
    }
    pub fn cancel(&self) {
        self.inner.lock().unwrap().cancel.cancel();
    }
    pub async fn check(&self, app: &tauri::AppHandle) -> Result<Value> {
        let _operation = self.operation.try_lock().context("更新操作正在进行。")?;
        let cancel = CancellationToken::new();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.cancel = cancel.clone();
            inner.update = None;
            inner.bytes = None;
            inner.state = json!({"status":"checking"});
        }
        self.publish(json!({}));
        let result=async {
            let mut builder=app.updater_builder().timeout(Duration::from_secs(20));
            if self.portable {builder=builder.target(format!("windows-{}-portable",std::env::consts::ARCH));}
            // Only development builds can use a loopback fixture. Shipped builds always use HTTPS.
            #[cfg(debug_assertions)]
            if let Ok(endpoint)=std::env::var("PORTCANVAS_UPDATE_TEST_ENDPOINT") {
                let url=endpoint.parse::<tauri::Url>()?;
                anyhow::ensure!(url.host_str()==Some("127.0.0.1"),"更新测试地址必须是本机回环地址。");
                builder=builder.endpoints(vec![url])?;
            }
            let updater=builder.build()?;
            let result=tokio::select!{biased;_=cancel.cancelled()=>return Ok(None),result=updater.check()=>result?};
            if let Some(mut update)=result {
                anyhow::ensure!(update.download_url.scheme()=="https" || cfg!(debug_assertions)&&update.download_url.host_str()==Some("127.0.0.1"),"更新下载地址必须使用 HTTPS。");
                update.timeout=Some(Duration::from_secs(300));
                let metadata=json!({"status":"available","version":update.version,"notes":update.body.clone().unwrap_or_default(),"downloaded":0,"total":null,"message":null});
                self.inner.lock().unwrap().update=Some(update);
                Ok(Some(metadata))
            }else{Ok(Some(json!({"status":"current","message":null})))}
        }.await;
        match result {
            Ok(Some(value)) => Ok(self.publish(value)),
            Ok(None) => Ok(self.publish(json!({"status":"canceled"}))),
            Err(error) => {
                Ok(self
                    .publish(json!({"status":"error","message":format!("检查更新失败：{error}")})))
            }
        }
    }
    pub async fn download(&self) -> Result<Value> {
        let _operation = self.operation.try_lock().context("更新操作正在进行。")?;
        let (update, cancel) = {
            let mut inner = self.inner.lock().unwrap();
            let update = inner.update.clone().context("请先检查更新。")?;
            inner.bytes = None;
            inner.cancel = CancellationToken::new();
            (update, inner.cancel.clone())
        };
        self.publish(json!({"status":"downloading","downloaded":0,"total":null,"connections":null,"message":null}));
        let counter = Mutex::new((0u64, Instant::now() - Duration::from_secs(1)));
        let download = crate::update_download::download(
            &update,
            |length, total, connections| {
                let mut counter = counter.lock().unwrap();
                counter.0 += length as u64;
                if counter.1.elapsed() >= Duration::from_millis(100) {
                    self.publish(
                        json!({"downloaded":counter.0,"total":total,"connections":connections}),
                    );
                    counter.1 = Instant::now();
                }
            },
            || {
                self.publish(json!({"status":"verifying"}));
            },
        );
        let result =
            tokio::select! {biased;_=cancel.cancelled()=>None,result=download=>Some(result)};
        match result {
            None => Ok(self.publish(json!({"status":"canceled","downloaded":0}))),
            Some(Ok(bytes)) => {
                let size = bytes.len();
                self.inner.lock().unwrap().bytes = Some(bytes);
                Ok(self.publish(json!({"status":"ready","downloaded":size,"total":size})))
            }
            Some(Err(error)) => Ok(self
                .publish(json!({"status":"error","message":format!("下载或校验失败：{error}")}))),
        }
    }
    pub fn begin_install(&self) -> Result<(Update, Vec<u8>)> {
        let _operation = self.operation.try_lock().context("更新操作正在进行。")?;
        let mut inner = self.inner.lock().unwrap();
        anyhow::ensure!(inner.state["status"] == "ready", "请先下载并校验更新。");
        let update = inner
            .update
            .clone()
            .context("更新信息已失效，请重新检查。")?;
        let bytes = inner.bytes.take().context("更新文件已失效，请重新下载。")?;
        update.verify(&bytes).context("更新文件签名校验失败。")?;
        inner.state["status"] = json!("installing");
        drop(inner);
        self.publish(json!({}));
        Ok((update, bytes))
    }
    pub fn install_error(&self, error: &anyhow::Error) {
        self.publish(json!({"status":"error","message":format!("安装更新失败：{error}")}));
    }
    pub fn restore_result(&self, value: Value) {
        self.publish(value);
    }
}

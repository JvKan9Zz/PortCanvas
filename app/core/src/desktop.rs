mod native;
pub use native::NativeDesktop;
use crate::{
    session::DeviceManager,
    ssh::{quote, SshConnection},
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

pub fn python(source: &str, args: Value) -> Result<String> {
    Ok(format!("python3 -u -c {}",quote(&format!("import sys,json,base64;sys.argv=json.loads(base64.b64decode('{}'));exec(base64.b64decode('{}'))",STANDARD.encode(serde_json::to_vec(&args)?),STANDARD.encode(source.replace("\r\n","\n"))))))
}
pub async fn schedule_reboot(connection:&SshConnection)->Result<()>{
    let command=format!("sh -c {}",quote(&include_str!("../resources/reboot.sh").replace("\r\n","\n")));
    let(output,status)=connection.exec(&command,4096,Duration::from_secs(8)).await?;
    anyhow::ensure!(status==0&&output.lines().any(|line|line=="PORTCANVAS_REBOOT_SCHEDULED"),"无法提交重启请求，请检查设备账户是否具备免交互重启权限。");
    Ok(())
}
#[derive(Clone)]
struct Running {
    id: u64,
    generation: u64,
    cancel: CancellationToken,
    done: CancellationToken,
}
async fn close_process(mut process: crate::ssh::SshChannel) {
    let _ = process.eof().await;
    let _ = tokio::time::timeout(Duration::from_secs(4), async {
        while process.wait().await.is_some() {}
    })
    .await;
    let _ = process.close().await;
}
#[derive(Clone)]
pub struct DesktopService {
    running: Arc<Mutex<Option<Running>>>,
    next: Arc<AtomicU64>,
    devices: DeviceManager,
    start_gate: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Deserialize)]
struct DesktopInfo {
    port: u16,
}
impl DesktopService {
    pub fn new(devices: DeviceManager) -> Self {
        Self {
            running: Arc::new(Mutex::new(None)),
            next: Arc::new(AtomicU64::new(0)),
            devices,
            start_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
    pub fn id(&self) -> Option<u64> {
        self.running.lock().unwrap().as_ref().map(|r| r.id)
    }
    pub async fn stop(&self, expected: Option<u64>) {
        let old = {
            let mut running = self.running.lock().unwrap();
            if expected.is_some() && running.as_ref().map(|r| r.id) != expected {
                return;
            }
            running.take()
        };
        if let Some(old) = old {
            old.cancel.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(5), old.done.cancelled()).await;
            let current=self.running.lock().unwrap();
            if current.is_none(){self.devices.update(old.generation,json!({"desktop":"disconnected","desktopStage":null}));}
        }
    }
    pub async fn failed(&self,id:u64,message:String){
        let generation={let running=self.running.lock().unwrap();let Some(run)=running.as_ref().filter(|run|run.id==id)else{return;};run.generation};
        self.stop(Some(id)).await;
        if self.running.lock().unwrap().is_none(){self.devices.update(generation,json!({"desktop":"error","message":if message.is_empty(){"桌面连接中断，请重新连接。".into()}else{message}}));}
    }
    pub fn ready(&self, id: u64) {
        let running = self.running.lock().unwrap();
        if let Some(run) = running.as_ref() {
            if run.id == id && !run.cancel.is_cancelled() {
                self.devices.update(
                    run.generation,
                    json!({"desktop":"connected","message":"远程桌面已连接"}),
                );
            }
        }
    }
    async fn open_process(&self, connection: &SshConnection, generation: u64, cancel: &CancellationToken, mode: crate::desktop_settings::DesktopMode) -> Result<(crate::ssh::SshChannel, DesktopInfo)> {
        connection.prepare(crate::remote_env::Feature::Desktop, cancel).await?;
        let config = json!({"display":connection.profile.display,"xauthority":connection.profile.xauthority,"tuning":mode.tuning()});
        let command = python(include_str!("../resources/desktop.py"), json!(["desktop", config.to_string()]))?;
        let mut process = tokio::time::timeout(Duration::from_secs(5), connection.channel()).await??;
        let result = async {
            anyhow::ensure!(!cancel.is_cancelled(), "桌面连接已取消。");
            process.exec(true, connection.remote_command(&command)).await?;
            self.devices.update(generation,json!({"desktopStage":"service"}));
            let ready = async {
                let mut output = String::new();
                let mut diagnostic = String::new();
                while let Some(message) = process.wait().await {
                    match message {
                        russh::ChannelMsg::Data { data } => {
                            output.push_str(&String::from_utf8_lossy(&data));
                            if output.len() > 65536 {
                                bail!("桌面服务响应过长。");
                            }
                            if let Some(start) = output.find("STUDIO_DESKTOP=") {
                                if let Some(end) = output[start..].find('\n') {
                                    return Ok(serde_json::from_str::<DesktopInfo>(
                                        &output[start + 15..start + end],
                                    )?);
                                }
                            }
                        }
                        russh::ChannelMsg::ExtendedData { data, .. } => {
                            diagnostic.push_str(&String::from_utf8_lossy(&data));
                            if diagnostic.len() > 6000 {
                                diagnostic = diagnostic
                                    .chars()
                                    .rev()
                                    .take(3000)
                                    .collect::<String>()
                                    .chars()
                                    .rev()
                                    .collect();
                            }
                        }
                        _ => {}
                    }
                }
                bail!(
                    "{}",
                    if diagnostic.trim().is_empty() {
                        "桌面服务在就绪前关闭。"
                    } else {
                        diagnostic.trim()
                    }
                )
            };

            tokio::select! {_=cancel.cancelled()=>Err(anyhow::anyhow!("桌面连接已取消。")),result=tokio::time::timeout(Duration::from_secs(20),ready)=>result.context("桌面启动超时，请确认远端 X11 桌面已登录。").and_then(|result| result)}
        }.await;
        match result {
            Ok(info) => Ok((process, info)),
            Err(error) => {close_process(process).await;Err(error)}
        }
    }
    pub fn awaiting_frame(&self, id: u64) {
        let generation = {let current=self.running.lock().unwrap();current.as_ref().filter(|run|run.id==id&&!run.cancel.is_cancelled()).map(|run|run.generation)};
        if let Some(generation)=generation {self.devices.update(generation,json!({"desktop":"connecting","desktopStage":"frame","message":"正在接收完整桌面画面…"}));}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn python_arguments_are_encoded_without_shell_expansion() {
        let cmd = python("print('ok')", json!(["desktop", "quote'\n$(false)"])).unwrap();
        assert!(!cmd.contains("$(false)"));
        assert!(cmd.starts_with("python3 -u -c '"));
    }
}

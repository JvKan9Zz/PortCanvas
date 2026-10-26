use crate::{
    session::DeviceManager,
    ssh::{quote, SshConnection},
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        handshake::server::{Request, Response},
        http,
        protocol::WebSocketConfig,
        Message,
    },
};
use tokio_util::sync::CancellationToken;

pub fn python(source: &str, args: Value) -> Result<String> {
    Ok(format!("python3 -u -c {}",quote(&format!("import sys,json,base64;sys.argv=json.loads(base64.b64decode('{}'));exec(base64.b64decode('{}'))",STANDARD.encode(serde_json::to_vec(&args)?),STANDARD.encode(source.replace("\r\n","\n"))))))
}
fn desktop_origin_matches(expected:&str,actual:Option<&str>)->bool{
    // url::Url treats custom schemes as opaque; WebKit can retain the app scheme.
    actual==Some(expected)||(cfg!(target_os="macos")&&expected=="null"&&actual==Some("tauri://localhost"))
}
pub async fn schedule_reboot(connection:&SshConnection)->Result<()>{
    let command=format!("sh -c {}",quote(&include_str!("../resources/reboot.sh").replace("\r\n","\n")));
    let(output,status)=connection.exec(&command,4096,Duration::from_secs(8)).await?;
    anyhow::ensure!(status==0&&output.lines().any(|line|line=="PORTCANVAS_REBOOT_SCHEDULED"),"无法提交重启请求，请检查设备账户是否具备免交互重启权限。");
    Ok(())
}
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
    pub async fn start(&self, origin: String,mode:crate::desktop_settings::DesktopMode) -> Result<Value> {
        let (generation, connection) = self.devices.connection_with_generation()?;
        self.stop(None).await;
        if generation != self.devices.generation() || connection.cancel.is_cancelled() {
            bail!("设备连接已变化，请重新打开桌面。");
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let cancel = connection.cancel.child_token();
        let done = CancellationToken::new();
        *self.running.lock().unwrap() = Some(Running {
            id,
            generation,
            cancel: cancel.clone(),
            done: done.clone(),
        });
        self.devices.update(
            generation,
            json!({"desktop":"connecting","desktopStage":"prepare","message":"正在打开远程桌面…"}),
        );
        let operation = async {
            connection.prepare(crate::remote_env::Feature::Desktop, &cancel).await?;
            let config = json!({"display":connection.profile.display,"xauthority":connection.profile.xauthority,"tuning":mode.tuning()});
            let command = python(
                include_str!("../resources/desktop.py"),
                json!(["desktop", config.to_string()]),
            )?;
            let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
            let port = listener.local_addr()?.port();
            let mut process =
                tokio::time::timeout(Duration::from_secs(5), connection.channel()).await??;
            if cancel.is_cancelled() {
                let _ = process.close().await;
                bail!("桌面连接已取消。");
            }
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
            let info = tokio::select! {_=cancel.cancelled()=>Err(anyhow::anyhow!("桌面连接已取消。")),result=tokio::time::timeout(Duration::from_secs(20),ready)=>result.context("桌面启动超时，请确认远端 X11 桌面已登录。").and_then(|result| result)};
            let info = match info {
                Ok(info) => info,
                Err(e) => {
                    close_process(process).await;
                    return Err(e);
                }
            };
            let token = uuid::Uuid::new_v4().simple().to_string();
            let expected_path = format!("/{token}");
            let devices = self.devices.clone();
            let worker_cancel = cancel.clone();
            let worker_done = done.clone();
            let conn = connection.clone();
            tokio::spawn(async move {
                let active = Arc::new(tokio::sync::Semaphore::new(1));
                loop {
                    tokio::select! {_=worker_cancel.cancelled()=>break,message=process.wait()=>{if message.is_none(){if !worker_cancel.is_cancelled(){devices.update(generation,json!({"desktop":"error","message":"远程桌面服务已结束。"}));}worker_cancel.cancel();break;}},accepted=listener.accept()=>{let Ok((socket,_))=accepted else{break;};let _=socket.set_nodelay(true);let Ok(permit)=active.clone().try_acquire_owned()else{continue;};let path=expected_path.clone();let origin=origin.clone();let cancel=worker_cancel.clone();let conn=conn.clone();tokio::spawn(async move{let _permit=permit;let operation=async{let mut config=WebSocketConfig::default();config.max_message_size=Some(4*1024*1024);config.max_frame_size=Some(4*1024*1024);let ws=tokio::time::timeout(Duration::from_secs(5),accept_hdr_async_with_config(socket,move|request:&Request,response:Response|{if request.uri().path()!=path||!desktop_origin_matches(&origin,request.headers().get("origin").and_then(|v|v.to_str().ok())){return Err(http::Response::builder().status(403).body(Some("Invalid session or origin".into())).unwrap());}Ok(response)},Some(config))).await??;bridge(ws,conn,info.port,cancel.clone()).await};tokio::select!{_=cancel.cancelled()=>{},_ = operation=>{}}});}}
                }
                close_process(process).await;
                worker_done.cancel();
            });
            Ok(
                json!({"url":format!("ws://127.0.0.1:{port}/{token}"),"id":id,"generation":generation,"backend":"vnc-x11-over-ssh","tuning":mode.tuning()}),
            )
        };
        let result = operation.await;
        if let Err(error) = &result {
            let canceled = cancel.is_cancelled();
            cancel.cancel();
            done.cancel();
            let mut current=self.running.lock().unwrap();
            if current.as_ref().is_some_and(|run|run.id==id){current.take();self.devices.update(generation,json!({"desktop":if canceled{"disconnected"}else{"error"},"message":error.to_string()}));}
        }
        result
    }
}
async fn bridge(
    ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    connection: Arc<SshConnection>,
    port: u16,
    cancel: CancellationToken,
) -> Result<()> {
    let channel = connection
        .handle
        .channel_open_direct_tcpip("127.0.0.1", port as u32, "127.0.0.1", 0)
        .await?;
    let (read, write) = tokio::io::split(channel.into_stream());
    let (mut reader, mut writer) = (read, write);
    let (mut send, mut receive) = ws.split();
    let up = async {
        while let Some(message) = receive.next().await {
            match message? {
                Message::Binary(bytes) => writer.write_all(&bytes).await?,
                Message::Close(_) => break,
                Message::Text(_) => bail!("VNC 只接受二进制数据。"),
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    };
    let down = async {
        let mut buffer = vec![0; 65536];
        loop {
            let n = reader.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            send.send(Message::Binary(buffer[..n].to_vec().into()))
                .await?;
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::select! {_=cancel.cancelled()=>{},result=up=>{result?;},result=down=>{result?;}}
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn websocket_origin_stays_bound_to_the_application(){
        assert!(desktop_origin_matches("http://tauri.localhost",Some("http://tauri.localhost")));
        assert!(!desktop_origin_matches("http://tauri.localhost",Some("null")));
        assert!(!desktop_origin_matches("null",Some("https://example.org")));
        assert!(!desktop_origin_matches("null",None));
        assert_eq!(desktop_origin_matches("null",Some("tauri://localhost")),cfg!(target_os="macos"));
    }
    #[test]
    fn python_arguments_are_encoded_without_shell_expansion() {
        let cmd = python("print('ok')", json!(["desktop", "quote'\n$(false)"])).unwrap();
        assert!(!cmd.contains("$(false)"));
        assert!(cmd.starts_with("python3 -u -c '"));
    }
}

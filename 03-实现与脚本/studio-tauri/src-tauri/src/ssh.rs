use anyhow::{bail, Context, Result};
use russh::{client, Channel, ChannelMsg};
use std::{sync::{Arc, Mutex}, time::Duration};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;
use crate::profiles::Profile;

pub type SshChannel = Channel<client::Msg>;
pub type HostCheck = Arc<dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<bool>> + Send>> + Send + Sync>;
pub struct ClientHandler { pub check: HostCheck, pub rejected: Arc<Mutex<Option<String>>> }
impl client::Handler for ClientHandler {
    type Error = anyhow::Error;
    async fn check_server_key(&mut self, key: &russh::keys::PublicKeyOrCertificate) -> Result<bool> {
        if key.certificate().is_some(){bail!("此版本尚未支持 SSH 主机证书认证，请使用受支持的主机公钥。");}
        let fingerprint=key.public_key().fingerprint(russh::keys::HashAlg::Sha256).to_string();
        match (self.check)(fingerprint).await {
            Ok(true)=>Ok(true),Ok(false)=>{*self.rejected.lock().unwrap()=Some("主机身份未确认，已停止连接。".into());Ok(false)},
            Err(error)=>{*self.rejected.lock().unwrap()=Some(error.to_string());Ok(false)}
        }
    }
}
pub struct SshConnection { pub handle: Arc<client::Handle<ClientHandler>>, pub profile: Profile, pub cancel: CancellationToken }
impl SshConnection {
    pub async fn connect(profile:Profile,password:Zeroizing<String>,check:HostCheck,cancel:CancellationToken)->Result<Self>{
        let profile=profile.validate()?;
        let config=client::Config{keepalive_interval:Some(Duration::from_secs(5)),keepalive_max:2,nodelay:true,channel_buffer_size:32,..Default::default()};
        let rejected=Arc::new(Mutex::new(None));
        let handler=ClientHandler{check,rejected:rejected.clone()};
        let operation=async{
            let mut handle=client::connect(Arc::new(config),(profile.host.as_str(),profile.port),handler).await.map_err(|e|anyhow::anyhow!(rejected.lock().unwrap().clone().unwrap_or_else(||friendly(&e.to_string()))))?;
            if !handle.authenticate_password(&profile.username,password.as_str()).await?.success(){bail!("认证失败：请检查用户名和密码。");}
            Ok(Self{handle:Arc::new(handle),profile,cancel:cancel.clone()})
        };
        tokio::select!{_ = cancel.cancelled()=>bail!("连接已取消。"),result=tokio::time::timeout(Duration::from_secs(30),operation)=>result.context("连接超时：请检查网络和设备。")?}
    }
    pub async fn channel(&self)->Result<SshChannel>{
        tokio::select!{_ = self.cancel.cancelled()=>bail!("设备已断开。"),result=self.handle.channel_open_session()=>Ok(result?)}
    }
    pub async fn exec(&self,command:&str,limit:usize,timeout:Duration)->Result<(String,i32)>{
        let mut channel=self.channel().await?;channel.exec(true,command).await?;
        let operation=async{
            let mut output=Vec::new();let mut diagnostic=Vec::new();let mut status=None;
            while let Some(message)=channel.wait().await {
                match message {
                    ChannelMsg::Data{data}=>{if output.len()+data.len()>limit{bail!("远端响应超过允许大小。");}output.extend_from_slice(&data);},
                    ChannelMsg::ExtendedData{data,..}=>{if diagnostic.len()<6000{diagnostic.extend_from_slice(&data[..data.len().min(6000-diagnostic.len())]);}},
                    ChannelMsg::ExitStatus{exit_status}=>status=Some(exit_status as i32),_=>{}
                }
            }
            let status=status.context("远端命令未返回退出状态。")?;
            if status!=0 && output.is_empty(){bail!("{}",String::from_utf8_lossy(&diagnostic).trim());}
            Ok((String::from_utf8(output).context("远端输出不是 UTF-8 文本。")?,status))
        };
        let result=tokio::select!{_ = self.cancel.cancelled()=>Err(anyhow::anyhow!("设备连接已断开。")),result=tokio::time::timeout(timeout,operation)=>result.context("远端命令超时。")?};
        let _=channel.close().await;result
    }
    pub async fn sftp(&self)->Result<russh_sftp::client::SftpSession>{let channel=self.channel().await?;channel.request_subsystem(true,"sftp").await?;Ok(russh_sftp::client::SftpSession::new(channel.into_stream()).await?)}
    pub async fn disconnect(&self){self.cancel.cancel();let _=self.handle.disconnect(russh::Disconnect::ByApplication,"Studio closed","en").await;}
}
pub fn friendly(message:&str)->String{
    if message.to_lowercase().contains("auth"){"认证失败：请检查用户名和密码。".into()}
    else if message.contains("10061")||message.to_lowercase().contains("refused"){"连接被拒绝：请确认 SSH 服务和端口。".into()}
    else if message.to_lowercase().contains("timed out"){"连接超时：请检查设备与网络。".into()}
    else{message.to_string()}
}
pub fn quote(text:&str)->String{format!("'{}'",text.replace('\'',"'\\''"))}

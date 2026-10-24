use anyhow::Result;
use studio_core::{profiles::Profile,ssh::{HostCheck,SshConnection,quote}};
use serde::Deserialize;
use std::{io::{self,Read},sync::Arc,time::Duration};
#[derive(Deserialize)]struct Input { profile:Profile,password:String,fingerprint:String }
#[tokio::main]async fn main()->Result<()> {
    let mut text=String::new();io::stdin().read_to_string(&mut text)?;let input:Input=serde_json::from_str(&text)?;use zeroize::Zeroize;text.zeroize();
    let expected=input.fingerprint;let check:HostCheck=Arc::new(move|actual|{let expected=expected.clone();Box::pin(async move{anyhow::ensure!(actual==expected,"主机指纹不匹配");Ok(true)})});
    let connection=SshConnection::connect(input.profile,zeroize::Zeroizing::new(input.password),check,tokio_util::sync::CancellationToken::new()).await?;
    let (output,status)=connection.exec("printf 'RUST_SSH_中文_OK\\n'; command -v tmux; command -v python3",10000,Duration::from_secs(10)).await?;anyhow::ensure!(status==0&&output.contains("RUST_SSH_中文_OK"));
    let sftp=connection.sftp().await?;let home=sftp.canonicalize(".").await?;let entries=sftp.read_dir(&home).await?;
    let mut channel=connection.channel().await?;channel.request_pty(true,"xterm-256color",80,24,0,0,&[]).await?;channel.exec(true,"printf 'PTY_OK\\n'").await?;
    let mut pty=Vec::new();while let Some(message)=channel.wait().await{if let russh::ChannelMsg::Data{data}=message{pty.extend_from_slice(&data);}}anyhow::ensure!(String::from_utf8_lossy(&pty).contains("PTY_OK"));
    // A bounded, loopback-only fixture verifies direct-tcpip without changing a service.
    let code="import socket\ns=socket.socket();s.bind(('127.0.0.1',0));s.listen(1);s.settimeout(10)\nprint(s.getsockname()[1],flush=True)\nc,_=s.accept();c.settimeout(5);c.sendall(b'RFB 003.008\\n');c.recv(1);c.close();s.close()";
    let mut fixture=connection.channel().await?;fixture.exec(true,format!("python3 -u -c {}",quote(code))).await?;
    let port=tokio::time::timeout(Duration::from_secs(10),async{let mut bytes=Vec::new();while let Some(message)=fixture.wait().await{if let russh::ChannelMsg::Data{data}=message{bytes.extend_from_slice(&data);if bytes.contains(&b'\n'){return String::from_utf8(bytes)?.trim().parse::<u16>().map_err(Into::into);}}}anyhow::bail!("转发探针未就绪")}).await??;
    let forwarding=connection.handle.channel_open_direct_tcpip("127.0.0.1",port as u32,"127.0.0.1",0).await?;let mut stream=forwarding.into_stream();
    use tokio::io::{AsyncReadExt,AsyncWriteExt};let mut banner=[0;12];tokio::time::timeout(Duration::from_secs(5),stream.read_exact(&mut banner)).await??;anyhow::ensure!(&banner==b"RFB 003.008\n");stream.write_all(b"x").await?;stream.shutdown().await?;fixture.close().await?;
    println!("{}",serde_json::json!({"ssh":true,"hostVerification":true,"sftp":true,"pty":true,"directTcpip":true,"home":home,"entries":entries.count(),"command":output.trim()}));
    sftp.close().await?;connection.disconnect().await;Ok(())
}

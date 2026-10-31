//! An owned in-process RFB stream over the authenticated SSH connection.
use super::*;
use tokio::io::{duplex, DuplexStream};

pub struct NativeDesktop {
    pub id: u64,
    pub generation: u64,
    stream: Option<DuplexStream>,
    cancel: CancellationToken,
}
impl NativeDesktop {
    pub fn take_stream(&mut self) -> Result<DuplexStream> {
        self.stream.take().context("原生桌面流已被使用。")
    }
    pub fn cancellation(&self) -> CancellationToken {
        self.cancel.clone()
    }
}
impl Drop for NativeDesktop {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl DesktopService {
    pub async fn start_native(
        &self,
        mode: crate::desktop_settings::DesktopMode,
        expected_generation: u64,
        request: CancellationToken,
    ) -> Result<NativeDesktop> {
        let (generation, connection) = self.devices.connection_with_generation()?;
        anyhow::ensure!(
            generation == expected_generation,
            "设备连接已变化，请重新打开桌面。"
        );
        let _start = tokio::select! {biased;_=request.cancelled()=>bail!("桌面连接已取消。"),gate=self.start_gate.lock()=>gate};
        anyhow::ensure!(!request.is_cancelled()&&generation==self.devices.generation()&&!connection.cancel.is_cancelled(),"设备连接已变化，请重新打开桌面。");
        self.stop(None).await;
        anyhow::ensure!(
            !request.is_cancelled()
                && generation == self.devices.generation()
                && !connection.cancel.is_cancelled(),
            "设备连接已变化，请重新打开桌面。"
        );
        let run = Running {
            id: self.next.fetch_add(1, Ordering::SeqCst) + 1,
            generation,
            cancel: connection.cancel.child_token(),
            done: CancellationToken::new(),
        };
        *self.running.lock().unwrap() = Some(run.clone());
        let forward = run.clone();
        let startup_finished = CancellationToken::new();
        let finished = startup_finished.clone();
        tokio::spawn(async move {
            tokio::select! {biased;_=finished.cancelled()=>{},_=request.cancelled()=>forward.cancel.cancel(),_=forward.done.cancelled()=>{}}
        });
        self.devices.update(
            generation,
            json!({"desktop":"connecting","desktopStage":"prepare","message":"正在打开远程桌面…"}),
        );
        let result=async {
            let (process,info)=self.open_process(&connection,generation,&run.cancel,mode).await?;
            let channel=tokio::time::timeout(Duration::from_secs(5),connection.handle.channel_open_direct_tcpip("127.0.0.1",u32::from(info.port),"127.0.0.1",0)).await;
            let channel=match channel {
                Ok(Ok(channel))=>channel,
                other=>{close_process(process).await;return Err(match other {Ok(Err(error))=>error.into(),Err(error)=>anyhow::anyhow!("桌面隧道建立超时：{error}"),_=>unreachable!()});}
            };
            if run.cancel.is_cancelled()||generation!=self.devices.generation(){
                let _=channel.eof().await;let _=channel.close().await;close_process(process).await;bail!("桌面连接已取消。");
            }
            self.devices.update(generation,json!({"desktopStage":"tunnel"}));
            let (stream,backend)=duplex(256*1024);
            let service=self.clone();let worker_run=run.clone();
            tokio::spawn(async move {
                let result=pump(backend,channel,process,&worker_run.cancel).await;
                if !worker_run.cancel.is_cancelled() {
                    let current=service.running.lock().unwrap().as_ref().is_some_and(|r|r.id==worker_run.id);
                    if current {service.devices.update(worker_run.generation,json!({"desktop":if result.is_err(){"error"}else{"disconnected"},"desktopStage":null,"message":result.err().map(|e|e.to_string()).unwrap_or_else(||"远程桌面连接已结束。".into())}));}
                }
                worker_run.cancel.cancel();worker_run.done.cancel();
            });
            Ok(NativeDesktop {id:run.id,generation,stream:Some(stream),cancel:run.cancel.clone()})
        }.await;
        startup_finished.cancel();
        if let Err(error) = &result {
            let canceled = run.cancel.is_cancelled();
            run.cancel.cancel();
            run.done.cancel();
            let owned = {
                let mut current = self.running.lock().unwrap();
                if current.as_ref().is_some_and(|r| r.id == run.id) {
                    current.take();
                    true
                } else {
                    false
                }
            };
            if owned {
                self.devices.update(generation,json!({"desktop":if canceled{"disconnected"}else{"error"},"desktopStage":null,"message":error.to_string()}));
            }
        }
        result
    }
}
async fn pump(
    stream: DuplexStream,
    mut channel: crate::ssh::SshChannel,
    mut process: crate::ssh::SshChannel,
    cancel: &CancellationToken,
) -> Result<()> {
    let (mut input, mut output) = tokio::io::split(stream);
    let mut remote = channel.make_writer();
    let result=async {
        let upload=tokio::io::copy(&mut input,&mut remote);
        tokio::pin!(upload);
        let download=async {loop {match channel.wait().await {
            Some(russh::ChannelMsg::Data{data})=>output.write_all(&data).await?,
            Some(russh::ChannelMsg::Eof|russh::ChannelMsg::Close)|None=>break,
            _=>{},
        }}Ok::<_,anyhow::Error>(())};
        let ended=async {while process.wait().await.is_some(){}bail!("远程桌面服务已结束。")};
        tokio::select!{biased;_=cancel.cancelled()=>{
            // The native decoder closes its local writer after releasing held input.
            // Drain already-written bytes before SSH EOF/close, with a bounded wait.
            let _=tokio::time::timeout(Duration::from_millis(250),&mut upload).await;
            Ok(())
        },result=&mut upload=>result.map(|_|()).map_err(Into::into),result=download=>result,result=ended=>result}
    }.await;
    let _ = channel.eof().await;
    let _ = channel.close().await;
    close_process(process).await;
    result
}

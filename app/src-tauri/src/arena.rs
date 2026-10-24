use crate::{desktop::python,session::DeviceManager};
use anyhow::{Context,Result};
use serde_json::{json,Value};
use std::{sync::{Arc,Mutex},time::{Duration,Instant}};
use tokio_util::sync::CancellationToken;
use tokio::{io::{AsyncReadExt,AsyncWriteExt},net::{TcpListener,TcpStream}};

const MAX_FRAME:usize=1024*1024;
#[derive(Default)]
struct JpegFrames {pending:Vec<u8>}
impl JpegFrames {
    fn push(&mut self,data:&[u8])->Result<Vec<Vec<u8>>>{
        self.pending.extend_from_slice(data);let mut frames=Vec::new();let mut at=0;
        while self.pending.len()-at>=4 {
            let size=u32::from_be_bytes(self.pending[at..at+4].try_into().unwrap())as usize;
            anyhow::ensure!((4..=MAX_FRAME).contains(&size),"摄像头帧长度无效。");
            if self.pending.len()-at<4+size{break;}
            let frame=&self.pending[at+4..at+4+size];
            anyhow::ensure!(frame.starts_with(&[255,216])&&frame.ends_with(&[255,217]),"摄像头返回了无效JPEG。");
            frames.push(frame.to_vec());at+=4+size;
        }
        self.pending.drain(..at);anyhow::ensure!(self.pending.len()<=MAX_FRAME+4,"摄像头帧缓冲超限。");Ok(frames)
    }
}
struct Running {id:u64,cancel:CancellationToken,done:CancellationToken}
struct Camera {id:u64,generation:u64,phase:&'static str,message:String,path:String,backend:String,url:String,fps:u32,sent:u64,sent_bytes:u64,meter:Instant,meter_count:u64,stream_fps:Option<f64>,sequence:u64,latest:Option<Vec<u8>>,viewer:Instant,running:Option<Running>}
#[derive(Clone)]
pub struct ArenaService {devices:DeviceManager,camera:Arc<Mutex<Camera>>,gate:Arc<tokio::sync::Mutex<()>>,changed:Arc<tokio::sync::Notify>}
impl ArenaService {
    pub fn new(devices:DeviceManager)->Self{Self {devices,camera:Arc::new(Mutex::new(Camera{id:0,generation:0,phase:"idle",message:String::new(),path:String::new(),backend:String::new(),url:String::new(),fps:20,sent:0,sent_bytes:0,meter:Instant::now(),meter_count:0,stream_fps:None,sequence:0,latest:None,viewer:Instant::now(),running:None})),gate:Arc::new(tokio::sync::Mutex::new(())),changed:Arc::new(tokio::sync::Notify::new())}}
    pub async fn metrics(&self)->Result<Value>{
        let(generation,connection)=self.devices.connection_with_generation()?;let started=Instant::now();
        let command=python(include_str!("../resources/device_metrics.py"),json!(["metrics"]))?;
        let(output,_)=connection.exec(&command,32000,Duration::from_secs(6)).await?;
        anyhow::ensure!(generation==self.devices.generation(),"设备连接已变化。");
        let mut data:Value=serde_json::from_str(output.trim()).context("设备性能数据无效。")?;
        data["generation"]=json!(generation);data["roundTripMs"]=json!(started.elapsed().as_secs_f64()*1000.0);Ok(data)
    }
    pub async fn cameras(&self)->Result<Value>{
        let(generation,connection)=self.devices.connection_with_generation()?;
        let command=python(include_str!("../resources/camera_list.py"),json!(["cameras"]))?;
        let(output,_)=connection.exec(&command,32000,Duration::from_secs(6)).await?;
        anyhow::ensure!(generation==self.devices.generation(),"设备连接已变化。");
        serde_json::from_str(output.trim()).context("摄像头列表无效。")
    }
    fn metadata(&self,camera:&Camera)->Value{
        let valid=camera.generation==self.devices.generation();
        json!({"id":camera.id,"phase":if valid{camera.phase}else{"idle"},"message":if valid{camera.message.as_str()}else{""},"path":camera.path,"backend":camera.backend,"targetFps":camera.fps,"streamUrl":if valid{camera.url.as_str()}else{""},"transport":"mjpeg-loopback","sequence":camera.sequence,"deliveredFrames":camera.sent,"deliveredBytes":camera.sent_bytes,"streamFps":camera.stream_fps,"frame":null})
    }
    pub fn status(&self)->Value{self.metadata(&self.camera.lock().unwrap())}
    async fn serve(&self,listener:TcpListener,id:u64,token:String,cancel:CancellationToken){
        let pending=Arc::new(tokio::sync::Semaphore::new(4));let viewer=Arc::new(tokio::sync::Semaphore::new(1));
        loop{
            let accepted=tokio::select!{_ = cancel.cancelled()=>break,r=listener.accept()=>r};let Ok((socket,_))=accepted else{break};
            let Ok(permit)=pending.clone().try_acquire_owned()else{continue};let service=self.clone();let token=token.clone();let cancel=cancel.clone();let viewer=viewer.clone();
            tokio::spawn(async move{let _permit=permit;let _=service.stream(socket,id,&token,cancel,viewer).await;});
        }
    }
    async fn stream(&self,mut socket:TcpStream,id:u64,token:&str,cancel:CancellationToken,viewer:Arc<tokio::sync::Semaphore>)->Result<()>{
        let mut request=Vec::new();let mut bytes=[0u8;1024];
        tokio::time::timeout(Duration::from_secs(2),async{
            while !request.windows(4).any(|s|s==b"\r\n\r\n"){
                let n=socket.read(&mut bytes).await?;anyhow::ensure!(n>0,"Preview connection closed");request.extend_from_slice(&bytes[..n]);anyhow::ensure!(request.len()<=4096,"Preview request too large");
            }Ok::<_,anyhow::Error>(())
        }).await??;
        let expected=format!("GET /{} HTTP/1.1",token);let line=request.split(|b|*b==b'\n').next().unwrap_or_default();
        if line.strip_suffix(b"\r").unwrap_or(line)!=expected.as_bytes(){let _=socket.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await;return Ok(());}
        let Ok(_viewer)=viewer.try_acquire_owned()else{let _=socket.write_all(b"HTTP/1.1 409 Conflict\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await;return Ok(());};
        socket.set_nodelay(true)?;
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=portcanvas\r\nCache-Control: no-store, no-cache\r\nPragma: no-cache\r\nConnection: close\r\n\r\n").await?;
        let mut sequence=0;
        loop{
            let (frame,ended)={let camera=self.camera.lock().unwrap();let ended=camera.id!=id||camera.generation!=self.devices.generation()||matches!(camera.phase,"idle"|"error");let frame=if !ended&&camera.sequence>sequence{camera.latest.as_ref().map(|data|(camera.sequence,data.clone()))}else{None};(frame,ended)};
            if ended{break;}
            if let Some((next,data))=frame{
                let head=format!("--portcanvas\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",data.len());let mut packet=Vec::with_capacity(head.len()+data.len()+2);packet.extend_from_slice(head.as_bytes());packet.extend_from_slice(&data);packet.extend_from_slice(b"\r\n");
                tokio::select!{_ = cancel.cancelled()=>break,r=tokio::time::timeout(Duration::from_secs(2),socket.write_all(&packet))=>r??};
                sequence=next;let mut camera=self.camera.lock().unwrap();if camera.id==id{camera.viewer=Instant::now();camera.sent+=1;camera.sent_bytes+=data.len()as u64;camera.meter_count+=1;if camera.meter.elapsed()>=Duration::from_secs(1){camera.stream_fps=Some(camera.meter_count as f64/camera.meter.elapsed().as_secs_f64());camera.meter=Instant::now();camera.meter_count=0;}}
            }else{tokio::select!{_ = cancel.cancelled()=>break,_ = self.changed.notified()=>{},_ = tokio::time::sleep(Duration::from_millis(200))=>{}}}
        }Ok(())
    }
    async fn stop_inner(&self,expected:Option<u64>){
        let old={let mut camera=self.camera.lock().unwrap();if expected.is_some()&&expected!=Some(camera.id){return;}camera.latest=None;camera.url.clear();camera.stream_fps=None;camera.phase="idle";camera.message.clear();camera.running.take()};
        self.changed.notify_one();
        if let Some(old)=old{old.cancel.cancel();let _=tokio::time::timeout(Duration::from_secs(4),old.done.cancelled()).await;}
    }
    pub async fn stop(&self,id:Option<u64>){let _guard=self.gate.lock().await;self.stop_inner(id).await;}
    pub async fn start(&self,path:String,fps:u32)->Result<Value>{
        anyhow::ensure!(regex::Regex::new(r"^/dev/video[0-9]{1,3}$").unwrap().is_match(&path),"请选择有效的视频设备。");
        anyhow::ensure!([8,15,20,30].contains(&fps),"请选择有效帧率。");
        let _guard=self.gate.lock().await;self.stop_inner(None).await;
        let(generation,connection)=self.devices.connection_with_generation()?;
        let listener=TcpListener::bind(("127.0.0.1",0)).await?;let token=uuid::Uuid::new_v4().simple().to_string();let url=format!("http://127.0.0.1:{}/{}",listener.local_addr()?.port(),token);
        let cancel=connection.cancel.child_token();let done=CancellationToken::new();
        let id={let mut camera=self.camera.lock().unwrap();camera.id+=1;let id=camera.id;camera.generation=generation;camera.phase="connecting";camera.message.clear();camera.path=path.clone();camera.backend.clear();camera.url=url;camera.fps=fps;camera.sent=0;camera.sent_bytes=0;camera.meter=Instant::now();camera.meter_count=0;camera.stream_fps=None;camera.sequence=0;camera.latest=None;camera.viewer=Instant::now();camera.running=Some(Running{id,cancel:cancel.clone(),done:done.clone()});id};
        let http=self.clone();let http_cancel=cancel.clone();tokio::spawn(async move{http.serve(listener,id,token,http_cancel).await;});
        let service=self.clone();tokio::spawn(async move{
            let operation=async {
                let mut process=tokio::select!{_ = cancel.cancelled()=>return Ok::<(),anyhow::Error>(()),result=connection.channel()=>result?};
                let result=async {
                    let command=python(include_str!("../resources/camera_stream.py"),json!(["camera",json!({"path":path,"fps":fps}).to_string()]))?;
                    tokio::select!{_ = cancel.cancelled()=>return Ok(()),result=process.exec(true,command)=>result?};
                    let mut parser=JpegFrames::default();let mut diagnostic=Vec::new();let mut heartbeat=tokio::time::interval(Duration::from_secs(1));
                    let deadline=tokio::time::sleep(Duration::from_secs(12));tokio::pin!(deadline);
                    loop {
                        let message=tokio::select!{
                            _ = cancel.cancelled()=>return Ok(()),
                            _ = &mut deadline=>anyhow::bail!("摄像头画面超时。"),
                            _ = heartbeat.tick()=>{if service.camera.lock().unwrap().viewer.elapsed()>Duration::from_secs(3){cancel.cancel();return Ok(());}tokio::select!{_ = cancel.cancelled()=>return Ok(()),result=tokio::time::timeout(Duration::from_secs(2),process.data(&b"."[..]))=>result.context("摄像头连接超时。")??};continue;},
                            message=process.wait()=>message
                        };
                        match message {
                            Some(russh::ChannelMsg::Data{data})=>for frame in parser.push(&data)?{deadline.as_mut().reset(tokio::time::Instant::now()+Duration::from_secs(12));let mut camera=service.camera.lock().unwrap();if camera.running.as_ref().map(|r|r.id)==Some(id)&&!cancel.is_cancelled(){camera.sequence+=1;camera.latest=Some(frame);camera.phase="connected";service.changed.notify_one();}},
                            Some(russh::ChannelMsg::ExtendedData{data,..})=>{
                                let count=data.len().min(4000-diagnostic.len());diagnostic.extend_from_slice(&data[..count]);
                                let text=String::from_utf8_lossy(&diagnostic);for line in text.lines(){if let Some(backend)=line.strip_prefix("PORTCANVAS_CAMERA:"){if matches!(backend,"ffmpeg-copy"|"opencv"){let mut camera=service.camera.lock().unwrap();if camera.id==id{camera.backend=backend.to_string();}}}}
                            },
                            Some(russh::ChannelMsg::ExitStatus{..})|None=>{let error=String::from_utf8_lossy(&diagnostic).lines().filter(|line|!line.starts_with("PORTCANVAS_CAMERA:")).collect::<Vec<_>>().join("\n");anyhow::bail!("{}",if error.trim().is_empty(){"摄像头采集已结束。"}else{error.trim()});},
                            _=>{}
                        }
                    }
                }.await;
                let _=process.eof().await;
                let _=tokio::time::timeout(Duration::from_secs(2),async{while process.wait().await.is_some(){}}).await;
                let _=process.close().await;result
            }.await;
            {let mut camera=service.camera.lock().unwrap();if camera.running.as_ref().map(|r|r.id)==Some(id){camera.phase=if cancel.is_cancelled(){"idle"}else{"error"};camera.message=operation.err().map(|e|e.to_string()).unwrap_or_default();camera.latest=None;camera.running=None;}}
            service.changed.notify_one();cancel.cancel();done.cancel();
        });
        Ok(self.status())
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn camera_frames_survive_split_and_combined_ssh_packets(){let frame=vec![255,216,1,2,255,217];let mut packet=(frame.len()as u32).to_be_bytes().to_vec();packet.extend(&frame);let mut parser=JpegFrames::default();assert!(parser.push(&packet[..3]).unwrap().is_empty());let mut rest=packet[3..].to_vec();rest.extend(&packet);assert_eq!(parser.push(&rest).unwrap(),vec![frame.clone(),frame]);assert!(parser.pending.is_empty());}
    #[test]fn camera_frame_sizes_and_jpeg_boundaries_are_checked(){assert!(JpegFrames::default().push(&0u32.to_be_bytes()).is_err());assert!(JpegFrames::default().push(&((MAX_FRAME+1)as u32).to_be_bytes()).is_err());assert!(JpegFrames::default().push(&[0,0,0,4,0,1,2,3]).is_err());}
}

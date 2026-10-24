use crate::{desktop::python,session::DeviceManager};
use anyhow::{Context,Result};
use base64::{engine::general_purpose::STANDARD,Engine};
use serde_json::{json,Value};
use std::{sync::{Arc,Mutex},time::{Duration,Instant}};
use tokio_util::sync::CancellationToken;

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
struct Camera {id:u64,generation:u64,phase:&'static str,message:String,path:String,sequence:u64,latest:Option<Vec<u8>>,viewer:Instant,running:Option<Running>}
#[derive(Clone)]
pub struct ArenaService {devices:DeviceManager,camera:Arc<Mutex<Camera>>,gate:Arc<tokio::sync::Mutex<()>>}
impl ArenaService {
    pub fn new(devices:DeviceManager)->Self{Self {devices,camera:Arc::new(Mutex::new(Camera{id:0,generation:0,phase:"idle",message:String::new(),path:String::new(),sequence:0,latest:None,viewer:Instant::now(),running:None})),gate:Arc::new(tokio::sync::Mutex::new(()))}}
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
    pub fn frame(&self,id:u64,after:u64)->Value{
        let mut camera=self.camera.lock().unwrap();let valid=camera.generation==self.devices.generation();
        if valid&&id==camera.id{camera.viewer=Instant::now();}
        let frame=if valid&&id==camera.id&&camera.phase=="connected"&&camera.sequence>after{camera.latest.as_ref().map(|data|json!({"sequence":camera.sequence,"data":STANDARD.encode(data)}))}else{None};
        json!({"id":camera.id,"phase":if valid{camera.phase}else{"idle"},"message":if valid{camera.message.as_str()}else{""},"path":camera.path,"frame":frame})
    }
    async fn stop_inner(&self,expected:Option<u64>){
        let old={let mut camera=self.camera.lock().unwrap();if expected.is_some()&&expected!=Some(camera.id){return;}camera.latest=None;camera.phase="idle";camera.message.clear();camera.running.take()};
        if let Some(old)=old{old.cancel.cancel();let _=tokio::time::timeout(Duration::from_secs(4),old.done.cancelled()).await;}
    }
    pub async fn stop(&self,id:Option<u64>){let _guard=self.gate.lock().await;self.stop_inner(id).await;}
    pub async fn start(&self,path:String)->Result<Value>{
        anyhow::ensure!(regex::Regex::new(r"^/dev/video[0-9]{1,3}$").unwrap().is_match(&path),"请选择有效的视频设备。");
        let _guard=self.gate.lock().await;self.stop_inner(None).await;
        let(generation,connection)=self.devices.connection_with_generation()?;
        let cancel=connection.cancel.child_token();let done=CancellationToken::new();
        let id={let mut camera=self.camera.lock().unwrap();camera.id+=1;let id=camera.id;camera.generation=generation;camera.phase="connecting";camera.message.clear();camera.path=path.clone();camera.sequence=0;camera.latest=None;camera.viewer=Instant::now();camera.running=Some(Running{id,cancel:cancel.clone(),done:done.clone()});id};
        let service=self.clone();tokio::spawn(async move{
            let operation=async {
                let mut process=tokio::select!{_ = cancel.cancelled()=>return Ok::<(),anyhow::Error>(()),result=connection.channel()=>result?};
                let result=async {
                    let command=python(include_str!("../resources/camera_stream.py"),json!(["camera",json!({"path":path}).to_string()]))?;
                    tokio::select!{_ = cancel.cancelled()=>return Ok(()),result=process.exec(true,command)=>result?};
                    let mut parser=JpegFrames::default();let mut diagnostic=Vec::new();
                    let mut heartbeat=tokio::time::interval(Duration::from_secs(1));
                    let deadline=tokio::time::sleep(Duration::from_secs(12));tokio::pin!(deadline);
                    loop {
                        let message=tokio::select!{
                            _ = cancel.cancelled()=>return Ok(()),
                            _ = &mut deadline=>anyhow::bail!("摄像头画面超时。"),
                            _ = heartbeat.tick()=>{if service.camera.lock().unwrap().viewer.elapsed()>Duration::from_secs(3){cancel.cancel();return Ok(());}tokio::select!{_ = cancel.cancelled()=>return Ok(()),result=tokio::time::timeout(Duration::from_secs(2),process.data(&b"."[..]))=>result.context("摄像头连接超时。")??};continue;},
                            message=process.wait()=>message
                        };
                        match message {
                            Some(russh::ChannelMsg::Data{data})=>for frame in parser.push(&data)?{deadline.as_mut().reset(tokio::time::Instant::now()+Duration::from_secs(12));let mut camera=service.camera.lock().unwrap();if camera.running.as_ref().map(|r|r.id)==Some(id)&&!cancel.is_cancelled(){camera.sequence+=1;camera.latest=Some(frame);camera.phase="connected";}},
                            Some(russh::ChannelMsg::ExtendedData{data,..})=>{let count=data.len().min(4000-diagnostic.len());diagnostic.extend_from_slice(&data[..count]);},
                            Some(russh::ChannelMsg::ExitStatus{..})|None=>anyhow::bail!("{}",if diagnostic.is_empty(){"摄像头采集已结束。".to_string()}else{String::from_utf8_lossy(&diagnostic).trim().to_string()}),
                            _=>{}
                        }
                    }
                }.await;
                // EOF belongs only to this helper. Its stdin watchdog releases
                // the video device on close, disconnect, or a crashed client.
                let _=process.eof().await;
                let _=tokio::time::timeout(Duration::from_secs(2),async{while process.wait().await.is_some(){}}).await;
                let _=process.close().await;result
            }.await;
            {let mut camera=service.camera.lock().unwrap();if camera.running.as_ref().map(|r|r.id)==Some(id){camera.phase=if cancel.is_cancelled(){"idle"}else{"error"};camera.message=operation.err().map(|e|e.to_string()).unwrap_or_default();camera.latest=None;camera.running=None;}}
            done.cancel();
        });
        Ok(self.frame(id,0))
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn camera_frames_survive_split_and_combined_ssh_packets(){let frame=vec![255,216,1,2,255,217];let mut packet=(frame.len()as u32).to_be_bytes().to_vec();packet.extend(&frame);let mut parser=JpegFrames::default();assert!(parser.push(&packet[..3]).unwrap().is_empty());let mut rest=packet[3..].to_vec();rest.extend(&packet);assert_eq!(parser.push(&rest).unwrap(),vec![frame.clone(),frame]);assert!(parser.pending.is_empty());}
    #[test]fn camera_frame_sizes_and_jpeg_boundaries_are_checked(){assert!(JpegFrames::default().push(&0u32.to_be_bytes()).is_err());assert!(JpegFrames::default().push(&((MAX_FRAME+1)as u32).to_be_bytes()).is_err());assert!(JpegFrames::default().push(&[0,0,0,4,0,1,2,3]).is_err());}
}

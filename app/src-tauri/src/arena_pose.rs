use crate::{desktop::python,session::{DeviceManager,EventSink}};
use anyhow::Result;
use serde_json::{json,Value};
use std::sync::{Arc,Mutex};
use std::time::{Duration,Instant};
use tokio_util::sync::CancellationToken;

pub fn interpret_pose(packet:&Value)->Value{
    if packet["source"]=="missing"{return json!({"phase":"unavailable"});}
    let invalid=||json!({"phase":"invalid"});if packet["source"]!="sample"{return invalid();}
    let sample=&packet["sample"];if sample["schema"]!="portcanvas.pose/1"{return invalid();}
    if sample["valid"]==false{return json!({"phase":"unavailable"});}
    if !sample["valid"].is_null()&&!sample["valid"].is_boolean(){return invalid();}
    let (Some(x),Some(y),Some(timestamp),Some(now))=(sample["x"].as_f64(),sample["y"].as_f64(),sample["timestamp"].as_f64(),packet["nowMs"].as_f64())else{return invalid();};
    if ![x,y,timestamp,now].iter().all(|n|n.is_finite())||!(0.0..=3.8).contains(&x)||!(0.0..=3.8).contains(&y)||now<timestamp{return invalid();}
    let heading=if sample["heading"].is_null(){None}else{let Some(n)=sample["heading"].as_f64()else{return invalid();};if !n.is_finite(){return invalid();}Some(n.rem_euclid(360.0))};
    let age=now-timestamp;if age>2000.0{return json!({"phase":"stale"});}
    let confidence=if sample["confidence"].is_null(){None}else{let Some(n)=sample["confidence"].as_f64()else{return invalid();};if !n.is_finite()||!(0.0..=1.0).contains(&n){return invalid();}Some(n)};
    json!({"phase":"live","x":x,"y":y,"heading":heading,"confidence":confidence,"ageMs":age})
}
struct Running{id:u64,cancel:CancellationToken,done:CancellationToken}
struct State{id:u64,generation:u64,value:Value,viewer:Instant,running:Option<Running>}
#[derive(Clone)]
pub struct PoseService{devices:DeviceManager,events:EventSink,state:Arc<Mutex<State>>,gate:Arc<tokio::sync::Mutex<()>>}
impl PoseService{
    pub fn new(devices:DeviceManager,events:EventSink)->Self{Self{devices,events,state:Arc::new(Mutex::new(State{id:0,generation:0,value:json!({"phase":"unavailable"}),viewer:Instant::now(),running:None})),gate:Arc::new(tokio::sync::Mutex::new(()))}}
    pub fn touch(&self){self.state.lock().unwrap().viewer=Instant::now();}
    pub fn snapshot(&self)->Value{
        let state=self.state.lock().unwrap();let mut value=if state.generation==self.devices.generation(){state.value.clone()}else{json!({"phase":"unavailable"})};value["id"]=json!(state.id);value["generation"]=json!(state.generation);value
    }
    fn publish(&self,id:u64,value:Value){
        {let mut state=self.state.lock().unwrap();if state.running.as_ref().map(|r|r.id)!=Some(id)||state.generation!=self.devices.generation(){return;}state.value=value;}
        (self.events)("arena-pose",self.snapshot());
    }
    async fn stop_inner(&self,id:Option<u64>){
        let old={let mut state=self.state.lock().unwrap();if id.is_some()&&id!=Some(state.id){return;}state.value=json!({"phase":"unavailable"});state.running.take()};
        if let Some(old)=old{old.cancel.cancel();let _=tokio::time::timeout(Duration::from_secs(3),old.done.cancelled()).await;}
    }
    pub async fn stop(&self,id:Option<u64>){let _guard=self.gate.lock().await;self.stop_inner(id).await;}
    pub async fn start(&self)->Result<Value>{
        let _guard=self.gate.lock().await;self.stop_inner(None).await;
        let(generation,connection)=self.devices.connection_with_generation()?;
        let path=std::env::var("PORTCANVAS_POSE_PATH").unwrap_or_else(|_|"/dev/shm/portcanvas-pose.json".into());
        anyhow::ensure!(path.starts_with('/')&&path.len()<=1024&&!path.contains('\0'),"定位数据路径无效。");
        let cancel=connection.cancel.child_token();let done=CancellationToken::new();
        let id={let mut state=self.state.lock().unwrap();state.id+=1;let id=state.id;state.generation=generation;state.viewer=Instant::now();state.value=json!({"phase":"unavailable"});state.running=Some(Running{id,cancel:cancel.clone(),done:done.clone()});id};
        let service=self.clone();tokio::spawn(async move{
            let operation=async{
                let mut channel=tokio::select!{_ = cancel.cancelled()=>return Ok::<(),anyhow::Error>(()),result=connection.channel()=>result?};
                let result=async{
                    let command=python(include_str!("../resources/pose_stream.py"),json!(["pose",json!({"path":path}).to_string()]))?;
                    tokio::select!{_ = cancel.cancelled()=>return Ok(()),result=channel.exec(true,command)=>result?};
                    let mut pending=Vec::new();let mut heartbeat=tokio::time::interval(Duration::from_secs(1));let deadline=tokio::time::sleep(Duration::from_secs(3));tokio::pin!(deadline);
                    loop{
                        let message=tokio::select!{_ = cancel.cancelled()=>return Ok(()),_ = &mut deadline=>anyhow::bail!("定位数据超时。"),_ = heartbeat.tick()=>{if service.state.lock().unwrap().viewer.elapsed()>Duration::from_secs(10){cancel.cancel();return Ok(());}tokio::select!{_ = cancel.cancelled()=>return Ok(()),r=tokio::time::timeout(Duration::from_secs(2),channel.data(&b"."[..]))=>r??};continue;},m=channel.wait()=>m};
                        match message{
                            Some(russh::ChannelMsg::Data{data})=>{
                                deadline.as_mut().reset(tokio::time::Instant::now()+Duration::from_secs(3));pending.extend_from_slice(&data);anyhow::ensure!(pending.len()<=16384,"定位数据过长。");
                                while let Some(end)=pending.iter().position(|b|*b==b'\n'){let line=pending.drain(..=end).collect::<Vec<_>>();let value=serde_json::from_slice::<Value>(&line).map(|packet|{let mut value=interpret_pose(&packet);value["helperPid"]=packet["pid"].clone();value}).unwrap_or_else(|_|json!({"phase":"invalid"}));service.publish(id,value);}
                            },
                            Some(russh::ChannelMsg::ExitStatus{..})|None=>anyhow::bail!("定位通道已关闭。"),
                            _=>{}
                        }
                    }
                }.await;
                let _=channel.eof().await;let _=tokio::time::timeout(Duration::from_secs(1),async{while channel.wait().await.is_some(){}}).await;let _=channel.close().await;result
            }.await;
            service.publish(id,json!({"phase":"unavailable","message":if cancel.is_cancelled(){String::new()}else{operation.err().map(|e|e.to_string()).unwrap_or_default()}}));
            {let mut state=service.state.lock().unwrap();if state.running.as_ref().map(|r|r.id)==Some(id){state.running=None;}}
            done.cancel();
        });Ok(self.snapshot())
    }
}
#[cfg(test)]mod tests{
    use super::*;
    fn packet(x:Value,y:Value,timestamp:f64)->Value{json!({"source":"sample","nowMs":5000,"sample":{"schema":"portcanvas.pose/1","x":x,"y":y,"timestamp":timestamp,"heading":-90}})}
    #[test]fn genuine_pose_is_in_meters_with_normalized_heading(){let v=interpret_pose(&packet(json!(1.2),json!(3.8),4900.0));assert_eq!(v["phase"],"live");assert_eq!(v["x"],1.2);assert_eq!(v["heading"],270.0);}
    #[test]fn absent_and_expired_data_never_become_a_live_point(){assert_eq!(interpret_pose(&json!({"source":"missing"}))["phase"],"unavailable");assert_eq!(interpret_pose(&packet(json!(1),json!(1),2000.0))["phase"],"stale");}
    #[test]fn invalid_coordinates_and_timestamps_are_rejected(){for p in [packet(json!(-0.1),json!(1),4900.0),packet(json!(1),json!(3.81),4900.0),packet(json!("1"),json!(1),4900.0),packet(json!(1),json!(1),5100.0)]{assert_eq!(interpret_pose(&p)["phase"],"invalid");}}
    #[test]fn unknown_schema_and_boolean_coordinates_are_not_data(){let mut p=packet(json!(true),json!(1),4900.0);assert_eq!(interpret_pose(&p)["phase"],"invalid");p["sample"]["x"]=json!(1);p["sample"]["schema"]=json!("other");assert_eq!(interpret_pose(&p)["phase"],"invalid");}
    #[test]fn explicit_tracking_loss_needs_no_coordinates(){assert_eq!(interpret_pose(&json!({"source":"sample","sample":{"schema":"portcanvas.pose/1","valid":false}}))["phase"],"unavailable");let mut p=packet(json!(1),json!(1),4900.0);p["sample"]["valid"]=json!("false");assert_eq!(interpret_pose(&p)["phase"],"invalid");}
    #[test]fn confidence_is_optional_and_bounded(){for n in [0.0,1.0]{let mut p=packet(json!(1),json!(1),4900.0);p["sample"]["confidence"]=json!(n);assert_eq!(interpret_pose(&p)["phase"],"live");}for n in [json!(-0.1),json!(1.01),json!("0.8")]{let mut p=packet(json!(1),json!(1),4900.0);p["sample"]["confidence"]=n;assert_eq!(interpret_pose(&p)["phase"],"invalid");}}
}


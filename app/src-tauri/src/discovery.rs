use crate::session::EventSink;
use serde_json::{json,Value};
use std::{collections::BTreeSet,net::{Ipv4Addr,SocketAddr},sync::{Arc,Mutex},time::Duration};
use tokio::{io::AsyncReadExt,net::TcpStream};
use tokio_util::sync::CancellationToken;
use futures_util::{stream,StreamExt};

#[derive(Clone)]pub struct Discovery{inner:Arc<Inner>}
struct Inner{state:Mutex<Value>,cancel:Mutex<CancellationToken>,events:EventSink}
impl Discovery{
 pub fn new(events:EventSink)->Self{Self{inner:Arc::new(Inner{state:Mutex::new(json!({"status":"idle","scanId":0,"checked":0,"total":0,"devices":[]})),cancel:Mutex::new(CancellationToken::new()),events})}}
 pub fn snapshot(&self)->Value{self.inner.state.lock().unwrap().clone()}
 fn emit(&self){(self.inner.events)("discovery-state",self.snapshot());}
 pub fn cancel(&self){self.inner.cancel.lock().unwrap().cancel();}
 pub fn start(&self)->Value{
  let mut state=self.inner.state.lock().unwrap();if state["status"]=="searching"{return state.clone();}
  let id=state["scanId"].as_u64().unwrap_or(0)+1;let cancel=CancellationToken::new();*self.inner.cancel.lock().unwrap()=cancel.clone();
  *state=json!({"status":"searching","scanId":id,"checked":0,"total":0,"devices":[],"limited":false});let result=state.clone();drop(state);self.emit();
  let service=self.clone();tokio::spawn(async move{service.run(id,cancel).await;});result
 }
 async fn run(&self,id:u64,cancel:CancellationToken){
  let targets=tokio::task::spawn_blocking(local_targets).await;
  let (targets,limited)=match targets{Ok(Ok(value))=>value,other=>{let mut state=self.inner.state.lock().unwrap();state["status"]=json!("error");state["message"]=json!(format!("局域网搜索失败：{other:?}"));drop(state);self.emit();return;}};
  {let mut state=self.inner.state.lock().unwrap();state["total"]=json!(targets.len());state["limited"]=json!(limited);}self.emit();
  let results=stream::iter(targets).map(|host|async move{let found=probe(SocketAddr::from((host,22))).await;found.then_some(host)}).buffer_unordered(128);
  tokio::pin!(results);
  loop{let item=tokio::select!{_ = cancel.cancelled()=>break,item=results.next()=>item};let Some(found)=item else{break};
   let mut state=self.inner.state.lock().unwrap();if state["scanId"]!=id{break;}let checked=state["checked"].as_u64().unwrap_or(0)+1;state["checked"]=json!(checked);
   if let Some(host)=found{state["devices"].as_array_mut().unwrap().push(json!({"host":host.to_string(),"port":22}));}
   let emit=found.is_some()||checked%32==0;drop(state);if emit{self.emit();}
  }
  {let mut state=self.inner.state.lock().unwrap();if state["scanId"]!=id{return;}state["status"]=json!(if cancel.is_cancelled(){"canceled"}else{"complete"});}self.emit();
 }
}
fn subnet_targets(ip:Ipv4Addr,prefix:u8)->(Vec<Ipv4Addr>,bool){
 let effective=prefix.max(16);let mask=if effective==0{0}else{u32::MAX.checked_shl((32-effective)as u32).unwrap_or(0)};let base=u32::from(ip)&mask;let count=1u32.checked_shl((32-effective)as u32).unwrap_or(0);
 let mut hosts:Vec<_>=(0..count).filter(|offset|effective>=31||(*offset>0&&*offset<count-1)).map(|offset|Ipv4Addr::from(base+offset)).filter(|host|*host!=ip).collect();
 hosts.sort_by_key(|host|u32::from(*host).abs_diff(u32::from(ip)));(hosts,prefix<16)
}
fn local_targets()->std::io::Result<(Vec<Ipv4Addr>,bool)>{
 let mut targets=BTreeSet::new();let mut locals=BTreeSet::new();let mut limited=false;
 for interface in if_addrs::get_if_addrs()?{if !interface.is_oper_up()||interface.is_loopback(){continue;}if let if_addrs::IfAddr::V4(address)=interface.addr{
  if !(address.ip.is_private()||address.ip.is_link_local()){continue;}locals.insert(address.ip);let (hosts,clipped)=subnet_targets(address.ip,address.prefixlen);limited|=clipped;targets.extend(hosts);
 }}
 for ip in locals.iter(){targets.remove(ip);}let mut targets:Vec<_>=targets.into_iter().collect();targets.sort_by_key(|ip|locals.iter().map(|local|u32::from(*ip).abs_diff(u32::from(*local))).min().unwrap_or(0));
 if targets.len()>65536{targets.truncate(65536);limited=true;}Ok((targets,limited))
}
async fn probe(address:SocketAddr)->bool{
 tokio::time::timeout(Duration::from_millis(900),async{
  let Ok(Ok(mut socket))=tokio::time::timeout(Duration::from_millis(400),TcpStream::connect(address)).await else{return false;};
  let mut line=Vec::new();let mut byte=[0;1];let mut total=0;
  while total<1024{match socket.read(&mut byte).await{Ok(1)=>{total+=1;line.push(byte[0]);if byte[0]==b'\n'{if line.starts_with(b"SSH-2.0-")||line.starts_with(b"SSH-1.99-"){return line.len()<=255;}line.clear();}},_=>return false}}
  false
 }).await.unwrap_or(false)
}
#[cfg(test)]mod tests{
 use super::*;use tokio::io::AsyncWriteExt;
 #[test]fn subnet_respects_prefix_and_excludes_self_network_broadcast(){let (hosts,limited)=subnet_targets(Ipv4Addr::new(192,168,2,10),24);assert_eq!(hosts.len(),253);assert!(!limited);for ip in [0,10,255]{assert!(!hosts.contains(&Ipv4Addr::new(192,168,2,ip)));}assert_eq!(subnet_targets(Ipv4Addr::new(10,0,0,1),32).0.len(),0);assert!(subnet_targets(Ipv4Addr::new(10,0,0,1),8).1);}
 #[tokio::test]async fn only_ssh_banners_are_discovered_without_sending_credentials(){
  for (banner,expected) in [(b"SSH-2.0-OpenSSH_8.2\r\n".as_slice(),true),(b"notice\r\nSSH-1.99-test\r\n",true),(b"HTTP/1.1 200 OK\r\n",false)]{
   let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();let banner=banner.to_vec();let server=tokio::spawn(async move{let (mut socket,_)=listener.accept().await.unwrap();socket.write_all(&banner).await.unwrap();socket.shutdown().await.unwrap();let mut bytes=Vec::new();socket.read_to_end(&mut bytes).await.unwrap();assert!(bytes.is_empty());});assert_eq!(probe(address).await,expected);server.await.unwrap();
  }
 }
}

use anyhow::Result;
use std::{path::PathBuf,sync::{Arc,Mutex},time::Duration};
use tokio::{io::{AsyncReadExt,AsyncSeekExt,AsyncWriteExt},net::{TcpListener,TcpStream},sync::Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct BackgroundMedia{inner:Arc<Inner>}
struct Inner{base:String,saved:Mutex<Option<PathBuf>>,draft:Mutex<Option<PathBuf>>,stop:CancellationToken}
impl BackgroundMedia{
    pub async fn new()->Result<Self>{
        let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).await?;
        let service=Self{inner:Arc::new(Inner{base:format!("http://127.0.0.1:{}/{}/",listener.local_addr()?.port(),uuid::Uuid::new_v4().simple()),saved:Mutex::new(None),draft:Mutex::new(None),stop:CancellationToken::new()})};
        let running=service.clone();tokio::spawn(async move{
            let slots=Arc::new(Semaphore::new(8));
            loop{tokio::select!{_ = running.inner.stop.cancelled()=>break,result=listener.accept()=>{
                let Ok((socket,_))=result else{break};let Ok(slot)=slots.clone().try_acquire_owned() else{continue};let service=running.clone();
                tokio::spawn(async move{let _slot=slot;tokio::select!{_ = service.inner.stop.cancelled()=>{},_ = service.serve(socket)=>{}}});
            }}}
        });Ok(service)
    }
    pub fn stop(&self){self.inner.stop.cancel();}
    pub fn saved(&self,path:Option<PathBuf>)->Option<String>{*self.inner.saved.lock().unwrap()=path.clone();path.map(|p|self.url(&p))}
    pub fn draft(&self,path:Option<PathBuf>)->Option<String>{*self.inner.draft.lock().unwrap()=path.clone();path.map(|p|self.url(&p))}
    fn url(&self,path:&std::path::Path)->String{format!("{}{}",self.inner.base,path.file_name().unwrap().to_string_lossy())}
    async fn serve(&self,mut socket:TcpStream)->Result<()>{
        let mut headers=Vec::new();let mut byte=[0;1];
        tokio::time::timeout(Duration::from_secs(3),async{
            while headers.len()<16384{if socket.read(&mut byte).await?==0{break;}headers.push(byte[0]);if headers.ends_with(b"\r\n\r\n"){break;}}
            Ok::<_,std::io::Error>(())
        }).await??;
        let text=std::str::from_utf8(&headers)?;let mut lines=text.lines();let mut first=lines.next().unwrap_or("").split_whitespace();
        let method=first.next().unwrap_or("");let target=first.next().unwrap_or("");
        let prefix=self.inner.base.split_once("127.0.0.1:").unwrap().1.split_once('/').unwrap().1;
        let expected=format!("/{prefix}");
        let path=target.strip_prefix(&expected).and_then(|name|{
            [self.inner.saved.lock().unwrap().clone(),self.inner.draft.lock().unwrap().clone()].into_iter().flatten().find(|path|path.file_name().is_some_and(|n|n==name))
        });
        if !matches!(method,"GET"|"HEAD")||path.is_none(){socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;return Ok(());}
        let path=path.unwrap();let mut file=match tokio::fs::File::open(&path).await{Ok(file)=>file,Err(_)=>{socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;return Ok(());}};
        let size=file.metadata().await?.len();let range=lines.find_map(|line|line.split_once(':').filter(|(key,_)|key.eq_ignore_ascii_case("Range")).map(|(_,value)|value.trim()));
        let (start,end,partial)=match byte_range(range,size){Some(range)=>range,None=>{socket.write_all(format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{size}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await?;return Ok(());}};
        let length=end-start+1;let content_range=if partial{format!("Content-Range: bytes {start}-{end}/{size}\r\n")}else{String::new()};
        let extension=path.extension().and_then(|e|e.to_str()).unwrap_or("");
        let mime=match extension{"png"=>"image/png","jpg"=>"image/jpeg","webp"=>"image/webp","mp4"=>"video/mp4","webm"=>"video/webm",_=>return Ok(())};
        socket.write_all(format!("HTTP/1.1 {}\r\nContent-Type: {mime}\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\n{content_range}Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",if partial{"206 Partial Content"}else{"200 OK"}).as_bytes()).await?;
        if method=="HEAD"{return Ok(());}file.seek(std::io::SeekFrom::Start(start)).await?;
        tokio::io::copy(&mut file.take(length),&mut socket).await?;Ok(())
    }
}
fn byte_range(header:Option<&str>,size:u64)->Option<(u64,u64,bool)>{
    if size==0{return None;}let Some(header)=header else{return Some((0,size-1,false));};
    let (start,end)=header.strip_prefix("bytes=")?.split_once('-')?;
    if start.is_empty(){let count=end.parse::<u64>().ok()?;if count==0{return None;}return Some((size.saturating_sub(count),size-1,true));}
    let start=start.parse::<u64>().ok()?;let end=if end.is_empty(){size-1}else{end.parse::<u64>().ok()?.min(size-1)};
    (start<size&&end>=start).then_some((start,end,true))
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn ranges_cover_head_tail_and_invalid_requests(){assert_eq!(byte_range(None,20),Some((0,19,false)));assert_eq!(byte_range(Some("bytes=4-8"),20),Some((4,8,true)));assert_eq!(byte_range(Some("bytes=18-"),20),Some((18,19,true)));assert_eq!(byte_range(Some("bytes=-4"),20),Some((16,19,true)));for range in ["bytes=20-","bytes=7-2","bytes=0-1,3-4","bytes=-0"]{assert_eq!(byte_range(Some(range),20),None);}}
 #[tokio::test]async fn media_streams_ranges_and_rejects_unregistered_paths(){
  let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();let path=root.join("clip.mp4");std::fs::write(&path,b"0123456789").unwrap();
  let service=BackgroundMedia::new().await.unwrap();let url=service.saved(Some(path)).unwrap();let parsed=url.split_once("://").unwrap().1;let (host,target)=parsed.split_once('/').unwrap();
  async fn request(host:&str,target:&str,range:&str)->Vec<u8>{let mut socket=TcpStream::connect(host).await.unwrap();socket.write_all(format!("GET /{target} HTTP/1.1\r\nHost: {host}\r\n{range}\r\n").as_bytes()).await.unwrap();let mut bytes=Vec::new();socket.read_to_end(&mut bytes).await.unwrap();bytes}
  let response=request(host,target,"Range: bytes=3-6\r\n").await;let text=String::from_utf8(response).unwrap();assert!(text.starts_with("HTTP/1.1 206"));assert!(text.contains("Content-Range: bytes 3-6/10"));assert!(text.ends_with("3456"));
  let unknown=request(host,"wrong/clip.mp4","").await;assert!(unknown.starts_with(b"HTTP/1.1 404"));service.saved(None);assert!(request(host,target,"").await.starts_with(b"HTTP/1.1 404"));service.stop();std::fs::remove_dir_all(root).unwrap();
 }
}

use crate::{session::DeviceManager, ssh::SshConnection};
use anyhow::{bail, Context, Result};
use russh_sftp::{
    client::{error::Error as SftpError, RawSftpSession, SftpSession},
    protocol::{FileAttributes, OpenFlags, Packet, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, UNIX_EPOCH},
};
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
};
use tokio_util::sync::CancellationToken;
pub type Confirmation = Arc<
    dyn Fn(Vec<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send>>
        + Send
        + Sync,
>;
pub fn local_drives() -> Result<Value> {
    #[cfg(windows)] {
        use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW,GetLogicalDrives};
        let mask=unsafe{GetLogicalDrives()};
        if mask==0 { return Err(std::io::Error::last_os_error()).context("无法列出本机盘符。"); }
        let mut drives=Vec::new();
        for index in 0..26 {
            if mask & (1<<index)==0 { continue; }
            let path=format!("{}:\\",(b'A'+index as u8) as char);
            let wide=path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let kind=match unsafe{GetDriveTypeW(wide.as_ptr())} {2=>"可移动磁盘",3=>"本地磁盘",4=>"网络磁盘",5=>"光驱",6=>"内存盘",_=>"磁盘"};
            drives.push(json!({"path":path,"label":format!("{path} · {kind}")}));
        }
        Ok(json!(drives))
    }
    #[cfg(not(windows))] { Ok(json!([{"path":"/","label":"/ · 文件系统"}])) }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Local,
    Remote,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Directory,
    File,
    Link,
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Meta {
    kind: Kind,
    size: Option<u64>,
    modified: Option<u64>,
}
fn local_meta(meta: std::fs::Metadata) -> Meta {
    let kind = if meta.file_type().is_symlink() {
        Kind::Link
    } else if meta.is_dir() {
        Kind::Directory
    } else if meta.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    Meta {
        kind,
        size: Some(meta.len()),
        modified: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64),
    }
}
fn remote_meta(meta: FileAttributes) -> Meta {
    let kind = match meta.permissions.unwrap_or(0) & 0o170000 {
        0o040000 => Kind::Directory,
        0o100000 => Kind::File,
        0o120000 => Kind::Link,
        _ => Kind::Other,
    };
    Meta {
        kind,
        size: meta.size,
        modified: meta.mtime.map(|v| v as u64 * 1000),
    }
}
fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        || matches!(error.downcast_ref::<SftpError>(),Some(SftpError::Status(s))if s.status_code==StatusCode::NoSuchFile)
}
pub fn file_error(error: &anyhow::Error) -> String {
    if missing(error) {
        "目录或文件不存在，请刷新后重试。".into()
    } else if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
        || matches!(error.downcast_ref::<SftpError>(),Some(SftpError::Status(s))if s.status_code==StatusCode::PermissionDenied)
    {
        "没有访问权限，请检查目录或文件权限。".into()
    } else {
        error.to_string()
    }
}
pub fn leaf(name: &str, windows: bool) -> Result<()> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\0']) {
        bail!("文件名无效。");
    }
    if windows
        && (name.chars().any(|c| c < ' ' || "\\<>:\"|?*".contains(c))
            || name.ends_with(['.', ' '])
            || regex::Regex::new(r"(?i)^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\.|$)")?.is_match(name))
    {
        bail!("Windows不支持此文件名：{name}");
    }
    Ok(())
}
fn posix(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}
fn posix_parent(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(p, _)| if p.is_empty() { "/" } else { p })
        .unwrap_or("/")
        .into()
}
#[derive(Clone)]
enum Access {
    Local,
    Remote(Arc<SftpSession>),
}
impl Access {
    fn join(&self, parent: &str, name: &str) -> String {
        match self {
            Self::Local => Path::new(parent).join(name).to_string_lossy().into_owned(),
            Self::Remote(_) => format!("{}/{}", parent.trim_end_matches('/'), name),
        }
    }
    async fn stat(&self, path: &str) -> Result<Meta> {
        Ok(match self {
            Self::Local => local_meta(fs::symlink_metadata(path).await?),
            Self::Remote(s) => remote_meta(s.symlink_metadata(path).await?),
        })
    }
    async fn optional(&self, path: &str) -> Result<Option<Meta>> {
        match self.stat(path).await {
            Ok(m) => Ok(Some(m)),
            Err(e) if missing(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }
    async fn list(&self, path: &str) -> Result<Vec<(String, Meta)>> {
        match self {
            Self::Local => {
                let mut entries = fs::read_dir(path).await?;
                let mut result = Vec::new();
                while let Some(entry) = entries.next_entry().await? {
                    result.push((
                        entry
                            .file_name()
                            .into_string()
                            .map_err(|_| anyhow::anyhow!("文件名不是有效Unicode。"))?,
                        local_meta(fs::symlink_metadata(entry.path()).await?),
                    ));
                }
                Ok(result)
            }
            Self::Remote(s) => Ok(s
                .read_dir(path)
                .await?
                .map(|e| (e.file_name(), remote_meta(e.metadata())))
                .collect()),
        }
    }
    async fn mkdir(&self, path: &str) -> Result<()> {
        match self {
            Self::Local => fs::create_dir(path).await?,
            Self::Remote(s) => s.create_dir(path).await?,
        };
        Ok(())
    }
    async fn unlink(&self, path: &str) -> Result<()> {
        match self {
            Self::Local => fs::remove_file(path).await?,
            Self::Remote(s) => s.remove_file(path).await?,
        };
        Ok(())
    }
    async fn read(&self, path: &str) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        Ok(match self {
            Self::Local => Box::new(fs::File::open(path).await?),
            Self::Remote(s) => Box::new(s.open(path).await?),
        })
    }
    async fn write(&self, path: &str) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        Ok(match self {
            Self::Local => Box::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .await?,
            ),
            Self::Remote(s) => Box::new(
                s.open_with_flags(
                    path,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                )
                .await?,
            ),
        })
    }
    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        match self {
            Self::Local => fs::rename(from, to).await?,
            Self::Remote(s) => s.rename(from, to).await?,
        };
        Ok(())
    }
}
struct Cache {
    generation: u64,
    session: Arc<SftpSession>,
    home: String,
}
#[derive(Clone)]
struct OwnedTemp {
    access: Access,
    path: String,
    remote: bool,
    identity: String,
}
struct Job {
    cancel: CancellationToken,
    user: AtomicBool,
    temps: Mutex<Vec<OwnedTemp>>,
    rename: Mutex<Option<Arc<RawSftpSession>>>,
}
struct Inner {
    state: Value,
    cache: Option<Cache>,
    job: Option<Arc<Job>>,
    pending: Vec<(String, String)>,
}
#[derive(Clone)]
pub struct FileService {
    inner: Arc<Mutex<Inner>>,
    opening: Arc<tokio::sync::Mutex<()>>,
    devices: DeviceManager,
    home: PathBuf,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    direction: String,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    local_items: Vec<String>,
    #[serde(default)]
    local_path: String,
    remote_path: String,
    #[serde(default)]
    rate_limit: u64,
}
fn dropped_sources(options:&Transfer)->Result<Vec<(String,String)>> {
    if options.local_items.is_empty(){return Ok(vec![]);}
    anyhow::ensure!(options.direction=="upload"&&options.names.is_empty(),"拖入文件仅支持上传，不能混合两种源列表。");
    let mut selected=HashSet::new();let mut sources=Vec::new();
    for input in &options.local_items {
        let path=Path::new(input);anyhow::ensure!(path.is_absolute(),"拖入文件须为本机绝对路径。");
        let name=path.file_name().and_then(|s|s.to_str()).context("不能直接传输磁盘根目录。")?;leaf(name,cfg!(windows))?;
        if selected.insert(path.to_path_buf()){sources.push((input.clone(),name.to_string()));}
    }Ok(sources)
}
struct Item {
    source: String,
    target: String,
    meta: Meta,
    existing: Option<Meta>,
}
impl FileService {
    pub fn new(devices: DeviceManager, home: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                state: json!({"connection":"disconnected","phase":"idle","busy":false,"bytes":0,"totalBytes":0,"completed":0,"totalFiles":0,"speed":0,"message":"选择文件或文件夹，开始双向传输。"}),
                cache: None,
                job: None,
                pending: vec![],
            })),
            opening: Arc::new(tokio::sync::Mutex::new(())),
            devices,
            home,
        }
    }
    pub fn snapshot(&self) -> Value {
        self.inner.lock().unwrap().state.clone()
    }
    fn update(&self, patch: Value) {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            for (k, v) in patch.as_object().unwrap() {
                inner.state[k] = v.clone();
            }
            inner.state["generation"] = json!(self.devices.generation());
            inner.state.clone()
        };
        (self.devices.events)("file-state", state);
    }
    pub fn cancel(&self) {
        let job = self.inner.lock().unwrap().job.clone();
        if let Some(job) = job {
            job.user.store(true, Ordering::SeqCst);
            job.cancel.cancel();
            self.update(json!({"phase":"canceling","message":"正在取消传输…"}));
        }
    }
    pub fn invalidate(&self) {
        let job = {
            let mut inner = self.inner.lock().unwrap();
            inner.cache = None;
            inner.job.clone()
        };
        if let Some(job) = job {
            job.cancel.cancel();
        }
        self.update(json!({"connection":"disconnected"}));
    }
    fn discard_session(&self, failed: &Arc<SftpSession>) {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            let Some(cache) = inner.cache.as_ref() else {
                return;
            };
            if !Arc::ptr_eq(&cache.session, failed) {
                return;
            }
            let generation = cache.generation;
            inner.cache = None;
            if inner.state["generation"].as_u64().unwrap_or(0) > generation {
                None
            } else {
                inner.state["connection"] = json!("disconnected");
                inner.state["generation"] = json!(generation);
                Some(inner.state.clone())
            }
        };
        if let Some(state) = state {
            (self.devices.events)("file-state", state);
        }
    }
    async fn sftp(&self) -> Result<(u64, Arc<SftpSession>, String, Arc<SshConnection>)> {
        let _opening = self.opening.lock().await;
        let (generation, connection) = self.devices.connection_with_generation()?;
        {
            let inner = self.inner.lock().unwrap();
            if let Some(cache) = &inner.cache {
                if cache.generation == generation {
                    return Ok((
                        generation,
                        cache.session.clone(),
                        cache.home.clone(),
                        connection,
                    ));
                }
            }
        }
        self.update(json!({"connection":"connecting"}));
        let opening: Result<_> = async {
            let sftp = Arc::new(connection.sftp().await?);
            let home = sftp.canonicalize(".").await?;
            Ok((sftp, home))
        }
        .await;
        let (sftp, home) = match opening {
            Ok(value) => value,
            Err(error) => {
                if generation == self.devices.generation() {
                    self.update(json!({"connection":"disconnected"}));
                }
                return Err(error);
            }
        };
        if generation != self.devices.generation() || connection.cancel.is_cancelled() {
            let _ = sftp.close().await;
            bail!("设备连接已变化，请重试。");
        }
        let identity = connection.profile.terminal_name();
        let pending = self.inner.lock().unwrap().pending.clone();
        for (owner, path) in pending {
            if owner == identity {
                match sftp.remove_file(&path).await {
                    Ok(()) => self
                        .inner
                        .lock()
                        .unwrap()
                        .pending
                        .retain(|p| p != &(owner.clone(), path.clone())),
                    Err(e) if matches!(&e,SftpError::Status(s)if s.status_code==StatusCode::NoSuchFile) => {
                        self.inner
                            .lock()
                            .unwrap()
                            .pending
                            .retain(|p| p != &(owner.clone(), path.clone()))
                    }
                    _ => {}
                }
            }
        }
        self.inner.lock().unwrap().cache = Some(Cache {
            generation,
            session: sftp.clone(),
            home: home.clone(),
        });
        self.update(json!({"connection":"connected"}));

        Ok((generation, sftp, home, connection))
    }
    fn local_path(&self, input: &str) -> Result<String> {
        let path = if input.is_empty() {
            self.home.clone()
        } else {
            PathBuf::from(input)
        };
        if !path.is_absolute() || input.contains('\0') {
            bail!("请输入本地绝对目录路径。");
        }
        Ok(path.to_string_lossy().into_owned())
    }
    fn remote_path(input: &str, home: &str) -> Result<String> {
        let path = if input.is_empty() || input == "~" {
            home.into()
        } else if let Some(relative) = input.strip_prefix("~/") {
            format!("{home}/{relative}")
        } else {
            input.into()
        };
        if !path.starts_with('/') || path.contains('\0') {
            bail!("请输入以/开头的远端绝对目录路径。");
        }
        Ok(posix(&path))
    }
    pub async fn list(&self, side: Side, input: String) -> Result<Value> {
        let mut used = None;
        let result=async{let(generation,access,path,home)=if side==Side::Local{let path=self.local_path(&input)?;let path=dunce::canonicalize(path)?.to_string_lossy().into_owned();(self.devices.generation(),Access::Local,path,self.home.to_string_lossy().into_owned())}else{let(g,s,h,_)=self.sftp().await?;used=Some(s.clone());let path=s.canonicalize(Self::remote_path(&input,&h)?).await?;(g,Access::Remote(s),path,h)};
            if access.stat(&path).await?.kind!=Kind::Directory{bail!("该路径不是文件夹。");}let mut entries=access.list(&path).await?;entries.sort_by(|a,b|(a.1.kind!=Kind::Directory).cmp(&(b.1.kind!=Kind::Directory)).then(a.0.cmp(&b.0)));let entries=entries.into_iter().map(|(name,meta)|json!({"name":name,"type":meta.kind,"size":meta.size,"modified":meta.modified})).collect::<Vec<_>>();if side==Side::Remote&&generation!=self.devices.generation(){bail!("设备连接已变化，请刷新。");}let parent=if side==Side::Local{Path::new(&path).parent().unwrap_or(Path::new(&path)).to_string_lossy().into_owned()}else{posix_parent(&path)};Ok(json!({"generation":generation,"path":path,"home":home,"parent":parent,"entries":entries}))}.await;
        if let Err(error) = &result {
            if matches!(
                error.downcast_ref::<SftpError>(),
                Some(
                    SftpError::IO(_)
                        | SftpError::Timeout
                        | SftpError::UnexpectedBehavior(_)
                        | SftpError::UnexpectedPacket
                )
            ) {
                if let Some(session) = used {
                    self.discard_session(&session);
                }
            }
        }
        result.map_err(|e: anyhow::Error| anyhow::anyhow!(file_error(&e)))
    }
    pub async fn mkdir(&self, side: Side, parent: String, name: String) -> Result<String> {
        if self.inner.lock().unwrap().job.is_some() {
            bail!("请等待当前传输结束后再新建文件夹。");
        }
        leaf(&name, side == Side::Local && cfg!(windows))?;
        let (access, parent) = if side == Side::Local {
            (Access::Local, self.local_path(&parent)?)
        } else {
            let (_, s, h, _) = self.sftp().await?;
            (Access::Remote(s), Self::remote_path(&parent, &h)?)
        };
        let path = access.join(&parent, &name);
        access
            .mkdir(&path)
            .await
            .map_err(|e| anyhow::anyhow!(file_error(&e)))?;
        Ok(path)
    }
    pub async fn transfer(&self, options: Transfer, confirm: Confirmation) -> Result<Value> {
        if !["upload", "download"].contains(&options.direction.as_str()) || (options.names.is_empty()&&options.local_items.is_empty())
        {
            bail!("请选择有效的传输方向和文件。");
        }
        if ![0, 1048576, 5242880, 10485760].contains(&options.rate_limit) {
            bail!("无效的限速设置。");
        }
        let dropped=dropped_sources(&options)?;
        let (_, connection) = self.devices.connection_with_generation()?;
        let job = Arc::new(Job {
            cancel: connection.cancel.child_token(),
            user: AtomicBool::new(false),
            temps: Mutex::new(vec![]),
            rename: Mutex::new(None),
        });
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.job.is_some() {
                bail!("已有传输正在进行，请等待或取消。");
            }
            inner.job = Some(job.clone());
        }
        self.update(json!({"busy":true,"phase":"planning","direction":options.direction,"message":"正在检查文件与目标…","bytes":0,"totalBytes":0,"completed":0,"totalFiles":0,"speed":0,"currentFile":""}));
        let mut bytes = 0u64;
        let mut completed = 0u64;
        let operation=async{
            let(_,sftp,home,connection)=self.sftp().await?;check(&job)?;let remote=Self::remote_path(&options.remote_path,&home)?;let local=self.local_path(&options.local_path)?;let upload=options.direction=="upload";
            let(source,dest,from,to)=if upload{(Access::Local,Access::Remote(sftp.clone()),local,remote)}else{(Access::Remote(sftp.clone()),Access::Local,remote,local)};
            if (dropped.is_empty()&&source.stat(&from).await?.kind!=Kind::Directory)||dest.stat(&to).await?.kind!=Kind::Directory{bail!("传输两端必须是已存在的文件夹。");}
            let mut stack=Vec::new();let mut selected=HashSet::new();for name in &options.names{leaf(name,upload&&cfg!(windows))?;if selected.insert(name.clone()){stack.push((source.join(&from,name),dest.join(&to,name),name.clone()));}}
            for(src,name)in &dropped{stack.push((src.clone(),dest.join(&to,name),name.clone()));}
            let mut planned=HashSet::new();let mut directories=Vec::new();let mut files=Vec::new();let mut conflicts=Vec::new();let mut total=0u64;
            while let Some((src,dst,name))=stack.pop(){check(&job)?;leaf(&name,!upload&&cfg!(windows))?;let key=if !upload&&cfg!(windows){dst.to_lowercase()}else{dst.clone()};if !planned.insert(key){bail!("多个源项目对应同一目标：{dst}");}let meta=source.stat(&src).await?;if matches!(meta.kind,Kind::Link|Kind::Other){bail!("暂不传输符号链接或特殊文件：{src}");}let existing=dest.optional(&dst).await?;if existing.as_ref().is_some_and(|m|m.kind!=meta.kind){bail!("目标存在不同类型的同名项目：{dst}");}
                if meta.kind==Kind::Directory{directories.push((dst.clone(),existing.is_some()));for(child,_)in source.list(&src).await?{stack.push((source.join(&src,&child),dest.join(&dst,&child),child));}}else{let size=meta.size.context("源文件缺少大小属性，无法校验传输。")?;meta.modified.context("源文件缺少修改时间，无法校验传输。")?;total=total.checked_add(size).context("传输总大小超出范围。")?;if existing.is_some(){conflicts.push(dst.clone());}files.push(Item{source:src,target:dst,meta,existing});}}
            self.update(json!({"totalBytes":total,"totalFiles":files.len()}));
            if !conflicts.is_empty(){self.update(json!({"phase":"confirming","message":format!("等待确认覆盖 {} 个文件。",conflicts.len())}));if !confirm(conflicts).await?{job.user.store(true,Ordering::SeqCst);job.cancel.cancel();}check(&job)?;
                if upload{let channel=connection.channel().await?;channel.request_subsystem(true,"sftp").await?;let raw=RawSftpSession::new(channel.into_stream());let version=raw.init().await?;if version.extensions.get("posix-rename@openssh.com").map(String::as_str)!=Some("1"){let _=raw.close_session();bail!("远端不支持安全原子覆盖，原文件保留，请选择其他目标。");}*job.rename.lock().unwrap()=Some(Arc::new(raw));}}
            for(path,exists)in &directories{check(&job)?;if !exists{dest.mkdir(path).await?;}}
            self.update(json!({"phase":"transferring","message":"正在传输…"}));let started=Instant::now();let mut last=Instant::now();let mut buffer=vec![0;65536];
            for item in &files{check(&job)?;self.update(json!({"currentFile":item.source}));let parent=if upload{posix_parent(&item.target)}else{Path::new(&item.target).parent().context("目标路径无效")?.to_string_lossy().into_owned()};let temporary=dest.join(&parent,&format!(".wcs-transfer-{}.part",uuid::Uuid::new_v4()));let mut output=dest.write(&temporary).await?;job.temps.lock().unwrap().push(OwnedTemp{access:dest.clone(),path:temporary.clone(),remote:upload,identity:connection.profile.terminal_name()});check(&job)?;let mut input=source.read(&item.source).await?;let mut copied=0u64;
                loop{let n=tokio::select!{_=job.cancel.cancelled()=>bail!("传输已中断。"),result=input.read(&mut buffer)=>result?};if n==0{break;}tokio::select!{_=job.cancel.cancelled()=>bail!("传输已中断。"),result=output.write_all(&buffer[..n])=>result?};copied+=n as u64;bytes+=n as u64;
                    if options.rate_limit>0{let due=Duration::from_secs_f64(bytes as f64/options.rate_limit as f64);if let Some(wait)=due.checked_sub(started.elapsed()){tokio::select!{_=job.cancel.cancelled()=>bail!("传输已中断。"),_=tokio::time::sleep(wait)=>{}}}}
                    if last.elapsed()>=Duration::from_millis(120){self.update(json!({"bytes":bytes,"completed":completed,"speed":bytes as f64/started.elapsed().as_secs_f64().max(0.001)}));last=Instant::now();}}
                output.flush().await?;output.shutdown().await?;drop(output);drop(input);check(&job)?;
                if copied!=item.meta.size.unwrap()||dest.stat(&temporary).await?.size!=item.meta.size||source.stat(&item.source).await?!=item.meta{bail!("传输期间源文件发生变化，请重新传输：{}",item.source);}
                let current=dest.optional(&item.target).await?;if current.is_some()&&current!=item.existing{bail!("目标在确认后发生变化，请重新传输：{}",item.target);}check(&job)?;
                if upload&&item.existing.is_some(){let raw=job.rename.lock().unwrap().clone().context("安全覆盖通道不可用。")?;let mut payload=Vec::new();for value in [&temporary,&item.target]{payload.extend_from_slice(&u32::try_from(value.len())?.to_be_bytes());payload.extend_from_slice(value.as_bytes());}match raw.extended("posix-rename@openssh.com",payload).await?{Packet::Status(s)if s.status_code==StatusCode::Ok=>{},Packet::Status(s)=>return Err(SftpError::Status(s).into()),_=>bail!("原子覆盖返回未知结果。")}}
                else{dest.rename(&temporary,&item.target).await?;}job.temps.lock().unwrap().retain(|t|t.path!=temporary);completed+=1;self.update(json!({"bytes":bytes,"completed":completed}));}
            self.update(json!({"phase":"completed","message":format!("传输完成：{completed} 个文件、{} 个文件夹。",directories.len()),"bytes":bytes,"completed":completed,"speed":0,"currentFile":""}));Ok::<_,anyhow::Error>(())
        }.await;
        if let Err(error) = operation {
            let canceled = job.user.load(Ordering::SeqCst);
            let message = if canceled {
                format!("已取消，已完成 {completed} 个文件。")
            } else if job.cancel.is_cancelled() {
                format!("设备连接中断，已完成 {completed} 个文件。")
            } else {
                file_error(&error)
            };
            self.update(json!({"phase":if canceled{"canceled"}else{"failed"},"message":message,"bytes":bytes,"completed":completed,"speed":0}));
        }
        let temps = std::mem::take(&mut *job.temps.lock().unwrap());
        for temp in temps {
            if let Err(error) = temp.access.unlink(&temp.path).await {
                if !missing(&error) {
                    if temp.remote {
                        self.inner
                            .lock()
                            .unwrap()
                            .pending
                            .push((temp.identity, temp.path.clone()));
                    }
                    let message = self.snapshot()["message"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    self.update(
                        json!({"message":format!("{message} 临时文件待清理：{}",temp.path)}),
                    );
                }
            }
        }
        if let Some(raw) = job.rename.lock().unwrap().take() {
            let _ = raw.close_session();
        }
        self.inner.lock().unwrap().job = None;
        self.update(json!({"busy":false}));
        Ok(self.snapshot())
    }
}
fn check(job: &Job) -> Result<()> {
    if job.cancel.is_cancelled() {
        bail!("传输已中断。");
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dropped_uploads_keep_each_source_parent_and_reject_ambiguous_input(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap());
        let a=root.join("one/中文.txt").to_string_lossy().into_owned();let b=root.join("two/second.txt").to_string_lossy().into_owned();
        let parse=|value|serde_json::from_value::<Transfer>(value).unwrap();
        let options=parse(json!({"direction":"upload","localItems":[a,b,a],"remotePath":"/tmp"}));
        let sources=dropped_sources(&options).unwrap();assert_eq!(sources.len(),2);assert_eq!(sources[0],(a.clone(),"中文.txt".into()));assert_eq!(sources[1].0,b);
        assert!(dropped_sources(&parse(json!({"direction":"download","localItems":[a],"remotePath":"/tmp"}))).is_err());
        assert!(dropped_sources(&parse(json!({"direction":"upload","names":["x"],"localItems":[a],"remotePath":"/tmp"}))).is_err());
        assert!(dropped_sources(&parse(json!({"direction":"upload","localItems":["relative.txt"],"remotePath":"/tmp"}))).is_err());
        assert!(dropped_sources(&parse(json!({"direction":"upload","localItems":[if cfg!(windows){"C:\\"}else{"/"}],"remotePath":"/tmp"}))).is_err());
    }
    #[test]
    fn names_and_remote_paths_preserve_supported_scope() {
        assert!(leaf("../x", false).is_err());
        assert!(leaf("CON.txt", true).is_err());
        assert!(leaf("bad\\name", true).is_err());
        assert!(leaf("中文.py", true).is_ok());
        assert_eq!(posix("/a/../b//c"), "/b/c");
        assert_eq!(
            FileService::remote_path("~/项目", "/home/demo").unwrap(),
            "/home/demo/项目"
        );
    }
    #[test]
    fn symlinks_and_special_file_modes_are_not_files() {
        let mut attrs = FileAttributes::default();
        attrs.permissions = Some(0o120777);
        assert_eq!(remote_meta(attrs.clone()).kind, Kind::Link);
        attrs.permissions = Some(0o060600);
        assert_eq!(remote_meta(attrs).kind, Kind::Other);
    }
    #[tokio::test]
    async fn failed_old_sftp_cannot_clear_a_replacement_session() {
        async fn fixture() -> Arc<SftpSession> {
            let (client, mut peer) = tokio::io::duplex(1024);
            tokio::spawn(async move {
                let mut init = [0; 9];
                peer.read_exact(&mut init).await.unwrap();
                peer.write_all(&[0, 0, 0, 5, 2, 0, 0, 0, 3]).await.unwrap();
                let mut buffer = [0; 100];
                while peer.read(&mut buffer).await.unwrap_or(0) > 0 {}
            });
            Arc::new(SftpSession::new(client).await.unwrap())
        }
        let first = fixture().await;
        let replacement = fixture().await;
        let service = FileService::new(DeviceManager::new(Arc::new(|_, _| {})), PathBuf::new());
        service.inner.lock().unwrap().cache = Some(Cache {
            generation: 0,
            session: replacement.clone(),
            home: "/".into(),
        });
        service.discard_session(&first);
        assert!(service.inner.lock().unwrap().cache.is_some());
        service.discard_session(&replacement);
        assert!(service.inner.lock().unwrap().cache.is_none());
    }
}

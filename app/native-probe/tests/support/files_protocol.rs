//! SFTP wire peer confined to an explicitly supplied, disposable directory.
use russh_sftp::protocol::*;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::UNIX_EPOCH,
};
#[derive(Default)]
pub struct Counts {
    pub active: AtomicUsize,
    pub writes: AtomicUsize,
    pub renames: AtomicUsize,
    pub removed: AtomicUsize,
}
impl Counts {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"active":self.active.load(Ordering::SeqCst),"writes":self.writes.load(Ordering::SeqCst),"renames":self.renames.load(Ordering::SeqCst),"removed":self.removed.load(Ordering::SeqCst)})
    }
}
enum Opened {
    File(fs::File),
    Directory(Option<Vec<File>>),
}
pub struct Peer {
    root: PathBuf,
    handles: HashMap<String, Opened>,
    next: u64,
    counts: Arc<Counts>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.counts.active.fetch_sub(1, Ordering::SeqCst);
    }
}
fn error(e: std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}
fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: "en".into(),
    }
}
fn attrs(m: fs::Metadata) -> FileAttributes {
    FileAttributes {
        size: Some(m.len()),
        permissions: Some(if m.is_dir() { 0o040755 } else { 0o100644 }),
        mtime: Some(
            m.modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
        ),
        ..Default::default()
    }
}
impl Peer {
    pub fn new(root: PathBuf, counts: Arc<Counts>) -> Self {
        counts.active.fetch_add(1, Ordering::SeqCst);
        Self {
            root: root.canonicalize().unwrap(),
            handles: HashMap::new(),
            next: 0,
            counts,
        }
    }
    fn path(&self, path: &str) -> Result<PathBuf, StatusCode> {
        let mut target = self.root.clone();
        for part in path.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                if target != self.root {
                    target.pop();
                }
                continue;
            }
            portcanvas_core::files::leaf(part, true).map_err(|_| StatusCode::PermissionDenied)?;
            target.push(part);
            if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(StatusCode::PermissionDenied);
            }
        }
        if !target.starts_with(&self.root) {
            return Err(StatusCode::PermissionDenied);
        }
        Ok(target)
    }
    fn handle(&mut self, id: u32, item: Opened) -> Handle {
        self.next += 1;
        let handle = self.next.to_string();
        self.handles.insert(handle.clone(), item);
        Handle { id, handle }
    }
}
impl russh_sftp::server::Handler for Peer {
    type Error = StatusCode;
    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }
    async fn init(&mut self, _: u32, _: HashMap<String, String>) -> Result<Version, StatusCode> {
        let mut v = Version::new();
        v.extensions
            .insert("posix-rename@openssh.com".into(), "1".into());
        Ok(v)
    }
    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, StatusCode> {
        let path = self.path(&path)?;
        fs::metadata(&path).map_err(error)?;
        let rel = path
            .strip_prefix(&self.root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        Ok(Name {
            id,
            files: vec![File::dummy(if rel.is_empty() {
                "/".into()
            } else {
                format!("/{rel}")
            })],
        })
    }
    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        Ok(Attrs {
            id,
            attrs: attrs(fs::symlink_metadata(self.path(&path)?).map_err(error)?),
        })
    }
    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        self.lstat(id, path).await
    }
    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, StatusCode> {
        let entries = fs::read_dir(self.path(&path)?)
            .map_err(error)?
            .map(|e| {
                let e = e.map_err(error)?;
                Ok(File::new(
                    e.file_name().to_str().unwrap(),
                    attrs(e.metadata().map_err(error)?),
                ))
            })
            .collect::<Result<Vec<_>, StatusCode>>()?;
        Ok(self.handle(id, Opened::Directory(Some(entries))))
    }
    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, StatusCode> {
        match self.handles.get_mut(&handle) {
            Some(Opened::Directory(entries)) => entries
                .take()
                .map(|files| Name { id, files })
                .ok_or(StatusCode::Eof),
            _ => Err(StatusCode::Failure),
        }
    }
    async fn open(
        &mut self,
        id: u32,
        filename: String,
        flags: OpenFlags,
        _: FileAttributes,
    ) -> Result<Handle, StatusCode> {
        let mut o = OpenOptions::new();
        o.read(flags.contains(OpenFlags::READ))
            .write(flags.contains(OpenFlags::WRITE))
            .create(flags.contains(OpenFlags::CREATE) && !flags.contains(OpenFlags::EXCLUDE))
            .create_new(flags.contains(OpenFlags::EXCLUDE))
            .truncate(flags.contains(OpenFlags::TRUNCATE));
        let f = o.open(self.path(&filename)?).map_err(error)?;
        Ok(self.handle(id, Opened::File(f)))
    }
    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, StatusCode> {
        let Some(Opened::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        f.seek(SeekFrom::Start(offset)).map_err(error)?;
        let mut data = vec![0; len.min(65536) as usize];
        let n = f.read(&mut data).map_err(error)?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(n);
        Ok(Data { id, data })
    }
    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, StatusCode> {
        let Some(Opened::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        f.seek(SeekFrom::Start(offset)).map_err(error)?;
        f.write_all(&data).map_err(error)?;
        self.counts.writes.fetch_add(1, Ordering::SeqCst);
        Ok(ok(id))
    }
    async fn close(&mut self, id: u32, handle: String) -> Result<Status, StatusCode> {
        self.handles.remove(&handle);
        Ok(ok(id))
    }
    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _: FileAttributes,
    ) -> Result<Status, StatusCode> {
        fs::create_dir(self.path(&path)?).map_err(error)?;
        Ok(ok(id))
    }
    async fn remove(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        fs::remove_file(self.path(&path)?).map_err(error)?;
        self.counts.removed.fetch_add(1, Ordering::SeqCst);
        Ok(ok(id))
    }
    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        fs::remove_dir(self.path(&path)?).map_err(error)?;
        self.counts.removed.fetch_add(1, Ordering::SeqCst);
        Ok(ok(id))
    }
    async fn rename(&mut self, id: u32, from: String, to: String) -> Result<Status, StatusCode> {
        let dest = self.path(&to)?;
        if dest.exists() {
            return Err(StatusCode::Failure);
        }
        fs::rename(self.path(&from)?, dest).map_err(error)?;
        self.counts.renames.fetch_add(1, Ordering::SeqCst);
        Ok(ok(id))
    }
    async fn extended(
        &mut self,
        id: u32,
        request: String,
        data: Vec<u8>,
    ) -> Result<Packet, StatusCode> {
        if request != "posix-rename@openssh.com" {
            return Err(StatusCode::OpUnsupported);
        }
        let mut pos = 0;
        let mut paths = Vec::new();
        for _ in 0..2 {
            let len = u32::from_be_bytes(
                data.get(pos..pos + 4)
                    .ok_or(StatusCode::BadMessage)?
                    .try_into()
                    .unwrap(),
            ) as usize;
            pos += 4;
            let path = std::str::from_utf8(data.get(pos..pos + len).ok_or(StatusCode::BadMessage)?)
                .map_err(|_| StatusCode::BadMessage)?;
            paths.push(self.path(path)?);
            pos += len;
        }
        // Tests emulate the extension's replace semantics on disposable files only.
        if paths[1].exists() {
            fs::remove_file(&paths[1]).map_err(error)?;
        }
        fs::rename(&paths[0], &paths[1]).map_err(error)?;
        self.counts.renames.fetch_add(1, Ordering::SeqCst);
        Ok(Packet::Status(ok(id)))
    }
}

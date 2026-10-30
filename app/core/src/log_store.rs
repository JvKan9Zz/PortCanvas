use crate::{log_protocol::Marker, profiles::atomic_json};
use anyhow::{bail, Context, Result};
use chrono::{Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cmp::{Ordering, Reverse},
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
pub fn project(marker: &Marker, manual: &str) -> Value {
    let path = if marker.project_root.is_empty() {
        &marker.cwd
    } else {
        &marker.project_root
    };
    let source = if manual.is_empty() {
        format!("auto:{}", if path.is_empty() { "unknown" } else { path })
    } else {
        format!("manual:{manual}")
    };
    let name = if manual.is_empty() {
        path.trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("未分类")
    } else {
        manual
    };
    let hash = Sha256::digest(source.as_bytes());
    let key = hash
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let slug = name
        .chars()
        .map(|c| {
            if c < ' ' || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .trim_end_matches(['.', ' '])
        .chars()
        .take(50)
        .collect::<String>();
    let slug = if slug.is_empty() { "未分类" } else { &slug };
    json!({"name":name,"source":source,"key":key,"folder":format!("project-{slug}-{key}")})
}
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        return true;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{Foundation::*, System::Threading::*};
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return GetLastError() != ERROR_INVALID_PARAMETER;
            }
            let mut code = 0;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok == 0 || code == 259
        }
    }
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, 0) == 0
                || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        }
    }
}
fn uuid(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok() && text.len() == 36 && !text.contains(['/', '\\'])
}
fn directories(path: &Path) -> Result<Box<dyn Iterator<Item = Result<PathBuf>>>> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Box::new(std::iter::empty()))
        }
        Err(error) => return Err(error.into()),
    };
    Ok(Box::new(entries.filter_map(|entry| match entry {
        Err(error) => Some(Err(error.into())),
        Ok(entry) => match entry.file_type() {
            Err(error) => Some(Err(error.into())),
            Ok(kind) if kind.is_dir() => Some(Ok(entry.path())),
            _ => None,
        },
    })))
}
fn read_meta(directory: &Path) -> Result<Option<Value>> {
    let file = directory.join("run.json");
    let metadata = match fs::symlink_metadata(&file) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if metadata.file_type().is_symlink() || metadata.len() > 4 * 1024 * 1024 {
        return Ok(None);
    }
    let mut data = Vec::new();
    File::open(file)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    let meta: Value = match serde_json::from_slice(&data) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let id = directory
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if meta["schema"] != "wcs.run/1"
        || meta["id"] != id
        || !meta["command"].is_string()
        || !meta["project"]["name"].is_string()
    {
        return Ok(None);
    }
    if fs::symlink_metadata(directory.join("output.log")).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Ok(None);
    }
    Ok(Some(meta))
}
struct Writer {
    meta: Value,
    directory: PathBuf,
    output: BufWriter<File>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    pub time_ms: i64,
    pub id: String,
}
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub status: String,
    pub cursor: Option<Cursor>,
}
struct Row {
    key: Cursor,
    value: Value,
}
impl PartialEq for Row {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for Row {}
impl PartialOrd for Row {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Row {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}
pub struct LogStore {
    pub root: PathBuf,
    writers: HashMap<String, Writer>,
    recent: HashMap<String, PathBuf>,
    order: VecDeque<String>,
    pub notice: String,
    max_bytes: Option<u64>,
    usage: Option<u64>,
    pub cleaned: u64,
}
impl LogStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.is_absolute() {
            bail!("日志根目录须为绝对路径。");
        }
        Ok(Self {
            root,
            writers: HashMap::new(),
            recent: HashMap::new(),
            order: VecDeque::new(),
            notice: String::new(),
            max_bytes: None,
            usage: None,
            cleaned: 0,
        })
    }
    fn record_bytes(directory: &Path) -> Result<u64> {
        let mut size = 0u64;
        for name in ["run.json", "output.log"] {
            match fs::symlink_metadata(directory.join(name)) {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => size = size.saturating_add(meta.len()),
                Ok(_) => bail!("日志包含非普通文件，已保留。"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(error.into()),
            }
        }
        Ok(size)
    }
    fn refresh_usage(&mut self) -> Result<u64> {
        self.flush()?;
        let mut used = 0u64;
        Self::scan(&self.root, |directory, _| {
            used = used.saturating_add(Self::record_bytes(&directory)?);
            Ok(true)
        })?;
        self.usage = Some(used);
        Ok(used)
    }
    pub fn set_max_bytes(&mut self, limit: Option<u64>) -> Result<()> {
        self.configure_max_bytes(limit);
        self.make_room(0, None)
    }
    pub fn configure_max_bytes(&mut self, limit: Option<u64>) {
        self.max_bytes = limit;
        self.usage = None;
    }
    fn make_room(&mut self, extra: u64, protected: Option<&str>) -> Result<()> {
        let Some(limit) = self.max_bytes else { return Ok(()); };
        anyhow::ensure!(extra <= limit, "单次日志内容超过存储上限，本地记录已暂停，终端继续运行。");
        let used = match self.usage { Some(used) => used, None => self.refresh_usage()? };
        if used.saturating_add(extra) <= limit { return Ok(()); }
        self.refresh_usage()?;
        let mut candidates = Vec::new();
        Self::scan(&self.root, |directory, meta| {
            let id = meta["id"].as_str().unwrap();
            if self.writers.contains_key(id) || protected == Some(id) || meta["status"] == "running"
                || meta["ownerPid"].as_u64().and_then(|pid| u32::try_from(pid).ok()).is_some_and(process_alive) {
                return Ok(true);
            }
            // Automatic retention only owns the two files of a valid record.
            if !fs::read_dir(&directory)?.all(|entry| entry.is_ok_and(|entry| {
                matches!(entry.file_name().to_str(), Some("run.json" | "output.log"))
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
            })) { return Ok(true); }
            if let Some(time) = meta["startedAt"].as_str().and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok()) {
                candidates.push((time.timestamp_millis(), id.to_string(), directory));
            }
            Ok(true)
        })?;
        candidates.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        for (_, id, directory) in candidates {
            if self.usage.unwrap().saturating_add(extra) <= limit { break; }
            let size = Self::record_bytes(&directory)?;
            self.delete_record(&directory)?;
            self.recent.remove(&id);
            self.usage = Some(self.usage.unwrap().saturating_sub(size));
            self.cleaned += 1;
        }
        anyhow::ensure!(self.usage.unwrap().saturating_add(extra) <= limit,
            "日志空间不足，当前没有可清理的已结束日志；请提高上限或等待记录封存，终端继续运行。");
        Ok(())
    }
    fn write_meta(&mut self, directory: &Path, meta: &Value) -> Result<()> {
        let file = directory.join("run.json");
        let before = fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0);
        atomic_json(&file, meta)?;
        if let Some(used) = self.usage.as_mut() {
            *used = used.saturating_sub(before).saturating_add(fs::metadata(file)?.len());
        }
        Ok(())
    }
    fn delete_record(&mut self, directory: &Path) -> Result<()> {
        let root = dunce::canonicalize(&self.root)?;
        let resolved = dunce::canonicalize(directory)?;
        anyhow::ensure!(resolved.starts_with(&root) && resolved != root
            && !fs::symlink_metadata(directory)?.file_type().is_symlink(), "日志路径校验失败。");
        if let Err(error) = fs::remove_dir_all(directory) { self.usage = None; return Err(error.into()); }
        Ok(())
    }
    pub fn active(&self) -> usize {
        self.writers.len()
    }
    pub fn manual_for(&self, id: &str) -> Option<String> {
        self.writers
            .get(id)
            .and_then(|w| w.meta["project"]["source"].as_str())
            .map(|source| {
                source
                    .strip_prefix("manual:")
                    .unwrap_or_default()
                    .to_string()
            })
    }
    fn cache(&mut self, id: &str, path: PathBuf) {
        if !self.recent.contains_key(id) {
            self.order.push_back(id.into());
        }
        self.recent.insert(id.into(), path);
        while self.order.len() > 1000 {
            if let Some(id) = self.order.pop_front() {
                self.recent.remove(&id);
            }
        }
    }
    fn scan(root: &Path, mut visit: impl FnMut(PathBuf, Value) -> Result<bool>) -> Result<()> {
        for group in directories(root)? {
            let group = group?;
            if !group
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .starts_with("project-")
            {
                continue;
            }
            for day in directories(&group)? {
                let day = day?;
                let name = day.file_name().unwrap_or_default().to_string_lossy();
                if name.len() != 10
                    || !name.bytes().enumerate().all(|(n, b)| {
                        if n == 4 || n == 7 {
                            b == b'-'
                        } else {
                            b.is_ascii_digit()
                        }
                    })
                {
                    continue;
                }
                for run in directories(&day)? {
                    let run = run?;
                    let id = run.file_name().unwrap_or_default().to_string_lossy();
                    if !uuid(&id) {
                        continue;
                    }
                    if let Some(meta) = read_meta(&run)? {
                        if !visit(run, meta)? {
                            return Ok(());
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn recover_status(&mut self, directory: &Path, meta: &mut Value) {
        let id = meta["id"].as_str().unwrap_or_default();
        if meta["status"] == "running" && !self.writers.contains_key(id) {
            let owner = meta["ownerPid"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok());
            if owner == Some(std::process::id()) || !owner.is_some_and(process_alive) {
                meta["status"] = json!("interrupted");
                meta["complete"] = json!(false);
                meta["ownerPid"] = Value::Null;
                meta["message"] = json!("上次记录未正常结束，输出可能不完整。");
                if let Err(error) = self.write_meta(directory, meta) {
                    self.notice = format!("中断状态无法写回：{error}");
                }
            }
        }
    }
    pub fn list(&mut self, query: Query) -> Result<Value> {
        // Discard each parsed metadata value after selection; only one page enters the UI.
        let mut heap: BinaryHeap<Reverse<Row>> = BinaryHeap::new();
        let mut projects = BTreeMap::new();
        let mut days = BTreeSet::new();
        let mut total = 0;
        let mut qualified = 0;
        let root = self.root.clone();
        self.flush()?;
        let mut usage = 0u64;
        Self::scan(&root, |directory, mut meta| {
            self.recover_status(&directory, &mut meta);
            usage = usage.saturating_add(Self::record_bytes(&directory)?);
            let id = meta["id"].as_str().unwrap().to_string();
            let key = meta["project"]["key"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let day = meta["day"].as_str().unwrap_or_default().to_string();
            projects.insert(key.clone(), meta["project"]["name"].clone());
            days.insert(day.clone());
            if !query.project.is_empty() && query.project != key
                || !query.day.is_empty() && query.day != day
                || !query.status.is_empty()
                    && query.status != meta["status"].as_str().unwrap_or_default()
            {
                return Ok(true);
            }
            total += 1;
            let started = meta["startedAt"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.timestamp_millis())
                .unwrap_or(0);
            let key = Cursor {
                time_ms: started,
                id: id.clone(),
            };
            if query.cursor.as_ref().is_some_and(|cursor| key >= *cursor) {
                return Ok(true);
            }
            qualified += 1;
            meta["command"] = json!(meta["command"]
                .as_str()
                .unwrap()
                .chars()
                .take(200)
                .collect::<String>());
            meta["directory"] = json!(directory);
            meta["bytes"] = json!(fs::metadata(directory.join("output.log"))
                .map(|m| m.len())
                .unwrap_or(0));
            meta["active"] = json!(self.writers.contains_key(&id));
            let row = Row { key, value: meta };
            if heap.len() < 200 {
                heap.push(Reverse(row));
            } else if heap.peek().is_some_and(|v| row.key > v.0.key) {
                heap.pop();
                heap.push(Reverse(row));
            }
            Ok(true)
        })?;
        self.usage = Some(usage);
        let mut rows = heap.into_iter().map(|v| v.0).collect::<Vec<_>>();
        rows.sort_by(|a, b| b.key.cmp(&a.key));
        for row in &rows {
            self.cache(
                row.value["id"].as_str().unwrap(),
                PathBuf::from(row.value["directory"].as_str().unwrap()),
            );
        }
        let cursor = rows.last().map(|r| r.key.clone());
        let records = rows.into_iter().map(|r| r.value).collect::<Vec<_>>();
        Ok(
            json!({"records":records,"total":total,"hasMore":qualified>200,"cursor":cursor,"projects":projects.into_iter().collect::<Vec<_>>(),"days":days.into_iter().rev().collect::<Vec<_>>(),"notice":self.notice,"usageBytes":usage,"maxBytes":self.max_bytes,"cleaned":self.cleaned}),
        )
    }
    fn locate(&mut self, id: &str) -> Result<(PathBuf, Value)> {
        if !uuid(id) {
            bail!("日志编号无效。");
        }
        if let Some(path) = self.recent.get(id) {
            if let Some(meta) = read_meta(path)? {
                return Ok((path.clone(), meta));
            }
        }
        let mut found = None;
        Self::scan(&self.root, |path, meta| {
            if meta["id"] == id {
                found = Some((path, meta));
                Ok(false)
            } else {
                Ok(true)
            }
        })?;
        let (path, meta) = found.context("日志不存在，请刷新列表。")?;
        self.cache(id, path.clone());
        Ok((path, meta))
    }
    pub fn find_source(&mut self, source: &str) -> Result<Option<(PathBuf, Value)>> {
        let mut found = None;
        Self::scan(&self.root, |path, meta| {
            if meta["sourceKey"] == source {
                found = Some((path, meta));
                Ok(false)
            } else {
                Ok(true)
            }
        })?;
        Ok(found)
    }
    pub fn begin(
        &mut self,
        marker: &Marker,
        device: &str,
        manual: &str,
        recovered: bool,
        kind: &str,
    ) -> Result<Option<String>> {
        let source = format!("{device}:{}:{}", marker.shell, marker.sequence);
        let old = if recovered {
            self.find_source(&source)?
        } else {
            None
        };
        if let Some((_, meta)) = &old {
            if meta["complete"] == true && meta["status"] != "running" {
                return Ok(None);
            }
            if let Some(pid) = meta["ownerPid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
            {
                if pid != std::process::id() && process_alive(pid) {
                    bail!("这份日志正在由另一软件实例记录。");
                }
            }
        }
        let date = now();
        let (directory, mut meta) = if let Some(old) = old {
            old
        } else {
            let group = project(marker, manual);
            let day = Local::now().format("%Y-%m-%d").to_string();
            let id = uuid::Uuid::new_v4().to_string();
            let path = self
                .root
                .join(group["folder"].as_str().unwrap())
                .join(&day)
                .join(&id);
            (
                path,
                json!({"schema":"wcs.run/1","id":id,"sourceKey":source,"kind":kind,"project":group,"day":day,"command":marker.command,"cwd":marker.cwd,"device":device,"startedAt":date,"gaps":[],"complete":true}),
            )
        };
        let id = meta["id"].as_str().context("日志编号缺失")?.to_string();
        if self.writers.contains_key(&id) {
            return Ok(Some(id));
        }
        for key in ["directory", "active", "bytes"] {
            meta.as_object_mut().unwrap().remove(key);
        }
        meta["status"] = json!("running");
        meta["ownerPid"] = json!(std::process::id());
        meta["endedAt"] = Value::Null;
        meta["exitCode"] = Value::Null;
        if recovered {
            meta["complete"] = json!(false);
            meta["gaps"]
                .as_array_mut()
                .context("日志缺口格式无效")?
                .push(json!({"at":date,"reason":"重新连接后恢复，中断期间输出未采集。"}));
        }
        let recovery = if recovered { "\n[记录恢复：中断期间的输出缺失]\n" } else { "" };
        let before = fs::metadata(directory.join("run.json")).map(|m| m.len()).unwrap_or(0);
        let extra = (serde_json::to_vec_pretty(&meta)?.len() as u64).saturating_sub(before) + recovery.len() as u64;
        self.make_room(extra, Some(&id))?;
        fs::create_dir_all(&directory)?;
        self.write_meta(&directory, &meta)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("output.log"))?;
        let mut writer = Writer {
            meta,
            directory: directory.clone(),
            output: BufWriter::new(file),
        };
        if recovered {
            writer
                .output
                .write_all(recovery.as_bytes())?;
            writer.output.flush()?;
            if let Some(used) = self.usage.as_mut() { *used += recovery.len() as u64; }
        }
        self.writers.insert(id.clone(), writer);
        self.cache(&id, directory);
        Ok(Some(id))
    }
    pub fn append(&mut self, id: &str, text: &str) -> Result<()> {
        if !self.writers.contains_key(id) { return Ok(()); }
        self.make_room(text.len() as u64, Some(id))?;
        if let Some(writer) = self.writers.get_mut(id) {
            if let Err(error) = writer.output.write_all(text.as_bytes()) { self.usage = None; return Err(error.into()); }
            if let Some(used) = self.usage.as_mut() { *used += text.len() as u64; }
        }
        Ok(())
    }
    pub fn flush(&mut self) -> Result<()> {
        for writer in self.writers.values_mut() {
            writer.output.flush()?;
        }
        Ok(())
    }
    pub fn finish(&mut self, id: &str, event: Option<&Marker>, reason: Option<&str>) -> Result<()> {
        let Some(mut writer) = self.writers.remove(id) else {
            return Ok(());
        };
        // Final status/gap metadata must still be saved when all data space is in use.
        self.usage = None;
        let date = now();
        writer.meta["endedAt"] = json!(date);
        writer.meta["endTimeSource"] = json!("computer_observed");
        writer.meta["ownerPid"] = Value::Null;
        writer.meta["exitCode"] = json!(event.and_then(|e| e.code));
        writer.meta["backgroundJobs"] =
            json!(event.map(|e| e.background_jobs.clone()).unwrap_or_default());
        writer.meta["status"] = json!(if writer.meta["kind"] == "session" {
            "session"
        } else if reason.is_some() {
            "interrupted"
        } else if event.and_then(|e| e.code) == Some(0) {
            "succeeded"
        } else {
            "failed"
        });
        if let Some(reason) = reason {
            writer.meta["complete"] = json!(false);
            writer.meta["gaps"]
                .as_array_mut()
                .context("日志缺口格式无效")?
                .push(json!({"at":date,"reason":reason}));
            writeln!(writer.output, "\n[记录中断：{reason}]")?;
        }
        let duration = if writer.meta["complete"] == true {
            writer.meta["startedAt"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| Utc::now().timestamp_millis() - d.timestamp_millis())
        } else {
            None
        };
        writer.meta["durationMs"] = json!(duration);
        let data_result: Result<()> = (|| {
            writer.output.flush()?;
            writer.output.get_ref().sync_data()?;
            Ok(())
        })();
        if let Err(error) = data_result {
            writer.meta["status"] = json!("write_error");
            writer.meta["complete"] = json!(false);
            writer.meta["message"] = json!(error.to_string());
            let _ = atomic_json(&writer.directory.join("run.json"), &writer.meta);
            return Err(error);
        }
        atomic_json(&writer.directory.join("run.json"), &writer.meta)?;
        Ok(())
    }
    pub fn close(&mut self, reason: &str) -> Result<()> {
        let ids = self.writers.keys().cloned().collect::<Vec<_>>();
        let mut error = None;
        for id in ids {
            if let Err(e) = self.finish(&id, None, Some(reason)) {
                error = Some(e);
            }
        }
        if let Some(error) = error {
            return Err(error);
        }
        Ok(())
    }
    pub fn recover_ended(&mut self, event: &Marker, device: &str, manual: &str) -> Result<()> {
        let source = format!("{device}:{}:{}", event.shell, event.sequence);
        if let Some((_, meta)) = self.find_source(&source)? {
            if meta["complete"] == true || meta["exitCode"].is_number() {
                return Ok(());
            }
        }
        if let Some(id) = self.begin(event, device, manual, true, "command")? {
            self.finish(&id, Some(event), None)?;
        }
        Ok(())
    }
    pub fn detail(&mut self, id: &str) -> Result<Value> {
        let (directory, mut meta) = self.locate(id)?;
        self.recover_status(&directory, &mut meta);
        let file = directory.join("output.log");
        let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        let start = size.saturating_sub(256 * 1024);
        let mut bytes = Vec::new();
        if size > 0 {
            let mut input = File::open(&file)?;
            input.seek(SeekFrom::Start(start))?;
            input.take(256 * 1024).read_to_end(&mut bytes)?;
        }
        let mut offset = 0;
        if start > 0 {
            while offset < bytes.len() && bytes[offset] & 0xc0 == 0x80 {
                offset += 1;
            }
        }
        meta["text"] = json!(String::from_utf8_lossy(&bytes[offset..]));
        meta["truncated"] = json!(start > 0);
        meta["bytes"] = json!(size);
        meta["directory"] = json!(directory);
        meta["file"] = json!(file);
        meta["active"] = json!(self.writers.contains_key(id));
        Ok(meta)
    }
    pub fn removable(&mut self, ids: &[String]) -> Result<Vec<Value>> {
        if ids.len() > 10000 || ids.iter().any(|id| !uuid(id)) {
            bail!("请选择有效日志（每次最多10000份）。");
        }
        let mut result = Vec::new();
        for id in ids {
            if let Ok((dir, mut meta)) = self.locate(id) {
                self.recover_status(&dir, &mut meta);
                if !self.writers.contains_key(id) && meta["status"] != "running" {
                    meta["directory"] = json!(dir);
                    result.push(meta);
                }
            }
        }
        Ok(result)
    }
    pub fn remove(&mut self, ids: &[String]) -> Result<Value> {
        let records = self.removable(ids)?;
        let mut removed = 0;
        for record in records {
            let directory = PathBuf::from(record["directory"].as_str().unwrap());
            self.delete_record(&directory)?;
            self.usage = None;
            self.recent.remove(record["id"].as_str().unwrap());
            removed += 1;
        }
        Ok(json!({"removed":removed}))
    }
}
#[cfg(test)]
mod retention_tests {
    use super::*;
    fn fixture() -> LogStore {
        let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("set WCS_TEST_ROOT to an isolated test directory"));
        LogStore::new(root.join(uuid::Uuid::new_v4().to_string())).unwrap()
    }
    fn marker(sequence: &str) -> Marker {
        Marker { kind: "begin".into(), shell: "1-1".into(), sequence: sequence.into(), code: Some(0),
            command: "synthetic-command".into(), cwd: "/fixture/project".into(), project_root: String::new(),
            end_cwd: String::new(), background_jobs: vec![] }
    }
    fn finished(store: &mut LogStore, sequence: &str, year: u32) -> (String, PathBuf) {
        let marker = marker(sequence);
        let id = store.begin(&marker, "fixture", "", false, "command").unwrap().unwrap();
        store.append(&id, "synthetic-output").unwrap();
        store.finish(&id, Some(&marker), None).unwrap();
        let (directory, mut meta) = store.locate(&id).unwrap();
        meta["startedAt"] = json!(format!("{year}-01-01T00:00:00.000Z"));
        atomic_json(&directory.join("run.json"), &meta).unwrap();
        store.usage = None;
        (id, directory)
    }
    #[test]
    fn oldest_finished_log_is_removed_to_admit_new_output() {
        let mut store = fixture();
        let (_, oldest) = finished(&mut store, "1", 2020);
        let (_, newer) = finished(&mut store, "2", 2021);
        let id = store.begin(&marker("3"), "fixture", "", false, "command").unwrap().unwrap();
        let ordinary = store.root.join("keep.txt");
        fs::write(&ordinary, b"ordinary-user-file").unwrap();
        let used = store.refresh_usage().unwrap();
        store.set_max_bytes(Some(used + 3)).unwrap();
        store.append(&id, "中文").unwrap();
        store.flush().unwrap();
        assert!(!oldest.exists());
        assert!(newer.is_dir());
        assert_eq!(fs::read_to_string(ordinary).unwrap(), "ordinary-user-file");
        assert_eq!(store.detail(&id).unwrap()["text"], "中文");
        assert_eq!(store.cleaned, 1);
        assert!(store.refresh_usage().unwrap() <= store.max_bytes.unwrap());
    }
    #[test]
    fn lowering_limit_prunes_by_time_and_manual_removal_releases_space() {
        let mut store = fixture();
        let (_, oldest) = finished(&mut store, "1", 2020);
        let (newer_id, newer) = finished(&mut store, "2", 2021);
        let size = LogStore::record_bytes(&newer).unwrap();
        store.set_max_bytes(Some(size)).unwrap();
        assert!(!oldest.exists());
        assert!(newer.exists());
        store.remove(&[newer_id]).unwrap();
        assert_eq!(store.refresh_usage().unwrap(), 0);
        store.set_max_bytes(None).unwrap();
        assert!(store.begin(&marker("3"), "fixture", "", false, "command").unwrap().is_some());
    }
    #[test]
    fn active_and_unmanaged_records_are_preserved_when_the_limit_cannot_fit() {
        let mut store = fixture();
        let (_, unmanaged) = finished(&mut store, "1", 2020);
        fs::write(unmanaged.join("keep.raw"), b"user-content").unwrap();
        let id = store.begin(&marker("2"), "fixture", "", false, "command").unwrap().unwrap();
        store.append(&id, "keep-active").unwrap();
        store.flush().unwrap();
        assert!(store.set_max_bytes(Some(1)).is_err());
        assert!(store.append(&id, "x").is_err());
        assert_eq!(store.detail(&id).unwrap()["text"], "keep-active");
        assert_eq!(fs::read(unmanaged.join("keep.raw")).unwrap(), b"user-content");
        store.set_max_bytes(None).unwrap();
        store.append(&id, "-continued").unwrap();
        store.flush().unwrap();
        assert_eq!(store.detail(&id).unwrap()["text"], "keep-active-continued");
    }
    #[test]
    fn recovering_a_record_cannot_prune_its_own_source() {
        let mut store = fixture();
        let marker = marker("1");
        let id = store.begin(&marker, "fixture", "", false, "command").unwrap().unwrap();
        store.append(&id, "keep-before-recovery").unwrap();
        store.finish(&id, None, Some("synthetic disconnect")).unwrap();
        let (directory, _) = store.locate(&id).unwrap();
        let output = fs::read(directory.join("output.log")).unwrap();
        let used = store.refresh_usage().unwrap();
        store.set_max_bytes(Some(used + 1)).unwrap();
        assert!(store.begin(&marker, "fixture", "", true, "command").is_err());
        assert_eq!(fs::read(directory.join("output.log")).unwrap(), output);
        assert!(directory.join("run.json").exists());
        assert_eq!(store.cleaned, 0);
    }
    #[test]
    fn another_instance_running_record_is_never_pruned() {
        let mut store = fixture();
        let (id, directory) = finished(&mut store, "1", 2020);
        let mut meta = store.detail(&id).unwrap();
        for key in ["directory", "file", "text", "bytes", "active", "truncated"] { meta.as_object_mut().unwrap().remove(key); }
        meta["status"] = json!("running");
        meta["ownerPid"] = json!(std::process::id());
        atomic_json(&directory.join("run.json"), &meta).unwrap();
        assert!(store.set_max_bytes(Some(1)).is_err());
        assert!(directory.exists());
        assert_eq!(store.cleaned, 0);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> LogStore {
        let root = std::env::var_os("WCS_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .ancestors()
                    .nth(3)
                    .unwrap()
                    .join("05-测试与验证/local/v04/rust-log-tests")
            });
        LogStore::new(root.join(uuid::Uuid::new_v4().to_string())).unwrap()
    }
    fn marker(sequence: &str) -> Marker {
        Marker {
            kind: "begin".into(),
            shell: "123-456".into(),
            sequence: sequence.into(),
            code: None,
            command: "python3 示例.py".into(),
            cwd: "/home/demo/项目/debug".into(),
            project_root: "/home/demo/项目".into(),
            end_cwd: String::new(),
            background_jobs: vec![],
        }
    }
    fn ended(sequence: &str, code: i32) -> Marker {
        Marker {
            kind: "end".into(),
            code: Some(code),
            ..marker(sequence)
        }
    }
    #[test]
    fn project_hashes_match_legacy() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/protocol.json")).unwrap();
        let marker: Marker = serde_json::from_value(fixture["parsed"][0].clone()).unwrap();
        assert_eq!(project(&marker, ""), fixture["project"]);
        assert_eq!(project(&marker, "测试项目"), fixture["manualProject"]);
    }
    #[test]
    fn reads_unchanged_v035_record() {
        let mut store = fixture();
        let directory = store.root.join(
            "project-project-demo-28b77680acdd/2026-10-23/264b9c82-d8c9-4f61-bce6-d67796b27bb8",
        );
        fs::create_dir_all(&directory).unwrap();
        let original = include_bytes!("../tests/fixtures/logs/project-project-demo-28b77680acdd/2026-10-23/264b9c82-d8c9-4f61-bce6-d67796b27bb8/run.json");
        fs::write(directory.join("run.json"), original).unwrap();
        fs::write(directory.join("output.log"), include_bytes!("../tests/fixtures/logs/project-project-demo-28b77680acdd/2026-10-23/264b9c82-d8c9-4f61-bce6-d67796b27bb8/output.log")).unwrap();
        let page = store.list(Query::default()).unwrap();
        assert_eq!(page["total"], 1);
        let record = store
            .detail("264b9c82-d8c9-4f61-bce6-d67796b27bb8")
            .unwrap();
        assert_eq!(record["text"], "示例输出\n");
        assert_eq!(record["exitCode"], 1);
        assert_eq!(record["complete"], true);
        assert_eq!(fs::read(directory.join("run.json")).unwrap(), original);
    }
    #[test]
    fn finished_utf8_and_exit_code_are_local() {
        let mut store = fixture();
        let id = store
            .begin(&marker("1"), "device", "", false, "command")
            .unwrap()
            .unwrap();
        store.append(&id, "中文错误输出\n").unwrap();
        store.finish(&id, Some(&ended("1", 1)), None).unwrap();
        let record = store.detail(&id).unwrap();
        assert_eq!(record["status"], "failed");
        assert_eq!(record["exitCode"], 1);
        assert_eq!(record["complete"], true);
        assert_eq!(record["text"], "中文错误输出\n");
        assert!(Path::new(record["file"].as_str().unwrap()).starts_with(&store.root));
    }
    #[test]
    fn manual_project_and_interruption_resume_one_record() {
        let mut store = fixture();
        let event = marker("1");
        let id = store
            .begin(&event, "device", "手动项目", false, "command")
            .unwrap()
            .unwrap();
        store.append(&id, "before\n").unwrap();
        store.finish(&id, None, Some("SSH断线")).unwrap();
        let recovered = store
            .begin(&event, "device", "不同项目", true, "command")
            .unwrap()
            .unwrap();
        assert_eq!(id, recovered);
        store.append(&id, "after\n").unwrap();
        store.finish(&id, Some(&ended("1", 0)), None).unwrap();
        let record = store.detail(&id).unwrap();
        assert_eq!(record["project"]["name"], "手动项目");
        assert_eq!(record["complete"], false);
        assert_eq!(record["exitCode"], 0);
        assert_eq!(record["durationMs"], Value::Null);
        assert!(record["text"].as_str().unwrap().contains("after"));
        store.recover_ended(&ended("1", 0), "device", "").unwrap();
        assert_eq!(store.list(Query::default()).unwrap()["total"], 1);
    }
    #[test]
    fn cleanup_keeps_active_and_unrelated_files() {
        let mut store = fixture();
        let done = store
            .begin(&marker("1"), "d", "", false, "command")
            .unwrap()
            .unwrap();
        store.finish(&done, Some(&ended("1", 0)), None).unwrap();
        let active = store
            .begin(&marker("2"), "d", "", false, "command")
            .unwrap()
            .unwrap();
        let unrelated = store.root.join("user-note.txt");
        fs::write(&unrelated, "keep").unwrap();
        assert_eq!(store.remove(&[done, active.clone()]).unwrap()["removed"], 1);
        assert!(store.detail(&active).is_ok());
        assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep");
        assert!(store.remove(&["../other".into()]).is_err());
        store.close("test ended").unwrap();
    }
    #[test]
    fn abandoned_owner_is_incomplete_and_removable() {
        let mut store = fixture();
        let id = store
            .begin(&marker("1"), "d", "", false, "command")
            .unwrap()
            .unwrap();
        store.finish(&id, Some(&ended("1", 0)), None).unwrap();
        let (directory, mut meta) = store.locate(&id).unwrap();
        meta["status"] = json!("running");
        meta["ownerPid"] = json!(2147483647);
        atomic_json(&directory.join("run.json"), &meta).unwrap();
        let page = store.list(Query::default()).unwrap();
        assert_eq!(page["records"][0]["status"], "interrupted");
        assert_eq!(page["records"][0]["complete"], false);
        assert_eq!(store.remove(&[id]).unwrap()["removed"], 1);
    }
    #[test]
    fn preview_bounds_chinese_without_altering_disk() {
        let mut store = fixture();
        let id = store
            .begin(&marker("1"), "d", "", false, "command")
            .unwrap()
            .unwrap();
        let text = "中文".repeat(80000);
        store.append(&id, &text).unwrap();
        store.finish(&id, Some(&ended("1", 0)), None).unwrap();
        let detail = store.detail(&id).unwrap();
        assert_eq!(detail["truncated"], true);
        assert!(!detail["text"].as_str().unwrap().contains('�'));
        assert_eq!(
            fs::read_to_string(detail["file"].as_str().unwrap()).unwrap(),
            text
        );
    }
    #[test]
    fn pagination_has_stable_cursor_and_bounded_record_page() {
        let mut store = fixture();
        for n in 0..205 {
            let seq = n.to_string();
            let id = store
                .begin(&marker(&seq), "d", "", false, "command")
                .unwrap()
                .unwrap();
            store.finish(&id, Some(&ended(&seq, 0)), None).unwrap();
        }
        let first = store.list(Query::default()).unwrap();
        assert_eq!(first["records"].as_array().unwrap().len(), 200);
        assert_eq!(first["total"], 205);
        assert_eq!(first["hasMore"], true);
        let cursor = serde_json::from_value(first["cursor"].clone()).unwrap();
        let next = store
            .list(Query {
                cursor: Some(cursor),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(next["records"].as_array().unwrap().len(), 5);
        assert_eq!(next["hasMore"], false);
        for item in next["records"].as_array().unwrap() {
            assert!(!first["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == item["id"]));
        }
        for n in 0..1010 {
            store.cache(&n.to_string(), store.root.join(n.to_string()));
        }
        assert!(store.recent.len() <= 1000);
    }
    #[test]
    fn write_failure_is_not_marked_successful() {
        let mut store = fixture();
        let id = store
            .begin(&marker("1"), "d", "", false, "command")
            .unwrap()
            .unwrap();
        let writer = store.writers.get_mut(&id).unwrap();
        writer.output = BufWriter::new(File::open(writer.directory.join("output.log")).unwrap());
        store.append(&id, "cannot write").unwrap();
        assert!(store.finish(&id, Some(&ended("1", 0)), None).is_err());
        let meta = store.detail(&id).unwrap();
        assert_eq!(meta["status"], "write_error");
        assert_eq!(meta["complete"], false);
    }
}

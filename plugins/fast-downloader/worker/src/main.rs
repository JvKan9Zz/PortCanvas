//! PortCanvas 128-Thread Fast Downloader Worker (`pc-worker-v1` + `--fetch` host transport).
use anyhow::{anyhow, bail, ensure, Context, Result};
use futures_util::{stream::FuturesUnordered, StreamExt};
use reqwest::{
    header::{
        ACCEPT_ENCODING, CONTENT_DISPOSITION, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE,
    },
    Client, StatusCode, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

const THREAD_OPTIONS: [usize; 4] = [8, 32, 64, 128];
const MAX_CONCURRENT_TASKS: usize = 3;

fn build_client(https_only: bool, no_proxy: bool, no_redirect: bool) -> Result<Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut builder = Client::builder()
        .user_agent("PortCanvas-FastDownloader/0.1.0")
        .https_only(https_only)
        .connect_timeout(Duration::from_secs(15))
        .pool_max_idle_per_host(128);
    if no_proxy {
        builder = builder.no_proxy();
    }
    if no_redirect {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }
    Ok(builder.build()?)
}

fn parse_content_range(text: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = text.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}

fn partition_ranges(size: u64, threads: usize) -> Vec<(u64, u64)> {
    if size == 0 {
        return Vec::new();
    }
    let min_chunk = 64 * 1024u64;
    let max_by_size = (size / min_chunk).max(1) as usize;
    let count = threads.clamp(1, 128).min(size as usize).min(max_by_size.max(threads.min(size as usize)));
    let count = if size < 1024 {
        (threads.min(size as usize)).max(1)
    } else {
        count.max(1)
    } as u64;
    (0..count)
        .map(|i| (size * i / count, size * (i + 1) / count - 1))
        .collect()
}

fn write_at_exact(file: &File, offset: u64, mut buf: &[u8]) -> Result<()> {
    let mut pos = offset;
    while !buf.is_empty() {
        #[cfg(windows)]
        let written = {
            use std::os::windows::fs::FileExt;
            file.seek_write(buf, pos)?
        };
        #[cfg(unix)]
        let written = {
            use std::os::unix::fs::FileExt;
            file.write_at(buf, pos)?
        };
        if written == 0 {
            bail!("写入磁盘返回 0 字节。");
        }
        buf = &buf[written..];
        pos += written as u64;
    }
    Ok(())
}

fn emit_stdout(out: &Arc<Mutex<std::io::Stdout>>, value: &Value) {
    if let Ok(mut guard) = out.lock() {
        let _ = serde_json::to_writer(&mut *guard, value);
        let _ = guard.write_all(b"\n");
        let _ = guard.flush();
    }
}

/// One-shot `--fetch` mode invoked by PortCanvas host when replacing built-in 32-thread downloader.
async fn run_fetch_mode(args: &[String]) -> Result<()> {
    let mut url_str = None;
    let mut output_str = None;
    let mut threads: usize = 128;
    let mut max_bytes: u64 = 256 * 1024 * 1024;
    let mut https_only = false;
    let mut no_proxy = false;
    let mut no_redirect = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--url" => {
                i += 1;
                url_str = args.get(i).cloned();
            }
            "--output" => {
                i += 1;
                output_str = args.get(i).cloned();
            }
            "--threads" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    threads = v;
                }
            }
            "--max-bytes" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    max_bytes = v;
                }
            }
            "--https-only" => https_only = true,
            "--no-proxy" => no_proxy = true,
            "--no-redirect" => no_redirect = true,
            _ => {}
        }
        i += 1;
    }

    let url = Url::parse(&url_str.context("缺少 --url 参数。")?)?;
    let output_path = PathBuf::from(output_str.context("缺少 --output 参数。")?);
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let client = build_client(https_only, no_proxy, no_redirect)?;

    fetch_to_file(&client, &url, &output_path, threads, Some(max_bytes), |delta, total, conns| {
        emit_stdout(
            &out,
            &json!({
                "type": "progress",
                "delta": delta,
                "total": total,
                "connections": conns
            }),
        );
    })
    .await
}

pub async fn fetch_to_file(
    client: &Client,
    url: &Url,
    output_path: &Path,
    threads: usize,
    max_bytes: Option<u64>,
    progress: impl Fn(usize, Option<u64>, usize) + Send + Sync,
) -> Result<()> {
    let threads = threads.clamp(1, 128);
    let probe = client
        .get(url.clone())
        .header(RANGE, "bytes=0-0")
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .await
        .context("无法连接下载服务器。")?;

    let range_info = probe
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_range);

    let supports_range = probe.status() == StatusCode::PARTIAL_CONTENT
        && matches!(range_info, Some((0, 0, size)) if size >= 2);

    if !supports_range {
        let response = if probe.status() == StatusCode::OK {
            probe
        } else {
            drop(probe);
            client
                .get(url.clone())
                .header(ACCEPT_ENCODING, "identity")
                .send()
                .await?
        };
        let response = response.error_for_status()?;
        let total = response.content_length();
        if let (Some(limit), Some(sz)) = (max_bytes, total) {
            ensure!(sz <= limit, "下载文件超过允许大小限制。");
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(output_path)?;
        let mut written = 0u64;
        let mut stream = response.bytes_stream();
        progress(0, total, 1);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            written += chunk.len() as u64;
            if let Some(limit) = max_bytes {
                ensure!(written <= limit, "下载文件超过允许大小限制。");
            }
            file.write_all(&chunk)?;
            progress(chunk.len(), total, 1);
        }
        file.flush()?;
        return Ok(());
    }

    let size = range_info.unwrap().2;
    if let Some(limit) = max_bytes {
        ensure!(size <= limit, "下载文件超过允许大小限制。");
    }
    let validator = probe
        .headers()
        .get(ETAG)
        .filter(|v| !v.as_bytes().starts_with(b"W/"))
        .or_else(|| probe.headers().get(LAST_MODIFIED))
        .cloned();
    drop(probe);

    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(output_path)?;
    file.set_len(size)?;
    let file = Arc::new(file);

    let count = threads.min(size as usize).max(1);
    let segments: Vec<(u64, u64)> = (0..count as u64)
        .map(|idx| (size * idx / count as u64, size * (idx + 1) / count as u64 - 1))
        .collect();
    let conns = segments.len();
    progress(0, Some(size), conns);

    let mut tasks = FuturesUnordered::new();
    for (start, end) in segments {
        let client = client.clone();
        let url = url.clone();
        let validator = validator.clone();
        let file = file.clone();
        let progress = &progress;
        tasks.push(async move {
            let mut req = client
                .get(url)
                .header(RANGE, format!("bytes={start}-{end}"))
                .header(ACCEPT_ENCODING, "identity");
            if let Some(val) = validator {
                req = req.header(IF_RANGE, val);
            }
            let resp = req.send().await.context("分段下载请求失败。")?;
            ensure!(
                resp.status() == StatusCode::PARTIAL_CONTENT,
                "服务器未返回请求的分段。"
            );
            let actual = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range);
            ensure!(
                actual == Some((start, end, size)),
                "分段范围与请求不一致。"
            );
            let mut offset = start;
            let expected_len = end - start + 1;
            let mut received = 0u64;
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                received += chunk.len() as u64;
                ensure!(received <= expected_len, "分段响应超过预期长度。");
                write_at_exact(&file, offset, &chunk)?;
                offset += chunk.len() as u64;
                progress(chunk.len(), Some(size), conns);
            }
            ensure!(received == expected_len, "分段下载不完整。");
            Ok::<(), anyhow::Error>(())
        });
    }

    while let Some(res) = tasks.next().await {
        res?;
    }
    Ok(())
}

// ======================= Interactive Background Worker Mode =======================

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ChunkState {
    start: u64,
    end: u64,
    done: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum TaskStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadTaskRecord {
    id: String,
    url: String,
    filename: String,
    save_dir: String,
    target_path: String,
    part_path: String,
    total_bytes: Option<u64>,
    downloaded_bytes: u64,
    threads: usize,
    active_threads: usize,
    supports_range: bool,
    validator: Option<String>,
    chunks: Vec<ChunkState>,
    status: TaskStatus,
    error: String,
    #[serde(skip)]
    speed_bps: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedState {
    save_dir: String,
    threads_index: usize,
    tasks: Vec<DownloadTaskRecord>,
}

struct WorkerRuntime {
    state_path: PathBuf,
    out: Arc<Mutex<std::io::Stdout>>,
    inner: Arc<Mutex<WorkerState>>,
    cancels: Arc<Mutex<BTreeMap<String, CancellationToken>>>,
    client: Client,
    url_draft: Arc<Mutex<String>>,
}

struct WorkerState {
    save_dir: String,
    threads_index: usize,
    tasks: Vec<DownloadTaskRecord>,
    last_speed_sample: Instant,
    last_bytes: BTreeMap<String, u64>,
}

impl WorkerRuntime {
    fn save_disk(&self) {
        let persisted = {
            let guard = self.inner.lock().unwrap();
            PersistedState {
                save_dir: guard.save_dir.clone(),
                threads_index: guard.threads_index,
                tasks: guard.tasks.clone(),
            }
        };
        if let Ok(data) = serde_json::to_vec_pretty(&persisted) {
            let tmp = self.state_path.with_extension("tmp");
            if std::fs::write(&tmp, data).is_ok() {
                let _ = std::fs::rename(&tmp, &self.state_path);
            }
        }
    }

    fn emit_ui_patch(&self) {
        let (patch, active_tasks, total_speed) = {
            let mut guard = self.inner.lock().unwrap();
            let elapsed = guard.last_speed_sample.elapsed().as_secs_f64();
            let update_speed = elapsed >= 0.45;
            if update_speed {
                guard.last_speed_sample = Instant::now();
            }
            let mut total_speed = 0.0f64;
            let mut active_count = 0usize;
            let mut rows = Vec::with_capacity(guard.tasks.len());
            let mut next_bytes = BTreeMap::new();

            let prev_bytes = guard.last_bytes.clone();
            for task in &mut guard.tasks {
                if task.status == TaskStatus::Downloading {
                    active_count += 1;
                    if update_speed {
                        let prev = prev_bytes.get(&task.id).copied().unwrap_or(task.downloaded_bytes);
                        let delta = task.downloaded_bytes.saturating_sub(prev) as f64;
                        task.speed_bps = delta / elapsed.max(0.05);
                    }
                    total_speed += task.speed_bps;
                    next_bytes.insert(task.id.clone(), task.downloaded_bytes);
                } else {
                    task.speed_bps = 0.0;
                }

                let progress = match task.total_bytes {
                    Some(total) if total > 0 => (task.downloaded_bytes as f64 / total as f64).clamp(0.0, 1.0),
                    _ => if task.status == TaskStatus::Completed { 1.0 } else { 0.0 },
                };
                let (badge, tone) = match task.status {
                    TaskStatus::Queued => ("排队中", "muted"),
                    TaskStatus::Downloading => ("下载中", "accent"),
                    TaskStatus::Paused => ("已暂停", "muted"),
                    TaskStatus::Completed => ("已完成", "ok"),
                    TaskStatus::Failed => ("失败", "danger"),
                };
                let size_part = match task.total_bytes {
                    Some(total) => format!("{} / {}", fmt_bytes(task.downloaded_bytes), fmt_bytes(total)),
                    None => fmt_bytes(task.downloaded_bytes),
                };
                let subtitle = match task.status {
                    TaskStatus::Downloading => {
                        let eta = match task.total_bytes {
                            Some(total) if task.speed_bps > 1.0 => {
                                let rem = total.saturating_sub(task.downloaded_bytes) as f64 / task.speed_bps;
                                format!(" · 剩余 {}", fmt_eta(rem))
                            }
                            _ => String::new(),
                        };
                        let mode = if task.supports_range {
                            format!("{} 线程", task.active_threads.max(1))
                        } else {
                            "单线程回退".into()
                        };
                        format!("{size_part} · {}/s · {mode}{eta}", fmt_bytes(task.speed_bps as u64))
                    }
                    TaskStatus::Completed => format!("{size_part} · 已保存至 {}", task.save_dir),
                    TaskStatus::Failed => format!("{size_part} · {}", task.error),
                    TaskStatus::Paused => format!("{size_part} · 已暂停（支持断点续传）"),
                    TaskStatus::Queued => format!("{size_part} · 等待空闲槽位"),
                };

                let mut actions = Vec::new();
                match task.status {
                    TaskStatus::Downloading | TaskStatus::Queued => {
                        actions.push(json!({"id": "pause", "label": "暂停", "labelEn": "Pause", "enabled": true}));
                    }
                    TaskStatus::Paused | TaskStatus::Failed => {
                        actions.push(json!({"id": "resume", "label": "继续", "labelEn": "Resume", "primary": true, "enabled": true}));
                    }
                    TaskStatus::Completed => {
                        actions.push(json!({"id": "open_folder", "label": "打开目录", "labelEn": "Open Folder", "primary": true, "enabled": true}));
                    }
                }
                actions.push(json!({"id": "remove", "label": "删除", "labelEn": "Delete", "danger": true, "enabled": true}));

                rows.push(json!({
                    "id": task.id,
                    "title": task.filename,
                    "subtitle": subtitle,
                    "badge": badge,
                    "badgeTone": tone,
                    "progress": progress,
                    "showProgress": task.status != TaskStatus::Completed,
                    "actions": actions
                }));
            }
            if update_speed {
                guard.last_bytes = next_bytes;
            }
            let configured_threads = THREAD_OPTIONS.get(guard.threads_index).copied().unwrap_or(128);
            let bg_active = active_count > 0;
            let bg_summary = if bg_active {
                format!("{active_count} 个任务下载中 · {}/s", fmt_bytes(total_speed as u64))
            } else {
                String::new()
            };
            let status_txt = if bg_active { "下载中" } else { "就绪" };
            let status_phase = if bg_active { 1 } else { 2 };
            let patch = json!({
                "saveDir": guard.save_dir,
                "threadsIndex": guard.threads_index,
                "status": status_txt,
                "statusPhase": status_phase,
                "totalSpeedText": format!("{}/s", fmt_bytes(total_speed as u64)),
                "activeCountText": format!("{active_count} 个任务"),
                "engineText": format!("{configured_threads} 线程 · 已接管内置下载"),
                "queueSummary": format!("共 {} 个任务 · 支持切页后台下载", guard.tasks.len()),
                "tasks": rows,
                "backgroundActive": bg_active,
                "backgroundSummary": bg_summary
            });
            (patch, active_count, total_speed)
        };
        let _ = (active_tasks, total_speed);
        emit_stdout(&self.out, &json!({"type": "state_patch", "patch": patch}));
    }

    fn schedule(self: &Arc<Self>) {
        let to_start: Vec<String> = {
            let mut guard = self.inner.lock().unwrap();
            let running = guard
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Downloading)
                .count();
            let slots = MAX_CONCURRENT_TASKS.saturating_sub(running);
            let mut ids = Vec::new();
            for task in &mut guard.tasks {
                if ids.len() >= slots {
                    break;
                }
                if task.status == TaskStatus::Queued {
                    task.status = TaskStatus::Downloading;
                    task.error.clear();
                    ids.push(task.id.clone());
                }
            }
            ids
        };
        for id in to_start {
            self.spawn_task(id);
        }
        self.emit_ui_patch();
    }

    fn spawn_task(self: &Arc<Self>, task_id: String) {
        let cancel = CancellationToken::new();
        self.cancels
            .lock()
            .unwrap()
            .insert(task_id.clone(), cancel.clone());
        let rt = self.clone();
        tokio::spawn(async move {
            let res = rt.execute_task(&task_id, cancel.clone()).await;
            rt.cancels.lock().unwrap().remove(&task_id);
            let completed_name = {
                let mut guard = rt.inner.lock().unwrap();
                if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == task_id) {
                    match res {
                        Ok(()) => {
                            task.status = TaskStatus::Completed;
                            task.error.clear();
                            if let Some(total) = task.total_bytes {
                                task.downloaded_bytes = total;
                            }
                            Some(task.filename.clone())
                        }
                        Err(_) if cancel.is_cancelled() => {
                            if task.status == TaskStatus::Downloading {
                                task.status = TaskStatus::Paused;
                            }
                            None
                        }
                        Err(e) => {
                            task.status = TaskStatus::Failed;
                            task.error = e.to_string();
                            None
                        }
                    }
                } else {
                    None
                }
            };
            rt.save_disk();
            if let Some(name) = completed_name {
                emit_stdout(
                    &rt.out,
                    &json!({"type": "notify", "message": format!("下载完成：{name}")}),
                );
            }
            rt.schedule();
        });
    }

    async fn execute_task(&self, task_id: &str, cancel: CancellationToken) -> Result<()> {
        let (url_str, save_dir, configured_threads, existing_chunks, existing_total, existing_part, existing_validator) = {
            let guard = self.inner.lock().unwrap();
            let task = guard
                .tasks
                .iter()
                .find(|t| t.id == task_id)
                .ok_or_else(|| anyhow!("任务不存在。"))?;
            (
                task.url.clone(),
                task.save_dir.clone(),
                task.threads,
                task.chunks.clone(),
                task.total_bytes,
                task.part_path.clone(),
                task.validator.clone(),
            )
        };
        let url = Url::parse(&url_str).context("下载链接无效。")?;
        ensure!(
            matches!(url.scheme(), "http" | "https"),
            "仅支持 HTTP / HTTPS 下载链接。"
        );
        std::fs::create_dir_all(&save_dir).context("无法创建保存目录。")?;

        // Check if we can resume existing Range chunks
        let can_resume = !existing_chunks.is_empty()
            && existing_total.is_some()
            && Path::new(&existing_part).is_file();

        let (total_size, supports_range, validator, chunks, part_path, target_path) = if can_resume {
            let guard = self.inner.lock().unwrap();
            let task = guard.tasks.iter().find(|t| t.id == task_id).unwrap();
            (
                existing_total,
                true,
                existing_validator,
                existing_chunks,
                PathBuf::from(&task.part_path),
                PathBuf::from(&task.target_path),
            )
        } else {
            // Probe server for Range support, size, and Content-Disposition filename
            let probe = tokio::select! {
                biased;
                _ = cancel.cancelled() => bail!("已暂停"),
                res = self.client.get(url.clone()).header(RANGE, "bytes=0-0").header(ACCEPT_ENCODING, "identity").send() => res?,
            };
            let range_info = probe
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range);
            let cd_name = probe
                .headers()
                .get(CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok())
                .and_then(extract_cd_filename);
            let final_name = sanitize_filename(
                cd_name
                    .as_deref()
                    .or_else(|| filename_from_url(&url))
                    .unwrap_or("download.bin"),
            );
            let target = unique_target_path(Path::new(&save_dir), &final_name);
            let part = PathBuf::from(format!("{}.pcpart", target.display()));
            let supports_range = probe.status() == StatusCode::PARTIAL_CONTENT
                && matches!(range_info, Some((0, 0, sz)) if sz >= 2);
            let validator = probe
                .headers()
                .get(ETAG)
                .filter(|v| !v.as_bytes().starts_with(b"W/"))
                .or_else(|| probe.headers().get(LAST_MODIFIED))
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);

            if !supports_range {
                // Single-thread fallback directly using this response if 200 OK
                let response = if probe.status() == StatusCode::OK {
                    probe
                } else {
                    drop(probe);
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => bail!("已暂停"),
                        res = self.client.get(url.clone()).header(ACCEPT_ENCODING, "identity").send() => res?,
                    }
                };
                let response = response.error_for_status()?;
                let total = response.content_length();
                {
                    let mut guard = self.inner.lock().unwrap();
                    if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == task_id) {
                        task.filename = target.file_name().unwrap().to_string_lossy().into_owned();
                        task.target_path = target.to_string_lossy().into_owned();
                        task.part_path = part.to_string_lossy().into_owned();
                        task.total_bytes = total;
                        task.supports_range = false;
                        task.active_threads = 1;
                        task.downloaded_bytes = 0;
                    }
                }
                self.save_disk();
                let mut file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&part)?;
                let mut stream = response.bytes_stream();
                while let Some(item) = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => bail!("已暂停"),
                    next = stream.next() => next,
                } {
                    let chunk = item?;
                    file.write_all(&chunk)?;
                    let mut guard = self.inner.lock().unwrap();
                    if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == task_id) {
                        task.downloaded_bytes += chunk.len() as u64;
                    }
                }
                file.flush()?;
                drop(file);
                std::fs::rename(&part, &target)?;
                return Ok(());
            }

            let size = range_info.unwrap().2;
            drop(probe);
            let ranges = partition_ranges(size, configured_threads);
            let chunks: Vec<ChunkState> = ranges
                .into_iter()
                .map(|(s, e)| ChunkState {
                    start: s,
                    end: e,
                    done: 0,
                })
                .collect();
            let file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(true)
                .open(&part)?;
            file.set_len(size)?;
            drop(file);
            {
                let mut guard = self.inner.lock().unwrap();
                if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == task_id) {
                    task.filename = target.file_name().unwrap().to_string_lossy().into_owned();
                    task.target_path = target.to_string_lossy().into_owned();
                    task.part_path = part.to_string_lossy().into_owned();
                    task.total_bytes = Some(size);
                    task.supports_range = true;
                    task.active_threads = chunks.len();
                    task.validator = validator.clone();
                    task.chunks = chunks.clone();
                    task.downloaded_bytes = 0;
                }
            }
            self.save_disk();
            (Some(size), true, validator, chunks, part, target)
        };

        let _ = supports_range;
        let size = total_size.unwrap();
        let file = Arc::new(
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&part_path)
                .context("无法打开断点续传临时文件。")?,
        );

        let chunk_progress: Arc<Vec<AtomicU64>> = Arc::new(
            chunks
                .iter()
                .map(|c| AtomicU64::new(c.done))
                .collect(),
        );

        // Periodic checkpoint sync from atomics into WorkerState + disk
        let sync_cancel = cancel.child_token();
        let sync_cancel_loop = sync_cancel.clone();
        let sync_inner = self.inner.clone();
        let sync_chunks = chunk_progress.clone();
        let sync_id = task_id.to_owned();
        let sync_state_path = self.state_path.clone();
        let sync_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                tokio::select! {
                    biased;
                    _ = sync_cancel_loop.cancelled() => break,
                    _ = interval.tick() => {
                        let persisted = {
                            let mut guard = sync_inner.lock().unwrap();
                            if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == sync_id) {
                                let mut sum = 0u64;
                                for (idx, c) in task.chunks.iter_mut().enumerate() {
                                    if let Some(atom) = sync_chunks.get(idx) {
                                        c.done = atom.load(Ordering::Relaxed);
                                        sum += c.done;
                                    }
                                }
                                task.downloaded_bytes = sum;
                            }
                            PersistedState {
                                save_dir: guard.save_dir.clone(),
                                threads_index: guard.threads_index,
                                tasks: guard.tasks.clone(),
                            }
                        };
                        if let Ok(data) = serde_json::to_vec_pretty(&persisted) {
                            let tmp = sync_state_path.with_extension("tmp");
                            if std::fs::write(&tmp, data).is_ok() {
                                let _ = std::fs::rename(&tmp, &sync_state_path);
                            }
                        }
                    }
                }
            }
        });

        let mut futs = FuturesUnordered::new();
        for (idx, chunk_meta) in chunks.iter().enumerate() {
            let total_len = chunk_meta.end - chunk_meta.start + 1;
            if chunk_meta.done >= total_len {
                continue;
            }
            let client = self.client.clone();
            let url = url.clone();
            let validator = validator.clone();
            let file = file.clone();
            let cancel = cancel.clone();
            let progress_vec = chunk_progress.clone();
            let start_base = chunk_meta.start;
            let end = chunk_meta.end;
            futs.push(async move {
                let mut attempts = 0u8;
                loop {
                    let already = progress_vec[idx].load(Ordering::Relaxed);
                    let req_start = start_base + already;
                    if req_start > end {
                        return Ok::<(), anyhow::Error>(());
                    }
                    let attempt_res = async {
                        let mut req = client
                            .get(url.clone())
                            .header(RANGE, format!("bytes={req_start}-{end}"))
                            .header(ACCEPT_ENCODING, "identity");
                        if let Some(val) = &validator {
                            req = req.header(IF_RANGE, val);
                        }
                        let resp = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => bail!("已暂停"),
                            res = req.send() => res?,
                        };
                        ensure!(
                            resp.status() == StatusCode::PARTIAL_CONTENT,
                            "服务器拒绝分段续传（状态 {}）。",
                            resp.status()
                        );
                        let mut pos = req_start;
                        let mut stream = resp.bytes_stream();
                        while let Some(piece) = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => bail!("已暂停"),
                            next = stream.next() => next,
                        } {
                            let piece = piece?;
                            let remaining = (end - pos + 1) as usize;
                            let take = piece.len().min(remaining);
                            write_at_exact(&file, pos, &piece[..take])?;
                            pos += take as u64;
                            progress_vec[idx].fetch_add(take as u64, Ordering::Relaxed);
                            if pos > end {
                                break;
                            }
                        }
                        ensure!(pos > end, "分段连接意外中断。");
                        Ok(())
                    }
                    .await;

                    match attempt_res {
                        Ok(()) => return Ok(()),
                        Err(_) if cancel.is_cancelled() => bail!("已暂停"),
                        Err(e) => {
                            attempts += 1;
                            if attempts >= 3 {
                                return Err(e);
                            }
                            tokio::time::sleep(Duration::from_millis(350 * attempts as u64)).await;
                        }
                    }
                }
            });
        }

        let mut first_err = None;
        while let Some(res) = futs.next().await {
            if let Err(e) = res {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        sync_cancel.cancel();
        let _ = sync_task.await;

        // Flush final atomic counts into state
        {
            let mut guard = self.inner.lock().unwrap();
            if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == task_id) {
                let mut sum = 0u64;
                for (idx, c) in task.chunks.iter_mut().enumerate() {
                    if let Some(atom) = chunk_progress.get(idx) {
                        c.done = atom.load(Ordering::Relaxed);
                        sum += c.done;
                    }
                }
                task.downloaded_bytes = sum;
            }
        }

        if let Some(err) = first_err {
            return Err(err);
        }
        ensure!(!cancel.is_cancelled(), "已暂停");
        drop(file);
        if target_path.exists() {
            let _ = std::fs::remove_file(&target_path);
        }
        std::fs::rename(&part_path, &target_path)?;
        let _ = size;
        Ok(())
    }
}

fn extract_cd_filename(header: &str) -> Option<String> {
    for part in header.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("filename=") {
            let cleaned = rest.trim_matches('"').trim_matches('\'').trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_owned());
            }
        }
    }
    None
}

fn filename_from_url(url: &Url) -> Option<&str> {
    url.path_segments()?
        .rev()
        .find(|s| !s.is_empty())
}

fn sanitize_filename(raw: &str) -> String {
    let without_query = raw.split(['?', '#']).next().unwrap_or(raw);
    let base = without_query
        .split(['/', '\\'])
        .last()
        .unwrap_or("download.bin")
        .trim();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '|' | '*' | '?' | '\0') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "download.bin".into()
    } else {
        cleaned
    }
}

fn unique_target_path(dir: &Path, filename: &str) -> PathBuf {
    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 1..1000u32 {
        let name = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{}-{filename}", uuid::Uuid::new_v4().simple()))
}

fn fmt_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let v = n as f64;
    if v >= GB {
        format!("{:.2} GB", v / GB)
    } else if v >= MB {
        format!("{:.1} MB", v / MB)
    } else if v >= KB {
        format!("{:.1} KB", v / KB)
    } else {
        format!("{n} B")
    }
}

fn fmt_eta(seconds: f64) -> String {
    let s = seconds.ceil() as u64;
    if s >= 3600 {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

async fn run_worker_mode() -> Result<()> {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let client = build_client(false, false, false)?;
    let mut rt: Option<Arc<WorkerRuntime>> = None;
    let shutdown_token = CancellationToken::new();

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match msg["type"].as_str().unwrap_or("") {
            "init" => {
                let data_dir = PathBuf::from(msg["dataDir"].as_str().unwrap_or("."));
                let _ = std::fs::create_dir_all(&data_dir);
                let default_dir = msg["defaultDownloadDir"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| data_dir.to_string_lossy().into_owned());
                let state_path = data_dir.join("downloader-state.json");
                let loaded: Option<PersistedState> = std::fs::read(&state_path)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok());
                let (save_dir, threads_index, mut tasks) = match loaded {
                    Some(s) => (
                        if s.save_dir.is_empty() {
                            default_dir
                        } else {
                            s.save_dir
                        },
                        s.threads_index.min(3),
                        s.tasks,
                    ),
                    None => (default_dir, 3, Vec::new()),
                };
                // Any interrupted tasks from a previous run stay paused ready to resume
                for task in &mut tasks {
                    if matches!(task.status, TaskStatus::Downloading | TaskStatus::Queued) {
                        task.status = TaskStatus::Paused;
                    }
                }
                let runtime = Arc::new(WorkerRuntime {
                    state_path,
                    out: out.clone(),
                    inner: Arc::new(Mutex::new(WorkerState {
                        save_dir,
                        threads_index,
                        tasks,
                        last_speed_sample: Instant::now(),
                        last_bytes: BTreeMap::new(),
                    })),
                    cancels: Arc::new(Mutex::new(BTreeMap::new())),
                    client: client.clone(),
                    url_draft: Arc::new(Mutex::new(String::new())),
                });
                runtime.emit_ui_patch();

                // Background UI refresh ticker (250ms while active)
                let tick_rt = runtime.clone();
                let tick_cancel = shutdown_token.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_millis(300));
                    loop {
                        tokio::select! {
                            biased;
                            _ = tick_cancel.cancelled() => break,
                            _ = interval.tick() => {
                                let has_active = tick_rt
                                    .inner
                                    .lock()
                                    .unwrap()
                                    .tasks
                                    .iter()
                                    .any(|t| t.status == TaskStatus::Downloading);
                                if has_active {
                                    tick_rt.emit_ui_patch();
                                }
                            }
                        }
                    }
                });
                rt = Some(runtime);
            }
            "set_state" => {
                let Some(runtime) = &rt else { continue };
                let key = msg["key"].as_str().unwrap_or("");
                match key {
                    "url" => {
                        *runtime.url_draft.lock().unwrap() =
                            msg["value"].as_str().unwrap_or("").to_owned();
                    }
                    "threadsIndex" => {
                        let idx = (msg["value"].as_u64().unwrap_or(3) as usize).min(3);
                        runtime.inner.lock().unwrap().threads_index = idx;
                        runtime.save_disk();
                        runtime.emit_ui_patch();
                    }
                    "saveDir" => {
                        let dir = msg["value"].as_str().unwrap_or("").to_owned();
                        if !dir.is_empty() {
                            runtime.inner.lock().unwrap().save_dir = dir;
                            runtime.save_disk();
                            runtime.emit_ui_patch();
                        }
                    }
                    _ => {}
                }
            }
            "action" => {
                let Some(runtime) = &rt else { continue };
                let action = msg["action"].as_str().unwrap_or("");
                let item_id = msg["itemId"].as_str().unwrap_or("");
                match action {
                    "set_save_dir" => {
                        if let Some(dir) = msg["payload"].as_str().filter(|s| !s.is_empty()) {
                            runtime.inner.lock().unwrap().save_dir = dir.to_owned();
                            runtime.save_disk();
                            runtime.emit_ui_patch();
                        }
                    }
                    "add_task" => {
                        let url_text = runtime.url_draft.lock().unwrap().trim().to_owned();
                        if url_text.is_empty() {
                            emit_stdout(
                                &out,
                                &json!({"type": "error", "message": "请先输入下载链接。"}),
                            );
                            continue;
                        }
                        let Ok(parsed) = Url::parse(&url_text) else {
                            emit_stdout(
                                &out,
                                &json!({"type": "error", "message": "下载链接格式无效。"}),
                            );
                            continue;
                        };
                        if !matches!(parsed.scheme(), "http" | "https") {
                            emit_stdout(
                                &out,
                                &json!({"type": "error", "message": "仅支持 HTTP / HTTPS 下载链接。"}),
                            );
                            continue;
                        }
                        let fname = sanitize_filename(
                            filename_from_url(&parsed).unwrap_or("download.bin"),
                        );
                        {
                            let mut guard = runtime.inner.lock().unwrap();
                            let threads = THREAD_OPTIONS
                                .get(guard.threads_index)
                                .copied()
                                .unwrap_or(128);
                            let save_dir = guard.save_dir.clone();
                            guard.tasks.insert(
                                0,
                                DownloadTaskRecord {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    url: url_text,
                                    filename: fname,
                                    save_dir,
                                    target_path: String::new(),
                                    part_path: String::new(),
                                    total_bytes: None,
                                    downloaded_bytes: 0,
                                    threads,
                                    active_threads: threads,
                                    supports_range: true,
                                    validator: None,
                                    chunks: Vec::new(),
                                    status: TaskStatus::Queued,
                                    error: String::new(),
                                    speed_bps: 0.0,
                                },
                            );
                        }
                        *runtime.url_draft.lock().unwrap() = String::new();
                        emit_stdout(
                            &out,
                            &json!({"type": "state_patch", "patch": {"url": ""}}),
                        );
                        emit_stdout(&out, &json!({"type": "error", "message": ""}));
                        runtime.save_disk();
                        runtime.schedule();
                    }
                    "pause" => {
                        if let Some(c) = runtime.cancels.lock().unwrap().remove(item_id) {
                            c.cancel();
                        }
                        {
                            let mut guard = runtime.inner.lock().unwrap();
                            if let Some(t) = guard.tasks.iter_mut().find(|t| t.id == item_id) {
                                if matches!(t.status, TaskStatus::Downloading | TaskStatus::Queued)
                                {
                                    t.status = TaskStatus::Paused;
                                }
                            }
                        }
                        runtime.save_disk();
                        runtime.schedule();
                    }
                    "resume" => {
                        {
                            let mut guard = runtime.inner.lock().unwrap();
                            let threads = THREAD_OPTIONS
                                .get(guard.threads_index)
                                .copied()
                                .unwrap_or(128);
                            if let Some(t) = guard.tasks.iter_mut().find(|t| t.id == item_id) {
                                if matches!(t.status, TaskStatus::Paused | TaskStatus::Failed) {
                                    t.threads = threads;
                                    t.status = TaskStatus::Queued;
                                    t.error.clear();
                                }
                            }
                        }
                        runtime.save_disk();
                        runtime.schedule();
                    }
                    "pause_all" => {
                        let cancels: Vec<CancellationToken> = runtime
                            .cancels
                            .lock()
                            .unwrap()
                            .values()
                            .cloned()
                            .collect();
                        for c in cancels {
                            c.cancel();
                        }
                        {
                            let mut guard = runtime.inner.lock().unwrap();
                            for t in &mut guard.tasks {
                                if matches!(t.status, TaskStatus::Downloading | TaskStatus::Queued)
                                {
                                    t.status = TaskStatus::Paused;
                                }
                            }
                        }
                        runtime.save_disk();
                        runtime.emit_ui_patch();
                    }
                    "resume_all" => {
                        {
                            let mut guard = runtime.inner.lock().unwrap();
                            for t in &mut guard.tasks {
                                if matches!(t.status, TaskStatus::Paused | TaskStatus::Failed) {
                                    t.status = TaskStatus::Queued;
                                    t.error.clear();
                                }
                            }
                        }
                        runtime.save_disk();
                        runtime.schedule();
                    }
                    "clear_completed" => {
                        runtime
                            .inner
                            .lock()
                            .unwrap()
                            .tasks
                            .retain(|t| t.status != TaskStatus::Completed);
                        runtime.save_disk();
                        runtime.emit_ui_patch();
                    }
                    "open_folder" => {
                        let target = {
                            let guard = runtime.inner.lock().unwrap();
                            guard.tasks.iter().find(|t| t.id == item_id).map(|t| {
                                if !t.target_path.is_empty() && Path::new(&t.target_path).exists() {
                                    t.target_path.clone()
                                } else {
                                    t.save_dir.clone()
                                }
                            })
                        };
                        if let Some(path) = target {
                            emit_stdout(&out, &json!({"type": "open_path", "path": path}));
                        }
                    }
                    "remove" => {
                        if let Some(c) = runtime.cancels.lock().unwrap().remove(item_id) {
                            c.cancel();
                        }
                        let part_to_del = {
                            let mut guard = runtime.inner.lock().unwrap();
                            let idx = guard.tasks.iter().position(|t| t.id == item_id);
                            idx.map(|i| guard.tasks.remove(i)).map(|t| t.part_path)
                        };
                        if let Some(part) = part_to_del.filter(|p| !p.is_empty()) {
                            let _ = std::fs::remove_file(part);
                        }
                        runtime.save_disk();
                        runtime.schedule();
                    }
                    _ => {}
                }
            }
            "shutdown" => {
                shutdown_token.cancel();
                if let Some(runtime) = &rt {
                    let cancels: Vec<CancellationToken> = runtime
                        .cancels
                        .lock()
                        .unwrap()
                        .values()
                        .cloned()
                        .collect();
                    for c in cancels {
                        c.cancel();
                    }
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    {
                        let mut guard = runtime.inner.lock().unwrap();
                        for t in &mut guard.tasks {
                            if matches!(t.status, TaskStatus::Downloading | TaskStatus::Queued) {
                                t.status = TaskStatus::Paused;
                            }
                        }
                    }
                    runtime.save_disk();
                }
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = if args.iter().any(|a| a == "--fetch") {
        run_fetch_mode(&args).await
    } else {
        run_worker_mode().await
    };
    if let Err(err) = result {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn spawn_test_server(support_range: bool, payload: Vec<u8>) -> (Url, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url: Url = format!("http://{}/sample.bin", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let body = Arc::new(payload);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut req = Vec::new();
                    let mut b = [0u8; 1];
                    while !req.ends_with(b"\r\n\r\n") {
                        if sock.read_exact(&mut b).await.is_err() {
                            return;
                        }
                        req.push(b[0]);
                    }
                    let text = String::from_utf8_lossy(&req).to_lowercase();
                    let range = text
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|v| v.split_once('-'))
                        .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)));
                    if support_range && range.is_some() {
                        let (s, e) = range.unwrap();
                        let head = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {s}-{e}/{}\r\nContent-Length: {}\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
                            body.len(),
                            e - s + 1
                        );
                        let _ = sock.write_all(head.as_bytes()).await;
                        let _ = sock.write_all(&body[s..=e]).await;
                    } else {
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = sock.write_all(head.as_bytes()).await;
                        let _ = sock.write_all(&body).await;
                    }
                });
            }
        });
        (url, task)
    }

    #[tokio::test]
    async fn fetch_uses_128_threads_for_range_and_falls_back_to_single_thread() {
        let payload: Vec<u8> = (0..262_147).map(|i| (i % 251) as u8).collect();
        let client = build_client(false, true, true).unwrap();

        // 128-thread range mode
        let (url, server) = spawn_test_server(true, payload.clone()).await;
        let out_file = std::env::temp_dir().join(format!("pc-dl-128-{}.bin", uuid::Uuid::new_v4()));
        let max_conns = Arc::new(AtomicU64::new(0));
        let received = Arc::new(AtomicU64::new(0));
        let mc = max_conns.clone();
        let rc = received.clone();
        let expected_len = payload.len() as u64;
        fetch_to_file(&client, &url, &out_file, 128, None, move |delta, total, conns| {
            assert_eq!(total, Some(expected_len));
            rc.fetch_add(delta as u64, Ordering::Relaxed);
            mc.fetch_max(conns as u64, Ordering::Relaxed);
        })
        .await
        .unwrap();
        server.abort();
        assert_eq!(max_conns.load(Ordering::Relaxed), 128);
        assert_eq!(received.load(Ordering::Relaxed), payload.len() as u64);
        assert_eq!(std::fs::read(&out_file).unwrap(), payload);
        let _ = std::fs::remove_file(&out_file);

        // Single-thread fallback mode
        let (url2, server2) = spawn_test_server(false, payload.clone()).await;
        let out_file2 = std::env::temp_dir().join(format!("pc-dl-1-{}.bin", uuid::Uuid::new_v4()));
        let max_conns2 = Arc::new(AtomicU64::new(0));
        let mc2 = max_conns2.clone();
        fetch_to_file(&client, &url2, &out_file2, 128, None, move |_, _, conns| {
            mc2.fetch_max(conns as u64, Ordering::Relaxed);
        })
        .await
        .unwrap();
        server2.abort();
        assert_eq!(max_conns2.load(Ordering::Relaxed), 1);
        assert_eq!(std::fs::read(&out_file2).unwrap(), payload);
        let _ = std::fs::remove_file(&out_file2);
    }

    #[test]
    fn filename_sanitization_blocks_traversal_and_invalid_chars() {
        assert_eq!(sanitize_filename("../../evil:file?.bin"), "evil_file");
        assert_eq!(sanitize_filename("archive.tar.gz"), "archive.tar.gz");
        assert_eq!(sanitize_filename(".."), "download.bin");
    }
}

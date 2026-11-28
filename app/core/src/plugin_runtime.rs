//! Generic declarative native plugin UI schema (`ui.json`) and `pc-worker-v1` local worker runtime.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::{Child, Command},
    sync::{mpsc, watch, Mutex},
};
use tokio_util::sync::CancellationToken;

pub const FRAME_MAGIC: &[u8; 4] = b"PCFR";
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_CONTROL_LINE_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSpec {
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default = "default_primary_ratio")]
    pub primary_width_ratio: f32,
    #[serde(default)]
    pub header_badge: Option<HeaderBadgeSpec>,
    #[serde(default)]
    pub initial_state: BTreeMap<String, Value>,
    #[serde(default)]
    pub presets: BTreeMap<String, BTreeMap<String, BTreeMap<String, Value>>>,
    #[serde(default)]
    pub primary: Vec<BlockSpec>,
    #[serde(default)]
    pub secondary: Vec<BlockSpec>,
}

fn default_layout() -> String {
    "split".into()
}

fn default_primary_ratio() -> f32 {
    0.62
}

impl UiSpec {
    pub fn parse(text: &str) -> Result<Self> {
        let spec: Self = serde_json::from_str(text).context("插件界面描述 ui.json 无效。")?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.layout.as_str(), "split" | "stack"),
            "ui.json layout 仅支持 split 或 stack。"
        );
        ensure!(
            self.primary_width_ratio.is_finite()
                && (0.2..=0.85).contains(&self.primary_width_ratio),
            "ui.json primaryWidthRatio 必须在 0.2..=0.85 之间。"
        );
        ensure!(
            !self.primary.is_empty() || !self.secondary.is_empty(),
            "ui.json 至少需要包含一个界面区块。"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderBadgeSpec {
    pub status_key: String,
    #[serde(default)]
    pub phase_key: Option<String>,
    #[serde(default)]
    pub default_text: String,
    #[serde(default)]
    pub default_text_en: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub key: String,
    #[serde(default)]
    pub equals: Option<Value>,
    #[serde(default)]
    pub not_equals: Option<Value>,
    #[serde(default, rename = "in")]
    pub in_values: Option<Vec<Value>>,
    #[serde(default)]
    pub truthy: Option<bool>,
}

impl Condition {
    pub fn matches(&self, state: &Map<String, Value>) -> bool {
        let current = state.get(&self.key);
        if let Some(expected) = &self.equals {
            if !values_equal(current, expected) {
                return false;
            }
        }
        if let Some(forbidden) = &self.not_equals {
            if values_equal(current, forbidden) {
                return false;
            }
        }
        if let Some(allowed) = &self.in_values {
            if !allowed.iter().any(|item| values_equal(current, item)) {
                return false;
            }
        }
        if let Some(expected_truthy) = self.truthy {
            let actual = match current {
                None | Some(Value::Null) => false,
                Some(Value::Bool(v)) => *v,
                Some(Value::Number(n)) => n.as_f64().is_some_and(|v| v != 0.0),
                Some(Value::String(s)) => !s.is_empty(),
                Some(Value::Array(a)) => !a.is_empty(),
                Some(Value::Object(o)) => !o.is_empty(),
            };
            if actual != expected_truthy {
                return false;
            }
        }
        true
    }
}

fn values_equal(current: Option<&Value>, target: &Value) -> bool {
    let Some(current) = current else {
        return target.is_null();
    };
    if current == target {
        return true;
    }
    if let (Some(a), Some(b)) = (current.as_f64(), target.as_f64()) {
        return (a - b).abs() < 1e-6;
    }
    false
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionSpec {
    pub label: String,
    #[serde(default)]
    pub label_en: String,
    pub value: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KpiItemSpec {
    pub label: String,
    #[serde(default)]
    pub label_en: String,
    pub value_key: String,
    #[serde(default)]
    pub default_value: String,
    #[serde(default)]
    pub full_width: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BlockSpec {
    Toolbar {
        #[serde(default)]
        id: String,
        #[serde(default)]
        items: Vec<ControlSpec>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    VideoStage {
        #[serde(default)]
        id: String,
        #[serde(default)]
        title: String,
        #[serde(default, rename = "titleEn")]
        title_en: String,
        #[serde(default, rename = "statusKey")]
        status_key: Option<String>,
        #[serde(default, rename = "badgeKey")]
        badge_key: Option<String>,
        #[serde(default, rename = "capabilitiesKey")]
        capabilities_key: Option<String>,
        #[serde(default, rename = "activeKey")]
        active_key: Option<String>,
        #[serde(default, rename = "loadingKey")]
        loading_key: Option<String>,
        #[serde(default, rename = "fitModeKey")]
        fit_mode_key: Option<String>,
        #[serde(default, rename = "rotationKey")]
        rotation_key: Option<String>,
        #[serde(default, rename = "flipHKey")]
        flip_h_key: Option<String>,
        #[serde(default, rename = "flipVKey")]
        flip_v_key: Option<String>,
        #[serde(default, rename = "showSnapshot")]
        show_snapshot: bool,
        #[serde(default, rename = "snapshotAction")]
        snapshot_action: Option<String>,
        #[serde(default, rename = "allowFullscreen")]
        allow_fullscreen: bool,
        #[serde(default, rename = "emptyText")]
        empty_text: String,
        #[serde(default, rename = "emptyTextEn")]
        empty_text_en: String,
        #[serde(default, rename = "footerStatusKey")]
        footer_status_key: Option<String>,
        #[serde(default)]
        controls: Vec<ControlSpec>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    ArenaMap {
        #[serde(default)]
        id: String,
        #[serde(default)]
        title: String,
        #[serde(default, rename = "titleEn")]
        title_en: String,
        #[serde(default = "default_arena_meters", rename = "widthMeters")]
        width_meters: f32,
        #[serde(default = "default_arena_meters", rename = "heightMeters")]
        height_meters: f32,
        #[serde(default, rename = "platformAssetKey")]
        platform_asset_key: Option<String>,
        #[serde(default, rename = "liveKey")]
        live_key: String,
        #[serde(default, rename = "xKey")]
        x_key: String,
        #[serde(default, rename = "yKey")]
        y_key: String,
        #[serde(default, rename = "headingKey")]
        heading_key: String,
        #[serde(default, rename = "hasHeadingKey")]
        has_heading_key: String,
        #[serde(default, rename = "coordsKey")]
        coords_key: String,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    KpiGrid {
        #[serde(default)]
        id: String,
        #[serde(default)]
        title: String,
        #[serde(default, rename = "titleEn")]
        title_en: String,
        #[serde(default, rename = "badgeKey")]
        badge_key: Option<String>,
        #[serde(default, rename = "badgePhaseKey")]
        badge_phase_key: Option<String>,
        #[serde(default = "default_kpi_columns")]
        columns: u8,
        #[serde(default)]
        items: Vec<KpiItemSpec>,
        #[serde(default, rename = "dynamicItemsKey")]
        dynamic_items_key: Option<String>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    ControlCard {
        #[serde(default)]
        id: String,
        #[serde(default)]
        title: String,
        #[serde(default, rename = "titleEn")]
        title_en: String,
        #[serde(default, rename = "badgeKey")]
        badge_key: Option<String>,
        #[serde(default)]
        controls: Vec<ControlSpec>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    TaskList {
        #[serde(default)]
        id: String,
        #[serde(default)]
        title: String,
        #[serde(default, rename = "titleEn")]
        title_en: String,
        #[serde(default, rename = "summaryKey")]
        summary_key: Option<String>,
        #[serde(default, rename = "itemsKey")]
        items_key: String,
        #[serde(default, rename = "emptyText")]
        empty_text: String,
        #[serde(default, rename = "emptyTextEn")]
        empty_text_en: String,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
}

fn default_arena_meters() -> f32 {
    3.8
}

fn default_kpi_columns() -> u8 {
    2
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ControlSpec {
    Segmented {
        #[serde(default)]
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default, rename = "labelEn")]
        label_en: String,
        #[serde(rename = "stateKey")]
        state_key: String,
        #[serde(default)]
        options: Vec<OptionSpec>,
        #[serde(default, rename = "disabledWhen")]
        disabled_when: Option<Condition>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    Select {
        #[serde(default)]
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default, rename = "labelEn")]
        label_en: String,
        #[serde(rename = "stateKey")]
        state_key: String,
        #[serde(default)]
        options: Vec<OptionSpec>,
        #[serde(default, rename = "optionsKey")]
        options_key: Option<String>,
        #[serde(default, rename = "disabledWhen")]
        disabled_when: Option<Condition>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    #[serde(rename = "checkbox", alias = "check-box")]
    CheckBox {
        #[serde(default)]
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default, rename = "labelEn")]
        label_en: String,
        #[serde(rename = "stateKey")]
        state_key: String,
        #[serde(default, rename = "disabledWhen")]
        disabled_when: Option<Condition>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    Slider {
        #[serde(default)]
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default, rename = "labelEn")]
        label_en: String,
        #[serde(rename = "stateKey")]
        state_key: String,
        min: f32,
        max: f32,
        #[serde(default = "default_step")]
        step: f32,
        #[serde(default)]
        unit: String,
        #[serde(default, rename = "disabledWhen")]
        disabled_when: Option<Condition>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    Input {
        #[serde(default)]
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default, rename = "labelEn")]
        label_en: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default, rename = "placeholderEn")]
        placeholder_en: String,
        #[serde(rename = "stateKey")]
        state_key: String,
        #[serde(default, rename = "submitAction")]
        submit_action: Option<String>,
        #[serde(default, rename = "disabledWhen")]
        disabled_when: Option<Condition>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    DirPicker {
        #[serde(default)]
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default, rename = "labelEn")]
        label_en: String,
        #[serde(rename = "stateKey")]
        state_key: String,
        #[serde(rename = "pickAction")]
        pick_action: String,
        #[serde(default, rename = "openAction")]
        open_action: Option<String>,
        #[serde(default, rename = "disabledWhen")]
        disabled_when: Option<Condition>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    Button {
        #[serde(default)]
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default, rename = "labelEn")]
        label_en: String,
        #[serde(default, rename = "activeLabel")]
        active_label: String,
        #[serde(default, rename = "activeLabelEn")]
        active_label_en: String,
        #[serde(default, rename = "activeKey")]
        active_key: Option<String>,
        action: String,
        #[serde(default)]
        payload: Option<Value>,
        #[serde(default)]
        icon: String,
        #[serde(default)]
        primary: bool,
        #[serde(default)]
        danger: bool,
        #[serde(default, rename = "busyKey")]
        busy_key: Option<String>,
        #[serde(default, rename = "disabledWhen")]
        disabled_when: Option<Condition>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    ButtonRow {
        #[serde(default)]
        id: String,
        #[serde(default)]
        buttons: Vec<ControlSpec>,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    CodeSnippet {
        #[serde(default)]
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default, rename = "labelEn")]
        label_en: String,
        text: String,
        #[serde(default, rename = "copyLabel")]
        copy_label: String,
        #[serde(default, rename = "copyLabelEn")]
        copy_label_en: String,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
    Notice {
        #[serde(default)]
        id: String,
        #[serde(default)]
        text: String,
        #[serde(default, rename = "textEn")]
        text_en: String,
        #[serde(default, rename = "textKey")]
        text_key: Option<String>,
        #[serde(default)]
        tone: String,
        #[serde(default, rename = "visibleWhen")]
        visible_when: Option<Condition>,
    },
}

fn default_step() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameFormat {
    Rgba8 = 1,
    Jpeg = 2,
}

#[derive(Clone, Debug)]
pub struct WorkerVideoFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    pub data: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub enum WorkerEvent {
    StatePatch(Map<String, Value>),
    Notify(String),
    Error(String),
    CopyText(String),
    OpenPath(String),
    Exited(Option<String>),
}

pub struct WorkerInit {
    pub plugin_id: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub data_dir: PathBuf,
    pub default_download_dir: PathBuf,
    pub language: String,
    pub frame_channel: bool,
}

pub struct LocalWorkerHandle {
    stdin: Arc<Mutex<Option<tokio::process::ChildStdin>>>,
    child: Arc<Mutex<Option<Child>>>,
    frames: watch::Receiver<Option<Arc<WorkerVideoFrame>>>,
    cancel: CancellationToken,
    #[cfg(windows)]
    _job: Option<WinJobGuard>,
}

#[cfg(windows)]
struct WinJobGuard(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for WinJobGuard {}
#[cfg(windows)]
unsafe impl Sync for WinJobGuard {}
#[cfg(windows)]
impl Drop for WinJobGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
fn attach_kill_on_close_job(child: &Child) -> Option<WinJobGuard> {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};
    let pid = child.id()?;
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            windows_sys::Win32::Foundation::CloseHandle(job);
            return None;
        }
        let proc_handle = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
        if !proc_handle.is_null() {
            let _ = AssignProcessToJobObject(job, proc_handle);
            windows_sys::Win32::Foundation::CloseHandle(proc_handle);
        }
        Some(WinJobGuard(job))
    }
}

impl LocalWorkerHandle {
    pub async fn spawn(
        init: WorkerInit,
        plugin_cancel: CancellationToken,
        events: mpsc::UnboundedSender<WorkerEvent>,
    ) -> Result<Self> {
        ensure!(
            init.executable.is_file(),
            "本地插件程序不存在：{}",
            init.executable.display()
        );
        std::fs::create_dir_all(&init.data_dir).context("无法创建插件数据目录。")?;
        let cancel = plugin_cancel.child_token();
        let (frame_tx, frame_rx) = watch::channel(None::<Arc<WorkerVideoFrame>>);

        let (frame_addr, frame_token) = if init.frame_channel {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .context("无法建立本地视频帧通道。")?;
            let addr = listener.local_addr()?.to_string();
            let token = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            let expected = token.clone();
            let frame_cancel = cancel.clone();
            tokio::spawn(async move {
                run_frame_receiver(listener, expected, frame_tx, frame_cancel).await;
            });
            (Some(addr), Some(token))
        } else {
            (None, None)
        };

        let mut command = build_worker_command(&init.executable, &init.args);
        command
            .current_dir(&init.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().with_context(|| {
            format!("无法启动本地插件进程：{}", init.executable.display())
        })?;

        #[cfg(windows)]
        let job = attach_kill_on_close_job(&child);

        let stdin = child.stdin.take().context("插件进程缺少 stdin。")?;
        let stdout = child.stdout.take().context("插件进程缺少 stdout。")?;
        let stderr = child.stderr.take().context("插件进程缺少 stderr。")?;

        let stdin_arc = Arc::new(Mutex::new(Some(stdin)));
        let child_arc = Arc::new(Mutex::new(Some(child)));

        let stderr_buf = Arc::new(Mutex::new(String::new()));
        {
            let stderr_buf = stderr_buf.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while let Ok(n) = reader.read_line(&mut line).await {
                    if n == 0 {
                        break;
                    }
                    let mut buf = stderr_buf.lock().await;
                    if buf.len() < 4096 {
                        buf.push_str(&line);
                    }
                    line.clear();
                }
            });
        }

        let handle = Self {
            stdin: stdin_arc.clone(),
            child: child_arc.clone(),
            frames: frame_rx,
            cancel: cancel.clone(),
            #[cfg(windows)]
            _job: job,
        };

        // Send initial handshake immediately
        let mut init_msg = json!({
            "type": "init",
            "pluginId": init.plugin_id,
            "dataDir": init.data_dir.to_string_lossy(),
            "defaultDownloadDir": init.default_download_dir.to_string_lossy(),
            "language": init.language,
        });
        if let (Some(addr), Some(token)) = (frame_addr, frame_token) {
            init_msg["frameAddr"] = json!(addr);
            init_msg["frameToken"] = json!(token);
        }
        handle.send_json(&init_msg).await?;

        // Control stdout reader + exit monitor
        let events_reader = events.clone();
        let child_wait = child_arc.clone();
        let cancel_reader = cancel.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::with_capacity(4096);
            loop {
                buf.clear();
                let read = tokio::select! {
                    biased;
                    _ = cancel_reader.cancelled() => break,
                    res = reader.read_until(b'\n', &mut buf) => res,
                };
                match read {
                    Ok(0) => break,
                    Ok(_) => {
                        if buf.len() > MAX_CONTROL_LINE_BYTES {
                            let _ = events_reader
                                .send(WorkerEvent::Error("插件控制消息过长。".into()));
                            continue;
                        }
                        if let Ok(value) = serde_json::from_slice::<Value>(&buf) {
                            dispatch_worker_message(value, &events_reader);
                        }
                    }
                    Err(_) => break,
                }
            }
            let exit_msg = if cancel_reader.is_cancelled() {
                None
            } else {
                let status = if let Some(mut c) = child_wait.lock().await.take() {
                    c.wait().await.ok()
                } else {
                    None
                };
                let stderr_text = stderr_buf.lock().await.trim().to_owned();
                if status.is_some_and(|s| s.success()) {
                    None
                } else if !stderr_text.is_empty() {
                    Some(stderr_text)
                } else {
                    Some("本地插件进程已退出。".into())
                }
            };
            let _ = events_reader.send(WorkerEvent::Exited(exit_msg));
        });

        // Cancel watchdog: kill child when plugin token is cancelled
        {
            let child_kill = child_arc.clone();
            let stdin_kill = stdin_arc.clone();
            let cancel_watch = cancel.clone();
            tokio::spawn(async move {
                cancel_watch.cancelled().await;
                let _ = write_json_line(&stdin_kill, &json!({"type": "shutdown"})).await;
                let _ = stdin_kill.lock().await.take();
                if let Some(mut c) = child_kill.lock().await.take() {
                    let exited = tokio::time::timeout(Duration::from_secs(2), c.wait()).await;
                    if exited.is_err() {
                        let _ = c.kill().await;
                        let _ = c.wait().await;
                    }
                }
            });
        }

        Ok(handle)
    }

    pub fn frames(&self) -> watch::Receiver<Option<Arc<WorkerVideoFrame>>> {
        self.frames.clone()
    }

    pub async fn send_json(&self, value: &Value) -> Result<()> {
        write_json_line(&self.stdin, value).await
    }

    pub async fn set_state(&self, key: &str, value: Value) -> Result<()> {
        self.send_json(&json!({"type": "set_state", "key": key, "value": value}))
            .await
    }

    pub async fn send_action(
        &self,
        action: &str,
        item_id: Option<&str>,
        payload: Option<Value>,
    ) -> Result<()> {
        self.send_json(&json!({
            "type": "action",
            "action": action,
            "itemId": item_id,
            "payload": payload,
        }))
        .await
    }

    pub async fn set_visibility(&self, visible: bool) -> Result<()> {
        self.send_json(&json!({"type": "visibility", "visible": visible}))
            .await
    }

    pub async fn shutdown(&self) {
        let _ = self.send_json(&json!({"type": "shutdown"})).await;
        let _ = self.stdin.lock().await.take();
        self.cancel.cancel();
        if let Some(mut child) = self.child.lock().await.take() {
            let exited = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            if exited.is_err() {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
    }
}

fn build_worker_command(executable: &Path, args: &[String]) -> Command {
    let is_python = executable
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("py"));
    if is_python {
        let (program, prefix_args) = resolve_python_launcher();
        let mut cmd = Command::new(program);
        for arg in prefix_args {
            cmd.arg(arg);
        }
        cmd.arg("-u")
            .arg(executable)
            .args(args)
            .env("PYTHONIOENCODING", "utf-8");
        return cmd;
    }
    let mut cmd = Command::new(executable);
    cmd.args(args);
    cmd
}

fn resolve_python_launcher() -> (PathBuf, &'static [&'static str]) {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    #[cfg(windows)]
    let candidates: &[(&str, &[&str])] = &[
        ("python.exe", &[]),
        ("python3.exe", &[]),
        ("py.exe", &["-3"]),
    ];
    #[cfg(not(windows))]
    let candidates: &[(&str, &[&str])] = &[("python3", &[]), ("python", &[])];
    for (name, prefix) in candidates {
        for dir in std::env::split_paths(&path_env) {
            let full = dir.join(name);
            if full.is_file() {
                return (full, prefix);
            }
        }
    }
    #[cfg(windows)]
    {
        (PathBuf::from("python"), &[])
    }
    #[cfg(not(windows))]
    {
        (PathBuf::from("python3"), &[])
    }
}

async fn write_json_line(
    stdin: &Arc<Mutex<Option<tokio::process::ChildStdin>>>,
    value: &Value,
) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    let mut guard = stdin.lock().await;
    let Some(writer) = guard.as_mut() else {
        bail!("插件进程输入通道已关闭。");
    };
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

fn dispatch_worker_message(value: Value, events: &mpsc::UnboundedSender<WorkerEvent>) {
    let kind = value["type"].as_str().unwrap_or("");
    match kind {
        "state_patch" => {
            if let Some(patch) = value["patch"].as_object() {
                let _ = events.send(WorkerEvent::StatePatch(patch.clone()));
            }
        }
        "notify" => {
            if let Some(msg) = value["message"].as_str() {
                let _ = events.send(WorkerEvent::Notify(msg.to_owned()));
            }
        }
        "error" => {
            if let Some(msg) = value["message"].as_str() {
                let _ = events.send(WorkerEvent::Error(msg.to_owned()));
            }
        }
        "copy_text" => {
            if let Some(text) = value["text"].as_str() {
                let _ = events.send(WorkerEvent::CopyText(text.to_owned()));
            }
        }
        "open_path" => {
            if let Some(path) = value["path"].as_str() {
                let _ = events.send(WorkerEvent::OpenPath(path.to_owned()));
            }
        }
        _ => {}
    }
}

async fn run_frame_receiver(
    listener: TcpListener,
    expected_token: String,
    frame_tx: watch::Sender<Option<Arc<WorkerVideoFrame>>>,
    cancel: CancellationToken,
) {
    let seq = AtomicU64::new(0);
    loop {
        let accepted = tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            res = listener.accept() => res,
        };
        let Ok((mut stream, _)) = accepted else {
            return;
        };
        let _ = stream.set_nodelay(true);
        let mut token_buf = vec![0u8; expected_token.len()];
        let auth_ok = tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            res = tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut token_buf)) => {
                matches!(res, Ok(Ok(_))) && token_buf == expected_token.as_bytes()
            }
        };
        if !auth_ok {
            continue;
        }
        // Authenticated worker connected; pump frames with latest-frame-wins semantics.
        let mut header = [0u8; 20];
        loop {
            let read = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                res = stream.read_exact(&mut header) => res,
            };
            if read.is_err() {
                break;
            }
            if &header[0..4] != FRAME_MAGIC {
                break;
            }
            let width = u32::from_le_bytes(header[4..8].try_into().unwrap());
            let height = u32::from_le_bytes(header[8..12].try_into().unwrap());
            let format = match header[12] {
                1 => FrameFormat::Rgba8,
                2 => FrameFormat::Jpeg,
                _ => break,
            };
            let len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
            if len == 0 || len > MAX_FRAME_BYTES || width == 0 || height == 0 {
                break;
            }
            if format == FrameFormat::Rgba8 {
                let expected_bytes = (width as usize)
                    .checked_mul(height as usize)
                    .and_then(|v| v.checked_mul(4));
                if expected_bytes != Some(len) {
                    break;
                }
            }
            let mut payload = vec![0u8; len];
            let read_payload = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                res = stream.read_exact(&mut payload) => res,
            };
            if read_payload.is_err() {
                break;
            }
            let sequence = seq.fetch_add(1, Ordering::Relaxed) + 1;
            let frame = Arc::new(WorkerVideoFrame {
                sequence,
                width,
                height,
                format,
                data: Arc::new(payload),
            });
            let _ = frame_tx.send(Some(frame));
        }
    }
}

pub fn validate_open_path(path: &str) -> Result<PathBuf> {
    let trimmed = path.trim();
    ensure!(!trimmed.is_empty(), "路径为空。");
    let p = Path::new(trimmed);
    ensure!(p.is_absolute(), "仅允许打开绝对路径。");
    ensure!(p.exists(), "目标文件或目录不存在。");
    Ok(p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_matching_supports_equals_in_and_truthy() {
        let mut state = Map::new();
        state.insert("algo".into(), json!(3));
        state.insert("active".into(), json!(true));
        state.insert("mode".into(), json!("direct"));

        assert!(Condition {
            key: "algo".into(),
            equals: Some(json!(3)),
            not_equals: None,
            in_values: None,
            truthy: None,
        }
        .matches(&state));

        assert!(Condition {
            key: "algo".into(),
            equals: None,
            not_equals: Some(json!(0)),
            in_values: Some(vec![json!(1), json!(3)]),
            truthy: None,
        }
        .matches(&state));

        assert!(Condition {
            key: "active".into(),
            equals: None,
            not_equals: None,
            in_values: None,
            truthy: Some(true),
        }
        .matches(&state));

        assert!(!Condition {
            key: "active".into(),
            equals: None,
            not_equals: None,
            in_values: None,
            truthy: Some(false),
        }
        .matches(&state));
    }

    #[test]
    fn ui_spec_validates_layout_and_blocks() {
        let raw = r#"{
            "layout": "stack",
            "primary": [
                {
                    "type": "control-card",
                    "title": "新建任务",
                    "controls": [
                        {"type": "input", "label": "下载地址", "stateKey": "url", "submitAction": "add_task"}
                    ]
                },
                {
                    "type": "task-list",
                    "title": "下载队列",
                    "itemsKey": "tasks",
                    "emptyText": "暂无任务"
                }
            ]
        }"#;
        let spec = UiSpec::parse(raw).unwrap();
        assert_eq!(spec.layout, "stack");
        assert_eq!(spec.primary.len(), 2);
    }

    #[tokio::test]
    async fn binary_frame_channel_authenticates_and_keeps_latest_frame() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let token = "0123456789abcdef0123456789abcdef".to_owned();
        let (tx, mut rx) = watch::channel(None);
        let cancel = CancellationToken::new();
        let recv_task = tokio::spawn(run_frame_receiver(
            listener,
            token.clone(),
            tx,
            cancel.clone(),
        ));

        // Wrong token should be ignored
        {
            let mut bad = tokio::net::TcpStream::connect(addr).await?;
            bad.write_all(b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx").await?;
            bad.flush().await?;
        }

        // Valid token + 2x2 RGBA8 frame
        let mut client = tokio::net::TcpStream::connect(addr).await?;
        client.write_all(token.as_bytes()).await?;
        let pixels = vec![255u8; 2 * 2 * 4];
        let mut packet = Vec::new();
        packet.extend_from_slice(FRAME_MAGIC);
        packet.extend_from_slice(&2u32.to_le_bytes());
        packet.extend_from_slice(&2u32.to_le_bytes());
        packet.push(1u8); // RGBA8
        packet.extend_from_slice(&[0u8; 3]);
        packet.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
        packet.extend_from_slice(&pixels);
        client.write_all(&packet).await?;
        client.flush().await?;

        tokio::time::timeout(Duration::from_secs(2), rx.changed()).await??;
        let frame = rx.borrow().clone().unwrap();
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 2);
        assert_eq!(frame.format, FrameFormat::Rgba8);
        assert_eq!(frame.data.len(), 16);

        cancel.cancel();
        let _ = recv_task.await;
        Ok(())
    }
}

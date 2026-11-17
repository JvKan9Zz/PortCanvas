//! Signed macOS bundle updates with a private helper process and atomic bundle exchange.
//!
//! This module never executes archive contents during preparation, touches user data, requests
//! elevation, or accepts an unsigned archive. macOS distribution signing is checked separately
//! from the existing version-bound Minisign update signature.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;

const BUNDLE_ID: &str = "com.portcanvas.desktop";
const EXECUTABLE: &str = "Contents/MacOS/PortCanvasNative";
const PREFIX: &str = ".PortCanvas-update-";
#[cfg(not(debug_assertions))]
const KEY: &str = include_str!("../assets/update-public-key.txt");
const MAX_ARCHIVE_BYTES: usize = 1024 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn canceled(cancel: &CancellationToken) -> Result<()> {
    ensure!(!cancel.is_cancelled(), "更新已取消。");
    Ok(())
}

/// Reject links in every existing component, including ancestor directories. A symlink supplied
/// as an application location must not turn an update into a write at another location.
fn safe_path(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "更新路径必须是绝对路径。");
    let mut at = PathBuf::new();
    for component in path.components() {
        ensure!(
            !matches!(component, Component::ParentDir | Component::CurDir),
            "更新路径包含不安全的分量。"
        );
        at.push(component);
        let meta = fs::symlink_metadata(&at)
            .with_context(|| format!("无法检查更新路径 {}", at.display()))?;
        ensure!(
            !meta.file_type().is_symlink(),
            "更新路径不能包含符号链接：{}",
            at.display()
        );
    }
    Ok(())
}

fn safe_tree(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(!meta.file_type().is_symlink(), "应用包包含符号链接。");
    ensure!(meta.is_file() || meta.is_dir(), "应用包包含特殊文件。");
    ensure!(meta.mode() & 0o6000 == 0, "应用包包含特权文件。");
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            safe_tree(&entry?.path())?;
        }
    }
    Ok(())
}

fn removable_tree(path: &Path) -> Result<()> {
    use std::os::macos::fs::MetadataExt as MacMetadataExt;
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.st_flags()
            & (libc::UF_IMMUTABLE | libc::SF_IMMUTABLE | libc::UF_APPEND | libc::SF_APPEND)
            == 0,
        "应用包含锁定的文件；请手动安装更新。"
    );
    if meta.is_dir() {
        ensure!(
            meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o300 == 0o300,
            "当前账户无法清理原应用；请手动安装更新。"
        );
        for entry in fs::read_dir(path)? {
            removable_tree(&entry?.path())?;
        }
    }
    Ok(())
}

fn private_directory(parent: &Path) -> Result<PathBuf> {
    safe_path(parent)?;
    ensure!(
        fs::metadata(parent)?.mode() & 0o002 == 0,
        "应用所在目录不能允许所有用户写入。"
    );
    for attempt in 0..32 {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "{PREFIX}{}-{nonce:x}-{attempt}",
            std::process::id()
        ));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e)
                    .context("无法在应用旁暂存更新；请将应用放到您可写的 Applications 目录。")
            }
        }
    }
    bail!("无法创建独立的更新暂存目录。")
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Intentionally do not use unpack(): only one fixed bundle root, ordinary directories and
/// ordinary files are admitted. No symlink/hardlink/pax extractor side effects can write outside
/// the staging tree, and no archive member can overwrite an earlier member. Raw iteration also
// rejects GNU/PAX extension headers before the tar reader can allocate their expanded bodies.
fn extract_archive(bytes: &[u8], root: &Path, cancel: &CancellationToken) -> Result<()> {
    canceled(cancel)?;
    ensure!(bytes.len() <= MAX_ARCHIVE_BYTES, "更新包超过大小限制。");
    safe_path(root)?;
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let mut expanded = 0u64;
    for (index, entry) in archive.entries()?.raw(true).enumerate() {
        canceled(cancel)?;
        ensure!(index < 50_000, "更新包文件过多。");
        let mut entry = entry?;
        let raw = entry.path_bytes();
        let raw = raw.strip_suffix(b"/").unwrap_or(&raw);
        let pieces: Vec<&[u8]> = raw.split(|b| *b == b'/').collect();
        ensure!(
            pieces.first() == Some(&b"PortCanvas.app".as_slice()),
            "更新包必须仅包含 PortCanvas.app。"
        );
        ensure!(
            pieces.iter().all(|p| !p.is_empty()
                && *p != b"."
                && *p != b".."
                && !p.contains(&b'\\')
                && !p.contains(&0)),
            "更新包包含不安全的路径。"
        );
        ensure!(raw.len() <= 4096 && pieces.len() <= 128, "更新包路径过长。");
        let path = root.join(Path::new(std::ffi::OsStr::from_bytes(raw)));
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_dir() || kind.is_file(),
            "更新包不允许链接或特殊文件。"
        );
        let size = entry.header().size()?;
        expanded = expanded.checked_add(size).context("更新包大小溢出。")?;
        ensure!(
            expanded <= MAX_EXPANDED_BYTES && size <= MAX_ARCHIVE_BYTES as u64,
            "更新包展开后超过大小限制。"
        );
        let parent = path.parent().context("更新包路径无效。")?;
        safe_create_directories(root, parent)?;
        safe_path(parent)?;
        if kind.is_dir() {
            ensure!(size == 0, "更新包目录包含无效数据。");
            if path.exists() {
                safe_path(&path)?;
                ensure!(path.is_dir(), "更新包目录与文件冲突。");
            } else {
                fs::create_dir(&path)?;
            }
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        } else {
            let mode = entry.header().mode()?;
            ensure!(mode & 0o6000 == 0, "更新包包含特权文件。");
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode & 0o755)
                .open(&path)?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                canceled(cancel)?;
                let n = entry.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buffer[..n])?;
            }
            out.sync_all()?;
        }
    }
    // Force gzip trailer/CRC validation; tar stops at its zero blocks before the gzip EOF.
    let mut decoder = archive.into_inner();
    let mut trailing = [0u8; 64 * 1024];
    let mut padding = 0usize;
    loop {
        canceled(cancel)?;
        let n = decoder.read(&mut trailing)?;
        if n == 0 {
            break;
        }
        padding += n;
        ensure!(
            padding <= 1024 * 1024 && trailing[..n].iter().all(|v| *v == 0),
            "更新包包含额外数据。"
        );
    }
    ensure!(root.join("PortCanvas.app").is_dir(), "更新包缺少应用。");
    Ok(())
}

fn safe_create_directories(root: &Path, path: &Path) -> Result<()> {
    let mut at = root.to_owned();
    for part in path.strip_prefix(root)?.components() {
        ensure!(matches!(part, Component::Normal(_)), "不安全的更新目录。");
        at.push(part);
        match fs::symlink_metadata(&at) {
            Ok(meta) => ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "更新目录包含链接或文件。"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&at)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn plist(app: &Path, key: &str) -> Result<String> {
    let output = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg(format!("Print :{key}"))
        .arg(app.join("Contents/Info.plist"))
        .output()?;
    ensure!(output.status.success(), "应用包缺少 {key}。");
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn validate_bundle(app: &Path, version: &str, team: Option<&str>) -> Result<String> {
    safe_path(app)?;
    safe_tree(app)?;
    ensure!(
        plist(app, "CFBundleIdentifier")? == BUNDLE_ID,
        "更新应用标识不匹配。"
    );
    ensure!(
        plist(app, "CFBundleExecutable")? == "PortCanvasNative",
        "更新可执行文件不匹配。"
    );
    ensure!(
        plist(app, "CFBundlePackageType")? == "APPL",
        "更新不是 macOS 应用。"
    );
    ensure!(
        semver::Version::parse(&plist(app, "CFBundleShortVersionString")?)?
            == semver::Version::parse(version)?,
        "更新应用版本与已签名版本不匹配。"
    );
    let executable = app.join(EXECUTABLE);
    let meta = fs::metadata(&executable)?;
    ensure!(
        meta.is_file() && meta.mode() & 0o111 != 0,
        "更新缺少原生可执行文件。"
    );
    let output = Command::new("/usr/bin/lipo")
        .arg("-archs")
        .arg(&executable)
        .output()?;
    ensure!(output.status.success(), "更新不是有效的 Mach-O 应用。");
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    ensure!(
        String::from_utf8(output.stdout)?
            .split_whitespace()
            .any(|v| v == arch),
        "更新不支持当前 Mac 架构。"
    );
    let output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "--all-architectures"])
        .arg(app)
        .output()?;
    ensure!(
        output.status.success(),
        "macOS 应用签名校验失败：{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let output = Command::new("/usr/bin/codesign")
        .args(["--display", "--verbose=4"])
        .arg(app)
        .output()?;
    ensure!(output.status.success(), "无法检查 macOS 应用签名。");
    let details = String::from_utf8(output.stderr)?;
    ensure!(
        details
            .lines()
            .any(|line| line.strip_prefix("Identifier=") == Some(BUNDLE_ID)),
        "macOS 代码签名标识与应用标识不匹配。"
    );
    let identity = if details.lines().any(|l| l == "Signature=adhoc") {
        "adhoc".to_owned()
    } else {
        details
            .lines()
            .find_map(|l| l.strip_prefix("TeamIdentifier="))
            .filter(|v| *v != "not set")
            .context("应用缺少发行团队标识。")?
            .to_owned()
    };
    if let Some(team) = team {
        ensure!(identity == team, "更新发行签名与已安装应用不匹配。");
    }
    Ok(identity)
}

#[derive(Serialize, Deserialize)]
struct Job {
    app: PathBuf,
    current: String,
    version: String,
    signature: String,
    public_key: String,
    parent_pid: u32,
}

/// Atomic exchange keeps the public application path present throughout replacement and rollback.
fn exchange(first: &Path, second: &Path) -> Result<()> {
    safe_path(first)?;
    safe_path(second)?;
    let a = CString::new(first.as_os_str().as_bytes())?;
    let b = CString::new(second.as_os_str().as_bytes())?;
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_SWAP,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("无法原子替换应用；原应用仍保留。");
    }
    // A directory flush failure must not be reported as a failed exchange after the swap
    // already happened, since the caller would then skip rollback. The rename result is final.
    if let Some(parent) = first.parent() {
        let _ = File::open(parent).and_then(|f| f.sync_all());
    }
    Ok(())
}

fn remove_stage(stage: &Path) -> Result<()> {
    safe_path(stage)?;
    ensure!(
        stage
            .file_name()
            .is_some_and(|n| n.as_bytes().starts_with(PREFIX.as_bytes())),
        "拒绝清理未知更新目录。"
    );
    ensure!(
        fs::metadata(stage)?.uid() == unsafe { libc::geteuid() },
        "更新目录不属于当前账户。"
    );
    // remove_dir_all never follows child symlinks. Only our private stage is ever removed.
    fs::remove_dir_all(stage)?;
    Ok(())
}

fn replace_and_launch(stage: &Path, job: &Job, timeout: Duration) -> Result<Child> {
    let candidate = stage.join("payload/PortCanvas.app");
    let team = validate_bundle(&job.app, &job.current, None)?;
    removable_tree(&job.app)?;
    validate_bundle(&candidate, &job.version, Some(&team))?;
    ensure!(
        fs::metadata(&candidate)?.dev() == fs::metadata(&job.app)?.dev(),
        "更新暂存必须与应用位于同一文件系统。"
    );
    exchange(&candidate, &job.app)?;
    let result = (|| -> Result<Child> {
        // Validate after the atomic exchange too, before executing any replacement code.
        validate_bundle(&job.app, &job.version, Some(&team))?;
        let ready = stage.join("launched");
        let mut child = Command::new(job.app.join(EXECUTABLE))
            .arg("--portcanvas-update-ack")
            .arg(&ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("无法启动更新后的应用。")?;
        let started = Instant::now();
        let wait = (|| -> Result<()> {
            loop {
                ensure!(child.try_wait()?.is_none(), "更新后的应用在就绪前退出。");
                if let Ok(meta) = fs::symlink_metadata(&ready) {
                    ensure!(
                        meta.is_file()
                            && !meta.file_type().is_symlink()
                            && meta.uid() == unsafe { libc::geteuid() },
                        "更新启动确认不安全。"
                    );
                    ensure!(fs::read(&ready)? == b"ready", "更新启动确认无效。");
                    return Ok(());
                }
                ensure!(
                    started.elapsed() < timeout,
                    "更新后的应用未能正常显示；已恢复旧版本。"
                );
                std::thread::sleep(Duration::from_millis(25));
            }
        })();
        if let Err(error) = wait {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(child)
    })();
    match result {
        Ok(child) => {
            // The old application and all downloaded/staged code are deleted only after a real
            // rendered-frame acknowledgement from the replacement application.
            remove_stage(stage).context("更新已启动，但旧代码清理失败。")?;
            Ok(child)
        }
        Err(error) => {
            exchange(&candidate, &job.app)
                .context("更新失败，自动恢复未完成；旧应用保留在更新暂存目录。")?;
            remove_stage(stage)?;
            Err(error)
        }
    }
}

fn installed_bundle(executable: &Path) -> Result<PathBuf> {
    safe_path(executable)?;
    let app = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .context("无法确定应用包路径。")?;
    ensure!(
        app.extension().is_some_and(|v| v == "app") && app.join(EXECUTABLE) == executable,
        "此开发构建请先安装完整的 PortCanvas.app。"
    );
    Ok(app.to_owned())
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
    safe_path(path)?;
    let meta = fs::metadata(path)?;
    ensure!(
        meta.is_file() && meta.len() <= limit,
        "更新文件无效或超过大小限制。"
    );
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "更新文件超过大小限制。");
    Ok(bytes)
}

fn job_from_file(path: &Path) -> Result<(PathBuf, Job)> {
    ensure!(
        path.file_name().is_some_and(|v| v == "job.json"),
        "更新助手参数无效。"
    );
    let stage = path.parent().context("更新助手缺少暂存目录。")?.to_owned();
    safe_path(&stage)?;
    let meta = fs::metadata(&stage)?;
    ensure!(
        meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
        "更新暂存目录权限不安全。"
    );
    ensure!(
        stage
            .file_name()
            .is_some_and(|v| v.as_bytes().starts_with(PREFIX.as_bytes())),
        "更新暂存目录无效。"
    );
    let job: Job = serde_json::from_slice(&read_limited(path, 32 * 1024)?)?;
    safe_path(&job.app)?;
    ensure!(
        stage.parent() == job.app.parent(),
        "更新必须在应用同级目录暂存。"
    );
    ensure!(
        job.parent_pid > 1 && job.parent_pid <= i32::MAX as u32,
        "更新等待进程无效。"
    );
    #[cfg(not(debug_assertions))]
    ensure!(
        job.public_key.trim() == KEY.trim(),
        "正式更新助手拒绝替换信任密钥。"
    );
    Ok((stage, job))
}

/// Called synchronously after confirmation. All preflight work and the helper handshake complete
/// before the caller quits its event loop; failures keep the existing application running.
pub fn start_update(
    bytes: &[u8],
    public_key: &str,
    current: &str,
    version: &str,
    signature: &str,
    cancel: &CancellationToken,
) -> Result<()> {
    canceled(cancel)?;
    ensure!(bytes.len() <= MAX_ARCHIVE_BYTES, "更新包超过大小限制。");
    crate::update::verify(bytes, public_key, signature, current, version)?;
    #[cfg(not(debug_assertions))]
    ensure!(
        public_key.trim() == KEY.trim(),
        "正式更新拒绝替换信任密钥。"
    );
    let executable = std::env::current_exe()?;
    let app = installed_bundle(&executable)?;
    let team = validate_bundle(&app, current, None)?;
    removable_tree(&app)?;
    let stage = private_directory(app.parent().context("无法确定安装目录。")?)?;
    let result = (|| -> Result<()> {
        fs::create_dir(stage.join("payload"))?;
        extract_archive(bytes, &stage.join("payload"), cancel)?;
        validate_bundle(&stage.join("payload/PortCanvas.app"), version, Some(&team))?;
        canceled(cancel)?;
        write_new(&stage.join("archive.tar.gz"), bytes, 0o600)?;
        crate::update::verify(
            &read_limited(&stage.join("archive.tar.gz"), MAX_ARCHIVE_BYTES as u64)?,
            public_key,
            signature,
            current,
            version,
        )?;
        let job = Job {
            app,
            current: current.into(),
            version: version.into(),
            signature: signature.into(),
            public_key: public_key.into(),
            parent_pid: std::process::id(),
        };
        write_new(&stage.join("job.json"), &serde_json::to_vec(&job)?, 0o600)?;
        fs::copy(executable, stage.join("runner"))?;
        fs::set_permissions(stage.join("runner"), fs::Permissions::from_mode(0o700))?;
        File::open(stage.join("runner"))?.sync_all()?;
        let helper_log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(stage.join("helper.log"))?;
        let mut child = Command::new(stage.join("runner"))
            .arg("--portcanvas-update-helper")
            .arg(stage.join("job.json"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(helper_log))
            .spawn()
            .context("无法启动独立更新助手。")?;
        let began = Instant::now();
        let handshake = (|| -> Result<()> {
            loop {
                canceled(cancel)?;
                ensure!(
                    child.try_wait()?.is_none(),
                    "更新助手预检失败，应用未退出。 {}",
                    fs::read_to_string(stage.join("helper.log"))
                        .unwrap_or_default()
                        .trim()
                );
                if stage.join("prepared").is_file() {
                    return Ok(());
                }
                ensure!(
                    began.elapsed() < Duration::from_secs(30),
                    "更新助手未响应，应用未退出。"
                );
                std::thread::sleep(Duration::from_millis(25));
            }
        })();
        if let Err(error) = handshake {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = remove_stage(&stage);
    }
    result
}

fn helper(path: &Path) -> Result<()> {
    let (stage, job) = job_from_file(path)?;
    ensure!(
        std::env::current_exe()? == stage.join("runner"),
        "更新助手不在受控暂存目录。"
    );
    ensure!(
        job.parent_pid != std::process::id(),
        "更新助手不能等待自身。"
    );
    let operation = (|| -> Result<()> {
        let bytes = read_limited(&stage.join("archive.tar.gz"), MAX_ARCHIVE_BYTES as u64)?;
        // Trust the on-disk signature again in the independent process, never a ready flag alone.
        crate::update::verify(
            &bytes,
            &job.public_key,
            &job.signature,
            &job.current,
            &job.version,
        )?;
        let team = validate_bundle(&job.app, &job.current, None)?;
        // Recreate the bundle exclusively from the newly verified archive in this process.
        fs::remove_dir_all(stage.join("payload"))?;
        fs::create_dir(stage.join("payload"))?;
        extract_archive(&bytes, &stage.join("payload"), &CancellationToken::new())?;
        validate_bundle(
            &stage.join("payload/PortCanvas.app"),
            &job.version,
            Some(&team),
        )?;
        write_new(&stage.join("prepared"), b"ready", 0o600)?;
        let began = Instant::now();
        loop {
            let alive = unsafe { libc::kill(job.parent_pid as i32, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
            if !alive {
                break;
            }
            ensure!(
                began.elapsed() < Duration::from_secs(120),
                "原应用未退出，已取消更新。"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        replace_and_launch(&stage, &job, Duration::from_secs(45))?;
        let cleanup = crate::macos_legacy_cleanup::current_user_after_readiness();
        for failure in cleanup.failures {
            eprintln!("{failure}");
        }
        Ok(())
    })();
    if let Err(error) = &operation {
        // If rollback itself failed, keep its unique old-code recovery location intact.
        if stage.join("payload/PortCanvas.app").exists()
            && plist(&job.app, "CFBundleShortVersionString")
                .ok()
                .as_deref()
                != Some(job.current.as_str())
        {
            return operation;
        }
        let _ = remove_stage(&stage);
        let old_alive = unsafe { libc::kill(job.parent_pid as i32, 0) } == 0;
        if !old_alive {
            let _ = Command::new(job.app.join(EXECUTABLE))
                .arg("--portcanvas-update-failed")
                .arg(format!("更新失败，已恢复原应用。\n{error:#}"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }
    operation
}

/// Entry-point dispatch before any GUI, data locks, services or media initialization.
pub fn dispatch_helper() -> Result<bool> {
    if std::env::args_os().nth(1).as_deref()
        != Some(std::ffi::OsStr::new("--portcanvas-update-helper"))
    {
        return Ok(false);
    }
    let path = std::env::args_os()
        .nth(2)
        .context("更新助手缺少任务文件。")?;
    helper(Path::new(&path))?;
    Ok(true)
}

/// Called once after the replacement app actually renders its first initialized frame.
pub fn acknowledge_launch() -> Result<()> {
    if std::env::args_os().nth(1).as_deref()
        != Some(std::ffi::OsStr::new("--portcanvas-update-ack"))
    {
        return Ok(());
    }
    let path = PathBuf::from(
        std::env::args_os()
            .nth(2)
            .context("更新启动确认路径缺失。")?,
    );
    ensure!(
        path.file_name().is_some_and(|v| v == "launched"),
        "更新启动确认路径无效。"
    );
    let stage = path.parent().context("更新启动确认目录缺失。")?;
    let (_, job) = job_from_file(&stage.join("job.json"))?;
    ensure!(
        std::env::current_exe()? == job.app.join(EXECUTABLE)
            && job.version == env!("CARGO_PKG_VERSION"),
        "更新启动确认应用不匹配。"
    );
    write_new(&path, b"ready", 0o600)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, path::PathBuf};
    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Self {
            let root = std::env::var_os("WCS_TEST_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir);
            let path = root.join(format!("macos-update-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn archive(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
        let out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(out);
        for (name, kind, data) in entries {
            let mut header = tar::Header::new_ustar();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_entry_type(*kind);
            // Deliberately build raw malicious paths; Builder::append_data correctly refuses them.
            let raw = header.as_mut_bytes();
            raw[..100].fill(0);
            raw[..name.len()].copy_from_slice(name.as_bytes());
            if kind.is_symlink() || kind.is_hard_link() {
                header.set_link_name("/tmp/outside").unwrap();
            }
            header.set_cksum();
            tar.append(&header, *data).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }
    fn bundle(root: &Path, version: &str, identifier: &str, behavior: &str) -> PathBuf {
        use std::process::Command;
        let app = root.join("PortCanvas.app");
        let contents = app.join("Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::write(contents.join("Info.plist"),format!(r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{identifier}</string><key>CFBundleExecutable</key><string>PortCanvasNative</string><key>CFBundleShortVersionString</key><string>{version}</string><key>CFBundleVersion</key><string>{version}</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#)).unwrap();
        let code = root.join("fixture.c");
        fs::write(&code,format!("#include <stdio.h>\n#include <unistd.h>\nint main(int argc,char **argv){{ {behavior} return 0;}}\n")).unwrap();
        assert!(Command::new("/usr/bin/cc")
            .arg(&code)
            .arg("-o")
            .arg(contents.join("MacOS/PortCanvasNative"))
            .status()
            .unwrap()
            .success());
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-", "--timestamp=none"])
            .arg(&app)
            .output()
            .unwrap()
            .status
            .success());
        app
    }
    #[test]
    fn bundle_validation_checks_identity_version_architecture_and_signature() {
        let s = Sandbox::new();
        let app = bundle(&s.0, "0.6.2", "com.portcanvas.desktop", "");
        assert_eq!(validate_bundle(&app, "0.6.2", None).unwrap(), "adhoc");
        assert!(validate_bundle(&app, "0.6.3", None).is_err());
        assert!(validate_bundle(&app, "0.6.2", Some("EXPECTEDTEAM")).is_err());
        fs::write(app.join("Contents/Info.plist"), b"invalid plist").unwrap();
        assert!(validate_bundle(&app, "0.6.2", None).is_err());
        let other = Sandbox::new();
        let wrong = bundle(&other.0, "0.6.2", "com.attacker.desktop", "");
        assert!(validate_bundle(&wrong, "0.6.2", None).is_err());
        let unsigned = Sandbox::new();
        let wrong = bundle(&unsigned.0, "0.6.2", "com.portcanvas.desktop", "");
        assert!(std::process::Command::new("/usr/bin/codesign")
            .arg("--remove-signature")
            .arg(wrong.join("Contents/MacOS/PortCanvasNative"))
            .status()
            .unwrap()
            .success());
        assert!(validate_bundle(&wrong, "0.6.2", None).is_err());
    }
    #[test]
    fn extracts_a_regular_bundle_without_executing_payload() {
        let s = Sandbox::new();
        let bytes = archive(&[(
            "PortCanvas.app/Contents/Info.plist",
            tar::EntryType::Regular,
            b"plist",
        )]);
        extract_archive(&bytes, &s.0, &CancellationToken::new()).unwrap();
        assert_eq!(
            fs::read(s.0.join("PortCanvas.app/Contents/Info.plist")).unwrap(),
            b"plist"
        );
    }
    #[test]
    fn rejects_archive_traversal_links_and_special_files() {
        for (path, kind) in [
            ("../escape", tar::EntryType::Regular),
            ("/tmp/escape", tar::EntryType::Regular),
            ("PortCanvas.app/../escape", tar::EntryType::Regular),
            ("Other.app/file", tar::EntryType::Regular),
            ("PortCanvas.app/link", tar::EntryType::Symlink),
            ("PortCanvas.app/link", tar::EntryType::Link),
            ("PortCanvas.app/pipe", tar::EntryType::Fifo),
            ("PortCanvas.app/Contents/./file", tar::EntryType::Regular),
        ] {
            let s = Sandbox::new();
            let bytes = archive(&[(path, kind, b"")]);
            assert!(
                extract_archive(&bytes, &s.0, &CancellationToken::new()).is_err(),
                "accepted {path}"
            );
        }
    }
    #[test]
    fn cancellation_never_produces_an_installable_bundle() {
        let s = Sandbox::new();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let bytes = archive(&[(
            "PortCanvas.app/Contents/Info.plist",
            tar::EntryType::Regular,
            b"plist",
        )]);
        assert!(extract_archive(&bytes, &s.0, &cancel).is_err());
        assert!(!s.0.join("PortCanvas.app").exists());
    }
    #[test]
    fn archive_cannot_follow_preexisting_directory_symlink_or_overwrite_files() {
        use std::os::unix::fs::symlink;
        let s = Sandbox::new();
        let outside = Sandbox::new();
        symlink(&outside.0, s.0.join("PortCanvas.app")).unwrap();
        let bytes = archive(&[(
            "PortCanvas.app/Contents/file",
            tar::EntryType::Regular,
            b"new",
        )]);
        assert!(extract_archive(&bytes, &s.0, &CancellationToken::new()).is_err());
        assert!(!outside.0.join("Contents").exists());
        let bytes = archive(&[("PortCanvas.app/file", tar::EntryType::Regular, b"new")]);
        fs::remove_file(s.0.join("PortCanvas.app")).unwrap();
        fs::create_dir(s.0.join("PortCanvas.app")).unwrap();
        fs::File::create(s.0.join("PortCanvas.app/file"))
            .unwrap()
            .write_all(b"original")
            .unwrap();
        assert!(extract_archive(&bytes, &s.0, &CancellationToken::new()).is_err());
        assert_eq!(
            fs::read(s.0.join("PortCanvas.app/file")).unwrap(),
            b"original"
        );
    }
    fn transaction_fixture(behavior: &str) -> (Sandbox, PathBuf, Job) {
        let s = Sandbox::new();
        let old = s.0.join("installed");
        fs::create_dir(&old).unwrap();
        let app = bundle(&old, "0.6.1", BUNDLE_ID, "");
        let stage = private_directory(&old).unwrap();
        fs::create_dir(stage.join("payload")).unwrap();
        bundle(&stage.join("payload"), "0.6.2", BUNDLE_ID, behavior);
        let job = Job {
            app,
            current: "0.6.1".into(),
            version: "0.6.2".into(),
            signature: String::new(),
            public_key: String::new(),
            parent_pid: std::process::id(),
        };
        fs::write(s.0.join("user-data.json"), b"preserved user data").unwrap();
        (s, stage, job)
    }
    #[test]
    fn successful_application_restart_removes_old_code_and_preserves_user_data() {
        let (s, stage, job) = transaction_fixture(
            r#"if(argc==3){FILE *f=fopen(argv[2],"w");if(!f)return 7;fputs("ready",f);fclose(f);usleep(200000);}"#,
        );
        let mut child = replace_and_launch(&stage, &job, Duration::from_secs(2)).unwrap();
        assert_eq!(
            plist(&job.app, "CFBundleShortVersionString").unwrap(),
            "0.6.2"
        );
        assert!(!stage.exists());
        assert_eq!(
            fs::read(s.0.join("user-data.json")).unwrap(),
            b"preserved user data"
        );
        assert!(child.wait().unwrap().success());
    }
    #[test]
    fn failed_application_start_restores_previous_bundle_and_removes_failed_code() {
        let (s, stage, job) = transaction_fixture("return 9;");
        assert!(replace_and_launch(&stage, &job, Duration::from_secs(2)).is_err());
        assert_eq!(
            plist(&job.app, "CFBundleShortVersionString").unwrap(),
            "0.6.1"
        );
        assert!(!stage.exists());
        assert_eq!(
            fs::read(s.0.join("user-data.json")).unwrap(),
            b"preserved user data"
        );
    }
    #[test]
    fn unresponsive_new_application_is_stopped_before_rollback() {
        let (_s, stage, job) = transaction_fixture("sleep(20);");
        let started = Instant::now();
        assert!(replace_and_launch(&stage, &job, Duration::from_millis(100)).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(
            plist(&job.app, "CFBundleShortVersionString").unwrap(),
            "0.6.1"
        );
        assert!(!stage.exists());
    }
    #[test]
    fn invalid_new_bundle_never_replaces_installed_application() {
        let (_s, stage, job) = transaction_fixture("");
        fs::write(
            stage.join("payload/PortCanvas.app/Contents/Info.plist"),
            b"bad metadata",
        )
        .unwrap();
        assert!(replace_and_launch(&stage, &job, Duration::from_millis(100)).is_err());
        assert_eq!(
            plist(&job.app, "CFBundleShortVersionString").unwrap(),
            "0.6.1"
        );
    }

    #[test]
    fn a_readonly_old_bundle_is_rejected_before_replacement() {
        let (_s, stage, job) = transaction_fixture(
            r#"if(argc==3){FILE *f=fopen(argv[2],"w");fputs("ready",f);fclose(f);usleep(200000);}"#,
        );
        fs::set_permissions(job.app.join("Contents"), fs::Permissions::from_mode(0o555)).unwrap();
        let result = replace_and_launch(&stage, &job, Duration::from_secs(2));
        for path in [
            job.app.join("Contents"),
            stage.join("payload/PortCanvas.app/Contents"),
        ] {
            if path.exists() {
                fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        assert!(result.is_err());
        assert_eq!(
            plist(&job.app, "CFBundleShortVersionString").unwrap(),
            "0.6.1"
        );
    }

    #[test]
    fn an_archive_for_only_the_other_cpu_architecture_is_rejected() {
        let s = Sandbox::new();
        let app = bundle(&s.0, "0.6.2", BUNDLE_ID, "");
        let other = if cfg!(target_arch = "aarch64") {
            "x86_64"
        } else {
            "arm64"
        };
        assert!(Command::new("/usr/bin/cc")
            .arg("-arch")
            .arg(other)
            .arg(s.0.join("fixture.c"))
            .arg("-o")
            .arg(app.join(EXECUTABLE))
            .status()
            .unwrap()
            .success());
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-", "--timestamp=none"])
            .arg(&app)
            .output()
            .unwrap()
            .status
            .success());
        assert!(validate_bundle(&app, "0.6.2", None)
            .unwrap_err()
            .to_string()
            .contains("架构"));
    }
    #[test]
    fn cancellation_during_large_file_extraction_stops_before_a_complete_bundle_exists() {
        let s = Sandbox::new();
        let cancel = CancellationToken::new();
        let data = vec![0u8; 16 * 1024 * 1024];
        let bytes = archive(&[(
            "PortCanvas.app/Contents/MacOS/PortCanvasNative",
            tar::EntryType::Regular,
            &data,
        )]);
        let target = s.0.join("PortCanvas.app/Contents/MacOS/PortCanvasNative");
        let stop = cancel.clone();
        let probe = target.clone();
        let thread = std::thread::spawn(move || {
            let began = Instant::now();
            while !probe.exists() && began.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(1));
            }
            stop.cancel();
        });
        let result = extract_archive(&bytes, &s.0, &cancel);
        thread.join().unwrap();
        assert!(result.is_err());
        assert!(fs::metadata(target).unwrap().len() < data.len() as u64);
        assert!(!s.0.join("PortCanvas.app/Contents/Info.plist").exists());
    }
    #[test]
    fn oversized_archive_member_is_rejected_before_creating_its_file() {
        let s = Sandbox::new();
        let mut header = tar::Header::new_ustar();
        header.set_path("PortCanvas.app/huge").unwrap();
        header.set_size(MAX_ARCHIVE_BYTES as u64 + 1);
        header.set_mode(0o644);
        header.set_cksum();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(header.as_bytes()).unwrap();
        gz.write_all(&[0u8; 1024]).unwrap();
        let bytes = gz.finish().unwrap();
        assert!(extract_archive(&bytes, &s.0, &CancellationToken::new()).is_err());
        assert!(!s.0.join("PortCanvas.app/huge").exists());
    }
    #[test]
    fn extended_archive_headers_cannot_bypass_member_resource_limits() {
        let s = Sandbox::new();
        let bytes = archive(&[
            (
                "PortCanvas.app/extension",
                tar::EntryType::GNULongName,
                b"PortCanvas.app/Contents/Info.plist\0",
            ),
            ("PortCanvas.app/fallback", tar::EntryType::Regular, b"plist"),
        ]);
        assert!(extract_archive(&bytes, &s.0, &CancellationToken::new()).is_err());
        assert!(!s.0.join("PortCanvas.app/Contents/Info.plist").exists());
    }
    #[test]
    fn code_signature_identity_must_match_the_bundle_identifier() {
        let s = Sandbox::new();
        let app = bundle(&s.0, "0.6.2", BUNDLE_ID, "");
        assert!(Command::new("/usr/bin/codesign")
            .args([
                "--force",
                "--sign",
                "-",
                "--timestamp=none",
                "--identifier",
                "com.unrelated.application"
            ])
            .arg(&app)
            .output()
            .unwrap()
            .status
            .success());
        assert!(validate_bundle(&app, "0.6.2", None).is_err());
    }
}

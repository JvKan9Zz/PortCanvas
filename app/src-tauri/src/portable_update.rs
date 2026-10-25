// The official Windows updater installs NSIS packages. Portable builds replace only
// their executable and bundled notices, using the verified new executable as helper.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize, Serialize)]
struct Plan {
    target: PathBuf,
    parent: u32,
    data: PathBuf,
}

pub fn prepare(bytes: &[u8], data: &Path) -> Result<PathBuf> {
    anyhow::ensure!(
        bytes.starts_with(b"MZ"),
        "便携版更新文件不是 Windows 程序。"
    );
    let target = std::env::current_exe()?;
    let directory = target.parent().context("找不到程序目录。")?;
    let stage = directory.join(format!(".portcanvas-update-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage).context("程序目录不可写，无法更新便携版。")?;
    let result = (|| {
        std::fs::write(stage.join("PortCanvas.exe"), bytes)?;
        std::fs::write(
            stage.join("plan.json"),
            serde_json::to_vec(&Plan {
                target,
                parent: std::process::id(),
                data: data.into(),
            })?,
        )?;
        Ok(stage.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    result
}
pub fn launch(stage: &Path) -> Result<()> {
    Command::new(stage.join("PortCanvas.exe"))
        .arg("--apply-portable-update")
        .spawn()
        .context("无法启动便携版更新助手。")?;
    Ok(())
}
pub fn run_helper() -> bool {
    if !std::env::args().any(|arg| arg == "--apply-portable-update") {
        return false;
    }
    let result = (|| -> Result<()> {
        let executable = std::env::current_exe()?;
        let stage = executable.parent().context("更新目录无效。")?;
        let plan: Plan = serde_json::from_slice(&std::fs::read(stage.join("plan.json"))?)?;
        validate_stage(stage, &plan.target)?;
        let result = apply(&executable, stage, &plan);
        let message = match &result {
            Ok(_) => {
                serde_json::json!({"status":"updated","version":env!("CARGO_PKG_VERSION"),"stage":stage})
            }
            Err(error) => {
                serde_json::json!({"status":"error","message":format!("安装更新失败：{error}"),"stage":stage})
            }
        };
        let _ = std::fs::write(
            plan.data.join("update-result.json"),
            serde_json::to_vec(&message)?,
        );
        if let Err(error) = Command::new(&plan.target)
            .arg("--portable")
            .current_dir(plan.target.parent().unwrap())
            .spawn()
        {
            if result.is_ok() {
                std::fs::remove_file(&plan.target)?;
                std::fs::rename(stage.join("previous.exe"), &plan.target)?;
                let failure = serde_json::json!({"status":"error","message":"新版启动失败，已恢复旧程序。","stage":stage});
                std::fs::write(
                    plan.data.join("update-result.json"),
                    serde_json::to_vec(&failure)?,
                )?;
                let _ = Command::new(&plan.target).arg("--portable").spawn();
            }
            return Err(error).context("更新后无法重新启动程序，请手动打开 PortCanvas。");
        }
        result
    })();
    if let Err(error) = result {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let message = format!("PortCanvas 更新未完成：{error}")
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let title = "PortCanvas"
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    true
}
fn validate_stage(stage: &Path, target: &Path) -> Result<()> {
    anyhow::ensure!(
        target.is_absolute()
            && target.file_name() == Some(std::ffi::OsStr::new("PortCanvas.exe"))
            && stage.parent() == target.parent()
            && stage
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(".portcanvas-update-")),
        "更新目录与程序路径不匹配。"
    );
    Ok(())
}
fn apply(executable: &Path, stage: &Path, plan: &Plan) -> Result<()> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
            System::Threading::*,
        };
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, plan.parent);
        if process.is_null() {
            anyhow::ensure!(
                std::io::Error::last_os_error().raw_os_error()
                    == Some(ERROR_INVALID_PARAMETER as i32),
                "无法等待旧程序退出。"
            );
        } else {
            let waited = WaitForSingleObject(process, 30000);
            CloseHandle(process);
            anyhow::ensure!(waited == WAIT_OBJECT_0, "旧程序尚未退出，请关闭后重试。");
        }
    }
    let backup = stage.join("previous.exe");
    std::fs::rename(&plan.target, &backup).context("旧程序仍被占用，未替换文件。")?;
    if let Err(error) = std::fs::copy(executable, &plan.target) {
        let _ = std::fs::remove_file(&plan.target);
        std::fs::rename(&backup, &plan.target)
            .context("更新失败，恢复旧程序失败；旧文件保留在更新目录。")?;
        return Err(error).context("无法写入新程序，已恢复旧程序。");
    }
    // The notices are bundled into the signed executable. No unsigned sidecar is downloaded.
    if let Err(error) = crate::profiles::atomic_bytes(
        &plan.target.with_file_name("THIRD-PARTY-NOTICES.txt"),
        include_bytes!("../../THIRD-PARTY-NOTICES.txt"),
    ) {
        std::fs::remove_file(&plan.target)?;
        std::fs::rename(&backup, &plan.target)?;
        return Err(error).context("无法更新许可说明，已恢复旧程序。");
    }
    Ok(())
}
pub fn result(data: &Path) -> Option<serde_json::Value> {
    let file = data.join("update-result.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
    if let (Some(stage), Ok(target)) = (value["stage"].as_str(), std::env::current_exe()) {
        let stage = Path::new(stage);
        if validate_stage(stage, &target).is_ok() {
            let stage = stage.to_path_buf();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let _ = std::fs::remove_dir_all(stage);
            });
        }
    }
    let _ = std::fs::remove_file(file);
    Some(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_and_helper_paths_cannot_escape_program_directory() {
        let root = std::env::temp_dir().join("portable-tests");
        let target = root.join("PortCanvas.exe");
        assert!(validate_stage(&root.join(".portcanvas-update-test"), &target).is_ok());
        assert!(validate_stage(&root.join("data"), &target).is_err());
        assert!(validate_stage(&root.join("child/.portcanvas-update-test"), &target).is_err());
    }
}

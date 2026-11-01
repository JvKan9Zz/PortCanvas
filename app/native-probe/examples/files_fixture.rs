#[path = "../tests/support/terminal_fixture.rs"]
mod support;
use anyhow::{Context, Result};
use std::{path::PathBuf, time::Duration};
#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").context("isolated root required")?);
    let data = root.join("files-gui");
    anyhow::ensure!(!data.exists(), "fresh directory required");
    std::fs::create_dir_all(&data)?;
    let local = root.join("local-files");
    let remote = root.join("remote-files");
    std::fs::create_dir_all(local.join("中文目录"))?;
    std::fs::create_dir_all(remote.join("远端目录"))?;
    std::fs::write(
        local.join("中文目录/设置.txt"),
        "native synthetic file\n中文🙂",
    )?;
    std::fs::write(local.join("local.txt"), "local fixture")?;
    std::fs::write(local.join("large.bin"), vec![71; 8 * 1024 * 1024])?;
    std::fs::write(remote.join("remote.txt"), "remote fixture")?;
    let mut f = support::Fixture::with_files(remote.clone()).await?;
    f.profile.name = "本机文件验证".into();
    portcanvas_core::profiles::ProfileStore {
        directory: data.clone(),
    }
    .save(f.profile.clone(), "synthetic-only")?;
    std::fs::write(
        root.join("files-ready.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"data":data,"local":local,"remote":remote,"fingerprint":f.fingerprint,"port":f.profile.port}),
        )?,
    )?;
    let end = tokio::time::Instant::now() + Duration::from_secs(3600);
    while tokio::time::Instant::now() < end && !root.join("files-stop").exists() {
        std::fs::write(
            root.join("files-counts.json"),
            serde_json::to_vec_pretty(&f.report())?,
        )?;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    std::fs::write(
        root.join("files-result.json"),
        serde_json::to_vec_pretty(&f.report())?,
    )?;
    Ok(())
}

#[path = "../tests/support/terminal_fixture.rs"]
mod support;
use anyhow::{Context, Result};
use portcanvas_core::profiles::ProfileStore;
use std::{path::PathBuf, time::Duration};
#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("WCS_TEST_ROOT").context("isolated WCS_TEST_ROOT required")?,
    );
    let data = root.join("desktop-gui");
    anyhow::ensure!(!data.exists(), "fresh fixture directory required");
    std::fs::create_dir_all(&data)?;
    let mut fixture = support::Fixture::new().await?;
    fixture.profile.name = "本机桌面验证".into();
    ProfileStore {
        directory: data.clone(),
    }
    .save(fixture.profile.clone(), "synthetic-only")?;
    std::fs::write(
        root.join("desktop-ready.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"data":data,"port":fixture.profile.port,"fingerprint":fixture.fingerprint}),
        )?,
    )?;
    let end = tokio::time::Instant::now() + Duration::from_secs(1800);
    while tokio::time::Instant::now() < end && !root.join("desktop-stop").exists() {
        std::fs::write(
            root.join("desktop-counts.json"),
            serde_json::to_vec_pretty(&fixture.report())?,
        )?;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    std::fs::write(
        root.join("desktop-result.json"),
        serde_json::to_vec_pretty(&fixture.report())?,
    )?;
    Ok(())
}

#[path = "../tests/support/terminal_fixture.rs"]
mod support;
use anyhow::{Context, Result};
use portcanvas_core::profiles::ProfileStore;
use std::{path::PathBuf, sync::atomic::Ordering, time::Duration};
#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("WCS_TEST_ROOT").context("isolated WCS_TEST_ROOT required")?,
    );
    let data = root.join("arena-gui");
    anyhow::ensure!(!data.exists(), "fresh fixture directory required");
    std::fs::create_dir_all(&data)?;
    let mut fixture = support::Fixture::new().await?;
    fixture.profile.name = "赛台受控验证（非实机）".into();
    let store = ProfileStore {
        directory: data.clone(),
    };
    store.save(fixture.profile.clone(), "synthetic-only")?;
    store.trust(&fixture.profile.address(), &fixture.fingerprint)?;
    std::fs::write(
        root.join("arena-ready.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"data":data,"port":fixture.profile.port}))?,
    )?;
    let end = tokio::time::Instant::now() + Duration::from_secs(1800);
    while tokio::time::Instant::now() < end && !root.join("arena-stop").exists() {
        if let Ok(mode) = std::fs::read_to_string(root.join("arena-pose-mode")) {
            if let Ok(n) = mode.trim().parse() {
                fixture.counts.arena.pose_mode.store(n, Ordering::SeqCst);
            }
        }
        let counts = &fixture.counts.arena;
        std::fs::write(
            root.join("arena-counts.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"mode":counts.pose_mode.load(Ordering::SeqCst),"camera":counts.cameras.load(Ordering::SeqCst),"pose":counts.poses.load(Ordering::SeqCst),"metrics":counts.metrics.load(Ordering::SeqCst)}),
            )?,
        )?;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Ok(())
}

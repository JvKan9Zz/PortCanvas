#[path = "../tests/support/terminal_fixture.rs"]
mod support;
use anyhow::{ensure, Result};
use portcanvas_core::profiles::ProfileStore;
use std::{path::PathBuf, sync::atomic::Ordering, time::Duration};
#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated root required"));
    let data = root.join("data");
    ensure!(!data.exists(), "fresh fixture required");
    std::fs::create_dir_all(&data)?;
    let mut fixture = support::Fixture::new().await?;
    fixture.profile.name = "重启协议验证（非实机）".into();
    let store = ProfileStore {
        directory: data.clone(),
    };
    store.save(fixture.profile.clone(), "synthetic-only")?;
    store.trust(&fixture.profile.address(), &fixture.fingerprint)?;
    std::fs::write(
        root.join("ready.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"data":data,"port":fixture.profile.port}))?,
    )?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(900);
    while tokio::time::Instant::now() < deadline && !root.join("stop").exists() {
        if let Ok(value) = std::fs::read_to_string(root.join("reboot-status")) {
            if let Ok(status) = value.trim().parse() {
                fixture.counts.reboot_status.store(status, Ordering::SeqCst);
            }
        }
        std::fs::write(
            root.join("counts.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"requests":fixture.counts.reboots.load(Ordering::SeqCst),"status":fixture.counts.reboot_status.load(Ordering::SeqCst)}),
            )?,
        )?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

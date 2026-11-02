use anyhow::{ensure, Result};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn write(file: &Path, value: serde_json::Value) -> Result<()> {
    Ok(fs::write(file, serde_json::to_vec_pretty(&value)?)?)
}
fn main() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated root required"));
    ensure!(
        root.is_absolute() && !root.join("data").exists(),
        "fresh isolated fixture required"
    );
    for dir in ["data", "old-data", "old-logs", "current-logs"] {
        fs::create_dir_all(root.join(dir))?;
    }
    let profile = |id: &str, name: &str| json!({"id":id,"name":name,"host":"192.0.2.1","port":22,"username":"fixture","futureDevice":{"label":name}});
    write(
        &root.join("data/devices.json"),
        json!({"devices":[profile("current","当前设备（合成资料）")],"hostKeys":{},"futureRoot":{"current":true}}),
    )?;
    write(
        &root.join("old-data/devices.json"),
        json!({"devices":[profile("legacy","旧版设备（合成资料）")],"hostKeys":{},"legacyRoot":{"legacy":true}}),
    )?;
    write(
        &root.join("data/log-settings.json"),
        json!({"root":root.join("current-logs"),"maxBytes":16777216,"projects":{},"currentLog":{"v":1}}),
    )?;
    write(
        &root.join("old-data/log-settings.json"),
        json!({"root":root.join("old-logs"),"projects":{"old":"旧项目"},"legacyLog":{"v":2}}),
    )?;
    fs::write(root.join("old-logs/keep.log"), b"synthetic preserved log")?;
    write(
        &root.join("ready.json"),
        json!({"source":root.join("old-data"),"target":root.join("data")}),
    )?;
    Ok(())
}

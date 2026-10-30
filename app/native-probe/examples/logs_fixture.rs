//! Owned local records, including a live external writer. No SSH or system commands.
use anyhow::{Context, Result};
use portcanvas_core::{log_protocol::Marker, log_store::LogStore, profiles::atomic_json};
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn marker(n: usize) -> Marker {
    Marker {
        kind: "begin".into(),
        shell: "native-logs-fixture".into(),
        sequence: n.to_string(),
        code: Some(if n == 3 { 7 } else { 0 }),
        command: if n == 208 {
            "合成实时记录".into()
        } else {
            format!("合成日志 {n:03}")
        },
        cwd: "/fixture/设置".into(),
        project_root: "/fixture/设置".into(),
        end_cwd: "/fixture/设置".into(),
        background_jobs: Vec::new(),
    }
}
fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("WCS_TEST_ROOT").context("isolated WCS_TEST_ROOT required")?,
    );
    anyhow::ensure!(root.is_absolute(), "WCS_TEST_ROOT must be absolute");
    let data = root.join("logs-gui");
    anyhow::ensure!(!data.exists(), "Use a fresh isolated fixture directory");
    std::fs::create_dir_all(&data)?;
    let logs = data.join("logs");
    let mut store = LogStore::new(logs.clone())?;
    for n in 0..207 {
        let item = marker(n);
        let id = store
            .begin(&item, "fixture", "", false, "command")?
            .context("fixture record")?;
        store.append(
            &id,
            if n == 206 {
                "设置不是界面标签，而是日志原文。\n中文、emoji 🧪、特殊路径 /tmp/a b/设置\n"
            } else {
                "synthetic output\n"
            },
        )?;
        if n == 205 {
            store.append(&id, &"长输出边界\n".repeat(30000))?;
        }
        if n == 204 {
            store.finish(&id, None, Some("合成中断，保留缺失提示"))?;
        } else {
            store.finish(&id, Some(&item), None)?;
        }
    }
    let live = store
        .begin(&marker(208), "fixture", "", false, "command")?
        .context("live fixture")?;
    store.append(&live, "SYNTHETIC_LIVE_OUTPUT\n")?;
    store.flush()?;
    atomic_json(
        &data.join("log-settings.json"),
        &json!({"root":logs,"projects":{},"maxBytes":1073741824u64}),
    )?;
    std::fs::write(logs.join("ordinary-keep.txt"), "ordinary fixture file")?;
    std::fs::create_dir_all(data.join("chosen-logs"))?;
    atomic_json(
        &root.join("logs-ready.json"),
        &json!({"data":data,"live":live,"records":208}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut ticks = 0;
    while Instant::now() < deadline && !root.join("logs-stop").exists() {
        ticks += 1;
        store.append(&live, &format!("TICK {ticks:04}\n"))?;
        store.flush()?;
        std::thread::sleep(Duration::from_millis(500));
    }
    store.close("local fixture completed")?;
    atomic_json(
        &root.join("logs-result.json"),
        &json!({"ticks":ticks,"records":208,"ordinaryPreserved":logs.join("ordinary-keep.txt").is_file()}),
    )?;
    Ok(())
}

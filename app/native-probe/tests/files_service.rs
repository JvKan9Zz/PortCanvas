#[path = "support/terminal_fixture.rs"]
mod support;
use anyhow::Result;
use portcanvas_core::{
    files::{Confirmation, Side},
    services::{Paths, Services},
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
async fn until(label: &str, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout: {label}"));
}
async fn fixture() -> Result<(support::Fixture, Arc<Services>, PathBuf)> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap())
        .join(format!("files-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("remote"))?;
    std::fs::create_dir_all(root.join("local"))?;
    let f = support::Fixture::with_files(root.join("remote")).await?;
    let s = Services::new(
        Paths {
            data: root.join("data"),
            documents: root.clone(),
            local_files: root.join("local"),
            portable: true,
        },
        Arc::new(|_, _| {}),
        Some("files-tests".into()),
    )?;
    connect(&s, &f).await?;
    Ok((f, s, root))
}
async fn connect(s: &Arc<Services>, f: &support::Fixture) -> Result<()> {
    let fingerprint = f.fingerprint.clone();
    s.connect(
        f.profile.clone(),
        zeroize::Zeroizing::new("synthetic-only".into()),
        Arc::new(move |identity, _| {
            let accepted = identity.fingerprint == fingerprint;
            Box::pin(async move { Ok(accepted) })
        }),
    )
    .await?;
    Ok(())
}
fn confirm(yes: bool) -> Confirmation {
    Arc::new(move |_| Box::pin(async move { Ok(yes) }))
}
fn options(s: &Services, root: &Path, direction: &str, names: Vec<&str>, rate: u64) -> Value {
    json!({"generation":s.devices.generation(),"direction":direction,"localPath":root.join("local"),"remotePath":"/","names":names,"rateLimit":rate})
}
fn temps(root: &Path) -> usize {
    std::fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".wcs-transfer-")
        })
        .count()
}
#[tokio::test]
async fn remote_delete_permission_failures_preserve_data_and_report_account_and_path() -> Result<()> {
    let (f,s,root)=fixture().await?;
    let directory=root.join("remote/locked dir");
    std::fs::create_dir(&directory)?;
    let file=directory.join("传感器.txt");
    std::fs::write(&file,b"keep on denied deletion")?;
    let removal=json!({"generation":s.devices.generation(),"parent":"/","names":["locked dir"]});
    f.counts.files.deny_listings.store(true,Ordering::SeqCst);
    let no_confirm:Confirmation=Arc::new(|_|Box::pin(async{anyhow::bail!("must inspect before confirming")}));
    let state=s.files.remove_remote(serde_json::from_value(removal.clone())?,no_confirm).await?;
    assert_eq!(state["phase"],"failed");
    assert!(state["message"].as_str().unwrap().contains("/locked dir"));
    assert!(state["message"].as_str().unwrap().contains(&f.profile.username));
    assert_eq!(std::fs::read(&file)?,b"keep on denied deletion");
    f.counts.files.deny_listings.store(false,Ordering::SeqCst);
    f.counts.files.deny_removals.store(true,Ordering::SeqCst);
    let state=s.files.remove_remote(serde_json::from_value(removal.clone())?,confirm(true)).await?;
    assert_eq!(state["phase"],"failed");assert_eq!(state["busy"],false);
    assert_eq!(state["currentFile"],"/locked dir/传感器.txt");
    assert!(state["message"].as_str().unwrap().contains("/locked dir/传感器.txt"));
    assert!(state["message"].as_str().unwrap().contains(&f.profile.username));
    assert_eq!(std::fs::read(&file)?,b"keep on denied deletion");
    assert_eq!(f.counts.files.removed.load(Ordering::SeqCst),0);
    f.counts.files.deny_removals.store(false,Ordering::SeqCst);
    let state=s.files.remove_remote(serde_json::from_value(removal)?,confirm(true)).await?;
    assert_eq!(state["phase"],"completed");assert!(!directory.exists());
    s.shutdown().await;
    until("permission sample SFTP sessions closed",||f.counts.files.active.load(Ordering::SeqCst)==0).await;
    Ok(())
}
#[tokio::test]
async fn native_files_use_shared_sftp_for_unicode_recursive_transfer_overwrite_and_delete(
) -> Result<()> {
    let (f, s, root) = fixture().await?;
    std::fs::create_dir(root.join("local/中文 目录"))?;
    std::fs::write(root.join("local/中文 目录/设置.txt"), "原始中文🙂")?;
    let listing = s.files.list(Side::Local, String::new()).await?;
    assert_eq!(listing["entries"][0]["name"], "中文 目录");
    let state = s
        .files
        .transfer(
            serde_json::from_value(options(&s, &root, "upload", vec!["中文 目录"], 0))?,
            confirm(true),
        )
        .await?;
    assert_eq!(state["phase"], "completed");
    assert_eq!(
        std::fs::read(root.join("remote/中文 目录/设置.txt"))?,
        "原始中文🙂".as_bytes()
    );
    let remote = s.files.list(Side::Remote, "/中文 目录".into()).await?;
    assert_eq!(remote["entries"][0]["name"], "设置.txt");
    s.files
        .mkdir_for_generation(
            Side::Remote,
            "/".into(),
            "空目录".into(),
            Some(s.devices.generation()),
        )
        .await?;
    let mut upload = options(&s, &root, "upload", vec!["设置.txt"], 0);
    upload["localPath"] = json!(root.join("local/中文 目录"));
    upload["remotePath"] = json!("/中文 目录");
    std::fs::write(root.join("local/中文 目录/设置.txt"), "更新内容")?;
    let state = s
        .files
        .transfer(serde_json::from_value(upload.clone())?, confirm(false))
        .await?;
    assert_eq!(state["phase"], "canceled");
    assert_eq!(
        std::fs::read(root.join("remote/中文 目录/设置.txt"))?,
        "原始中文🙂".as_bytes()
    );
    let state = s
        .files
        .transfer(serde_json::from_value(upload)?, confirm(true))
        .await?;
    assert_eq!(state["phase"], "completed");
    assert_eq!(
        std::fs::read(root.join("remote/中文 目录/设置.txt"))?,
        "更新内容".as_bytes()
    );
    std::fs::create_dir(root.join("downloads"))?;
    let mut download = options(&s, &root, "download", vec!["中文 目录", "空目录"], 0);
    download["localPath"] = json!(root.join("downloads"));
    let state = s
        .files
        .transfer(serde_json::from_value(download)?, confirm(true))
        .await?;
    assert_eq!(state["phase"], "completed");
    assert_eq!(
        std::fs::read(root.join("downloads/中文 目录/设置.txt"))?,
        "更新内容".as_bytes()
    );
    assert!(root.join("downloads/空目录").is_dir());
    let removal =
        json!({"generation":s.devices.generation(),"parent":"/","names":["中文 目录","空目录"]});
    let state = s
        .files
        .remove_remote(serde_json::from_value(removal.clone())?, confirm(false))
        .await?;
    assert_eq!(state["phase"], "canceled");
    assert!(root.join("remote/中文 目录").exists());
    let state = s
        .files
        .remove_remote(serde_json::from_value(removal)?, confirm(true))
        .await?;
    assert_eq!(state["phase"], "completed");
    assert!(std::fs::read_dir(root.join("remote"))?.next().is_none());
    assert_eq!(s.devices.snapshot()["ssh"], "connected");
    s.shutdown().await;
    until("SFTP released", || {
        f.counts.files.active.load(Ordering::SeqCst) == 0
    })
    .await;
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&f.report())?,
    )?;
    Ok(())
}
#[tokio::test]
async fn stale_device_operations_cancel_and_disconnect_preserve_targets_and_remove_temporaries(
) -> Result<()> {
    let (f, s, root) = fixture().await?;
    std::fs::write(root.join("local/large.bin"), vec![42; 8 * 1024 * 1024])?;
    let old_generation = s.devices.generation();
    s.disconnect().await;
    connect(&s, &f).await?;
    let mut stale = options(&s, &root, "upload", vec!["large.bin"], 0);
    stale["generation"] = json!(old_generation);
    assert!(s
        .files
        .transfer(serde_json::from_value(stale)?, confirm(true))
        .await
        .is_err());
    assert!(s
        .files
        .mkdir_for_generation(
            Side::Remote,
            "/".into(),
            "stale".into(),
            Some(old_generation)
        )
        .await
        .is_err());
    assert!(!root.join("remote/stale").exists());
    for disconnect in [false, true] {
        let service = s.clone();
        let op = options(&s, &root, "upload", vec!["large.bin"], 1048576);
        let job = tokio::spawn(async move {
            service
                .files
                .transfer(serde_json::from_value(op)?, confirm(true))
                .await
        });
        until("bounded transfer progress", || {
            let state = s.files.snapshot();
            state["busy"] == true
                && state["phase"] == "transferring"
                && state["bytes"].as_u64().unwrap_or(0) > 0
        })
        .await;
        if disconnect {
            s.disconnect().await;
        } else {
            s.files.cancel();
        }
        let state = tokio::time::timeout(Duration::from_secs(15), job).await???;
        assert_eq!(
            state["phase"],
            if disconnect { "failed" } else { "canceled" }
        );
        assert!(!root.join("remote/large.bin").exists());
        if disconnect {
            connect(&s, &f).await?;
            s.files.list(Side::Remote, "/".into()).await?;
        }
        assert_eq!(temps(&root.join("remote")), 0);
    }
    s.shutdown().await;
    until("all SFTP sessions closed", || {
        f.counts.files.active.load(Ordering::SeqCst) == 0
    })
    .await;
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&f.report())?,
    )?;
    Ok(())
}

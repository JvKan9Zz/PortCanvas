#[path = "support/terminal_fixture.rs"]
mod support;
use anyhow::Result;
use portcanvas_core::services::{Paths, Services};
use portcanvas_native_probe::camera_client;
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
fn install_arena(services: &Services) -> Result<()> {
    if let Some(path) = std::env::var_os("PORTCANVAS_TEST_ARENA_PACKAGE") {
        services.plugins.import(&PathBuf::from(path), &CancellationToken::new())?;
        return Ok(());
    }
    let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(), flate2::Compression::fast(),
    ));
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/arena-monitor");
    for name in ["manifest.json", "assets/arena-platform.png", "config/arena.json",
                 "scripts/camera_list.py", "scripts/camera_stream.py", "scripts/pose_stream.py"] {
        archive.append_path_with_name(source.join(name), name)?;
    }
    let bytes = archive.into_inner()?.finish()?;
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap());
    let path = root.join(format!("arena-fixture-{}.pcplugin", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes)?;
    let result = services.plugins.import(&path, &CancellationToken::new());
    std::fs::remove_file(path)?;
    result?;
    Ok(())
}
async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
async fn connected() -> Result<(support::Fixture, Arc<Services>)> {
    let f = support::Fixture::new().await?;
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap())
        .join(format!("arena-{}", uuid::Uuid::new_v4()));
    let s = Services::new(
        Paths {
            plugins: None,
            data: root.join("data"),
            documents: root.clone(),
            local_files: root,
            portable: true,
        },
        Arc::new(|_, _| {}),
        Some("arena-tests".into()),
    )?;
    let key = f.fingerprint.clone();
    s.connect(
        f.profile.clone(),
        zeroize::Zeroizing::new("synthetic-only".into()),
        Arc::new(move |identity, _| {
            let accepted = identity.fingerprint == key;
            Box::pin(async move { Ok(accepted) })
        }),
    )
    .await?;
    Ok((f, s))
}
#[tokio::test]
async fn camera_and_pose_keep_the_shared_ssh_connection_after_sftp_browsing() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(format!("arena-files-{}",uuid::Uuid::new_v4()));
    let remote = root.join("remote");
    std::fs::create_dir_all(&remote)?;
    std::fs::write(remote.join("owned.txt"), "owned fixture")?;
    let f = support::Fixture::with_files(remote).await?;
    let s = Services::new(Paths{plugins:None,data:root.join("data"),documents:root.clone(),local_files:root,portable:true},Arc::new(|_,_|{}),Some("arena-files-test".into()))?;
    install_arena(&s)?;
    let fingerprint=f.fingerprint.clone();
    s.connect(f.profile.clone(),zeroize::Zeroizing::new("synthetic-only".into()),Arc::new(move|identity,_|{let accepted=identity.fingerprint==fingerprint;Box::pin(async move{Ok(accepted)})})).await?;
    let sftp=s.devices.connection()?.sftp().await?;
    assert!(sftp.read_dir("/").await?.into_iter().next().is_some());
    let generation=s.devices.generation();
    let stop=CancellationToken::new();
    let pose=s.pose.start_for_generation(generation,stop.clone()).await?;
    let (frames,receiver)=watch::channel(None);
    let task=tokio::spawn(camera_client::run(s.clone(),generation,"/dev/video0".into(),20,frames,stop.clone()));
    until(||receiver.borrow().is_some()).await;
    // Include several five-second SSH keepalive periods, with the SFTP channel retained.
    for _ in 0..55 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(s.devices.snapshot()["ssh"],"connected");
        assert!(!task.is_finished(),"camera task ended while SSH was still connected");
        s.arena.metrics().await?;
    }
    stop.cancel();
    task.await??;
    s.pose.stop(pose["id"].as_u64()).await;
    sftp.close().await?;
    s.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn native_arena_reuses_ssh_without_http_and_cleans_camera_and_pose() -> Result<()> {
    let (f, s) = connected().await?;
    install_arena(&s)?;
    let generation = s.devices.generation();
    assert_eq!(s.arena.cameras().await?[0]["path"], "/dev/video0");
    assert!(s.arena.metrics().await?["cpuTotal"].as_u64().unwrap() > 0);
    let pose = s
        .pose
        .start_for_generation(generation, CancellationToken::new())
        .await?;
    until(|| s.pose.snapshot()["phase"] == "unavailable").await;
    for (mode, phase) in [
        (1, "live"),
        (2, "stale"),
        (3, "invalid"),
        (4, "unavailable"),
    ] {
        f.counts.arena.pose_mode.store(mode, Ordering::SeqCst);
        until(|| s.pose.snapshot()["phase"] == phase).await;
    }
    let state = s
        .arena
        .start_native(
            "/dev/video0".into(),
            20,
            generation,
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(state["streamUrl"], "");
    assert_eq!(state["transport"], "native-jpeg");
    let id = state["id"].as_u64().unwrap();
    until(|| s.arena.status()["phase"] == "connected").await;
    let frame = s.arena.native_frame(id, 0, generation)?.unwrap();
    assert_eq!(
        camera_client::decode(frame.sequence, &frame.jpeg)?.width,
        64
    );
    let canceled = CancellationToken::new();
    canceled.cancel();
    assert!(s
        .arena
        .start_native("/dev/video0".into(), 20, generation, canceled)
        .await
        .is_err());
    assert!(s
        .arena
        .start_native(
            "/dev/video0".into(),
            20,
            generation + 1,
            CancellationToken::new()
        )
        .await
        .is_err());
    assert_eq!(s.arena.status()["id"], id);
    assert!(s
        .pose
        .start_for_generation(generation + 1, CancellationToken::new())
        .await
        .is_err());
    assert_eq!(s.pose.snapshot()["id"], pose["id"]);
    s.arena.stop(Some(id)).await;
    s.pose.stop(pose["id"].as_u64()).await;
    until(|| {
        f.counts.arena.cameras.load(Ordering::SeqCst) == 0
            && f.counts.arena.poses.load(Ordering::SeqCst) == 0
    })
    .await;
    assert_eq!(s.devices.snapshot()["ssh"], "connected");
    s.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn camera_decoder_cancel_error_and_restart_keep_resource_ownership() -> Result<()> {
    let (f, s) = connected().await?;
    install_arena(&s)?;
    let generation = s.devices.generation();
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let (tx, rx) = watch::channel(None);
    let service = s.clone();
    let job = tokio::spawn(async move {
        camera_client::run(service, generation, "/dev/video0".into(), 20, tx, token).await
    });
    until(|| rx.borrow().is_some()).await;
    cancel.cancel();
    job.await??;
    until(|| f.counts.arena.cameras.load(Ordering::SeqCst) == 0).await;
    f.counts.arena.bad_jpeg.store(true, Ordering::SeqCst);
    let (tx, _) = watch::channel(None);
    assert!(camera_client::run(
        s.clone(),
        generation,
        "/dev/video0".into(),
        20,
        tx,
        CancellationToken::new()
    )
    .await
    .is_err());
    until(|| f.counts.arena.cameras.load(Ordering::SeqCst) == 0).await;
    f.counts.arena.bad_jpeg.store(false, Ordering::SeqCst);
    let state = s.arena.start_native("/dev/video0".into(), 20, generation, CancellationToken::new()).await?;
    assert_eq!(state["transport"], "native-jpeg");
    assert_eq!(state["streamUrl"], "");
    let id = state["id"].as_u64().unwrap();
    until(|| s.arena.status()["phase"] == "connected").await;
    assert!(s.arena.native_frame(id, 0, generation)?.is_some());
    s.arena.stop(Some(id)).await;
    until(|| f.counts.arena.cameras.load(Ordering::SeqCst) == 0).await;
    s.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn camera_and_pose_require_an_enabled_plugin_and_stop_when_it_is_removed() -> Result<()> {
    use portcanvas_core::plugins::ARENA_ID;
    let (fixture, services) = connected().await?;
    let generation = services.devices.generation();
    let cancel = CancellationToken::new();
    let start_camera = || services.arena.start_native("/dev/video0".into(), 20, generation, cancel.clone());
    assert!(services.arena.cameras().await.unwrap_err().to_string().contains("尚未安装"));
    assert!(start_camera().await.unwrap_err().to_string().contains("尚未安装"));
    assert!(services.pose.start().await.unwrap_err().to_string().contains("尚未安装"));
    assert!(services.arena.metrics().await?["cpuTotal"].as_u64().is_some());

    install_arena(&services)?;
    start_camera().await?;
    services.pose.start().await?;
    until(|| fixture.counts.arena.cameras.load(Ordering::SeqCst) == 1
        && fixture.counts.arena.poses.load(Ordering::SeqCst) == 1).await;

    services.plugins.set_enabled(ARENA_ID, false)?;
    until(|| fixture.counts.arena.cameras.load(Ordering::SeqCst) == 0
        && fixture.counts.arena.poses.load(Ordering::SeqCst) == 0).await;
    assert!(start_camera().await.unwrap_err().to_string().contains("已停用"));
    assert!(services.pose.start().await.unwrap_err().to_string().contains("已停用"));

    services.plugins.set_enabled(ARENA_ID, true)?;
    services.pose.start().await?;
    until(|| fixture.counts.arena.poses.load(Ordering::SeqCst) == 1).await;
    services.plugins.uninstall(ARENA_ID)?;
    until(|| fixture.counts.arena.poses.load(Ordering::SeqCst) == 0).await;
    assert!(services.plugins.installed()?.is_empty());
    services.shutdown().await;
    Ok(())
}


#[tokio::test]
async fn cancelling_an_inflight_monitor_sample_closes_only_its_ssh_channel() -> Result<()> {
    let (fixture, services) = connected().await?;
    let generation = services.devices.generation();
    fixture.counts.arena.hold_metrics.store(true, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let task_services = services.clone();
    let request = tokio::spawn(async move {
        task_services.arena.metrics_for_generation(generation, &task_cancel).await
    });
    until(|| fixture.counts.arena.metrics_pending.load(Ordering::SeqCst) == 1).await;
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_secs(2), request).await??
        .unwrap_err().to_string();
    assert!(error.contains("已取消"));
    until(|| fixture.counts.arena.metrics_pending.load(Ordering::SeqCst) == 0).await;
    fixture.counts.arena.hold_metrics.store(false, Ordering::SeqCst);
    assert!(services.arena.metrics().await?["cpuTotal"].is_number());
    assert_eq!(services.devices.snapshot()["ssh"], "connected");
    services.shutdown().await;
    Ok(())
}


#[tokio::test]
async fn vision_package_stream_hot_updates_and_stops_when_disabled() -> Result<()> {
    use portcanvas_core::plugins::VISION_ID;
    use serde_json::json;
    let (fixture, services) = connected().await?;
    let generation = services.devices.generation();
    let cancel = CancellationToken::new();
    assert!(services.vision.inspect_for_generation(generation, &cancel).await.is_err());

    let package = if let Some(path) = std::env::var_os("PORTCANVAS_TEST_VISION_PACKAGE") {
        PathBuf::from(path)
    } else {
        let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(), flate2::Compression::fast(),
        ));
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/vision-studio");
        for name in ["manifest.json", "scripts/vision_inspect.py", "scripts/vision_stream.py"] {
            archive.append_path_with_name(source.join(name), name)?;
        }
        let bytes = archive.into_inner()?.finish()?;
        let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap());
        let path = root.join(format!("vision-fixture-{}.pcplugin", uuid::Uuid::new_v4()));
        std::fs::write(&path, bytes)?;
        path
    };
    let installed = services.plugins.import(&package, &cancel)?;
    assert_eq!(installed.manifest.id, VISION_ID);
    let info = services.vision.inspect_for_generation(generation, &cancel).await?;
    assert_eq!(info["sharedFrame"]["available"], true);
    assert!(services.vision.inspect_for_generation(generation + 1, &cancel).await.is_err());

    let initial = json!({"source":"watch","algo":0});
    let id = services.vision.start(initial, generation, cancel.clone()).await?;
    until(|| services.vision.status()["phase"] == "connected").await;
    let frame = services.vision.native_frame(id, 0, generation)?.unwrap();
    let image = camera_client::decode(frame.sequence, &frame.jpeg)?;
    assert_eq!((image.width, image.height), (64, 36));
    assert_eq!(services.vision.status()["telemetry"]["activeSource"], "shared");
    let update = json!({"source":"watch","algo":3,"hLow":42});
    services.vision.update_config(update.clone())?;
    until(|| fixture.counts.arena.vision_configs.lock().unwrap().contains(&update)).await;
    assert_eq!(services.vision.status()["id"], id);
    until(|| services.vision.native_frame(id, frame.sequence, generation).is_ok_and(|f| f.is_some())).await;

    services.plugins.set_enabled(VISION_ID, false)?;
    until(|| fixture.counts.arena.vision_streams.load(Ordering::SeqCst) == 0).await;
    assert!(services.vision.start(update, generation, cancel.clone()).await.is_err());
    assert_eq!(services.devices.snapshot()["ssh"], "connected");
    services.shutdown().await;
    Ok(())
}

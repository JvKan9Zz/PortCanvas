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
    let s = Services::new(Paths{data:root.join("data"),documents:root.clone(),local_files:root,portable:true},Arc::new(|_,_|{}),Some("arena-files-test".into()))?;
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

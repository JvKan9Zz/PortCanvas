#[path = "support/terminal_fixture.rs"]
mod support;
use anyhow::Result;
use portcanvas_core::services::{Paths, Services};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};
async fn connect(service: &Services, fixture: &support::Fixture) -> Result<()> {
    let fp = fixture.fingerprint.clone();
    service
        .connect(
            fixture.profile.clone(),
            zeroize::Zeroizing::new("synthetic-only".into()),
            Arc::new(move |host, _| {
                let matches = host.fingerprint == fp;
                Box::pin(async move { Ok(matches) })
            }),
        )
        .await?;
    Ok(())
}
#[tokio::test]
async fn confirmed_reboot_rejects_stale_busy_and_duplicate_requests_and_recovers_after_failure(
) -> Result<()> {
    let fixture = support::Fixture::new().await?;
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap())
        .join(format!("reboot-{}", uuid::Uuid::new_v4()));
    let s = Services::new(
        Paths {
            data: root.join("data"),
            documents: root.join("docs"),
            local_files: root.clone(),
            portable: true,
        },
        Arc::new(|_, _| {}),
        Some("owned-reboot-fixture".into()),
    )?;
    assert!(s
        .reboot(|_| async { panic!("disconnected request must not confirm") })
        .await
        .is_err());
    assert!(!s.rebooting.load(Ordering::SeqCst));
    connect(&s, &fixture).await?;
    let result = s
        .reboot(|target| async move {
            assert!(target.contains("fixture@127.0.0.1"));
            Ok(false)
        })
        .await?;
    assert_eq!(result["canceled"], true);
    assert_eq!(fixture.counts.reboots.load(Ordering::SeqCst), 0);
    s.migrating.store(true, Ordering::SeqCst);
    assert!(s
        .reboot(|_| async { panic!("busy request must not confirm") })
        .await
        .is_err());
    s.migrating.store(false, Ordering::SeqCst);
    assert!(s
        .reboot(|_| async {
            s.migrating.store(true, Ordering::SeqCst);
            Ok(true)
        })
        .await
        .is_err());
    s.migrating.store(false, Ordering::SeqCst);
    assert!(!s.rebooting.load(Ordering::SeqCst));
    let failure = s
        .reboot(|_| async { anyhow::bail!("synthetic confirmation failure") })
        .await
        .unwrap_err();
    assert!(failure
        .to_string()
        .contains("synthetic confirmation failure"));
    assert!(!s.rebooting.load(Ordering::SeqCst));
    let (ready, rx) = tokio::sync::oneshot::channel();
    let owned = s.clone();
    let first = tokio::spawn(async move {
        owned
            .reboot(|_| async {
                let _ = ready.send(());
                std::future::pending::<Result<bool>>().await
            })
            .await
    });
    rx.await?;
    assert!(s
        .reboot(|_| async { panic!("duplicate request must not confirm") })
        .await
        .is_err());
    assert!(s.rebooting.load(Ordering::SeqCst));
    first.abort();
    let _ = first.await;
    assert!(!s.rebooting.load(Ordering::SeqCst));
    assert!(s
        .reboot(|_| async {
            s.disconnect().await;
            Ok(true)
        })
        .await
        .is_err());
    assert_eq!(fixture.counts.reboots.load(Ordering::SeqCst), 0);
    connect(&s, &fixture).await?;
    fixture.counts.reboot_status.store(1, Ordering::SeqCst);
    assert!(s.reboot(|_| async { Ok(true) }).await.is_err());
    assert_eq!(s.devices.snapshot()["ssh"], "connected");
    assert!(!s.rebooting.load(Ordering::SeqCst));
    fixture.counts.reboot_status.store(0, Ordering::SeqCst);
    assert_eq!(s.reboot(|_| async { Ok(true) }).await?["requested"], true);
    assert_eq!(fixture.counts.reboots.load(Ordering::SeqCst), 2);
    assert_eq!(s.devices.snapshot()["ssh"], "disconnected");
    assert!(!s.rebooting.load(Ordering::SeqCst));
    s.shutdown().await;
    drop(s);
    std::fs::remove_dir_all(root)?;
    Ok(())
}

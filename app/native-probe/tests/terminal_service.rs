#[path = "support/terminal_fixture.rs"]
mod support;
use anyhow::Result;
use portcanvas_core::{
    services::{Paths, Services},
    terminal::TerminalService,
};
use portcanvas_native_probe::terminal::Terminal;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

async fn ready(service: &TerminalService, buffer: Arc<Mutex<Terminal>>) -> Result<u64> {
    let ack = service.clone();
    let state = service
        .start(
            80,
            24,
            Arc::new(move |frame| {
                let id = u64::from_le_bytes(frame[..8].try_into()?);
                let seq = u64::from_le_bytes(frame[8..16].try_into()?);
                buffer.lock().unwrap().feed(&frame[16..]);
                ack.ack(id, seq);
                Ok(())
            }),
        )
        .await?;
    Ok(state["id"].as_u64().unwrap())
}
async fn until(label: &str, mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out: {label}"));
}

#[tokio::test]
async fn native_vt_over_shared_ssh_preserves_tabs_modes_resize_and_reconnect() -> Result<()> {
    let fixture = support::Fixture::new().await?;
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap())
        .join(format!("terminal-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root)?;
    let services = Services::new(
        Paths {
            data: root.join("data"),
            documents: root.clone(),
            local_files: root.clone(),
            portable: true,
        },
        Arc::new(|_, _| {}),
        Some("native-terminal-tests".into()),
    )?;
    let fingerprint = fixture.fingerprint.clone();
    services
        .connect(
            fixture.profile.clone(),
            zeroize::Zeroizing::new("synthetic-only".into()),
            Arc::new(move |identity, _| {
                let matches = identity.fingerprint == fingerprint;
                Box::pin(async move { Ok(matches) })
            }),
        )
        .await?;
    let device = services.terminal.list()?["deviceKey"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(services
        .logs
        .set_project_for_device("main", "wrong device".into(), Some("replaced-device"))
        .await
        .is_err());
    assert_eq!(services.logs.terminal_snapshot("main")["project"], "");
    services
        .logs
        .set_project_for_device("main", "设置".into(), Some(&device))
        .await?;
    let one = services.terminal.get("main", Some(&device))?;
    let key = services.terminal.create(Some(&device))?["key"]
        .as_str()
        .unwrap()
        .to_string();
    let two = services.terminal.get(&key, Some(&device))?;
    let a = Arc::new(Mutex::new(Terminal::new(80, 24)));
    let b = Arc::new(Mutex::new(Terminal::new(80, 24)));
    let id = tokio::time::timeout(Duration::from_secs(10), ready(&one, a.clone())).await??;
    let id2 = ready(&two, b.clone()).await?;
    assert_eq!(services.logs.terminal_snapshot("main")["phase"], "ready");
    assert_eq!(services.logs.terminal_snapshot(&key)["phase"], "ready");
    until("initial CJK", || {
        a.lock().unwrap().text().contains("中文宽字符")
    })
    .await;
    one.write(id, "FIRST-中\r".into()).await?;
    two.write(id2, "SECOND-🙂\r".into()).await?;
    until("isolated input echo", || {
        a.lock().unwrap().text().contains("FIRST-中")
            && b.lock().unwrap().text().contains("SECOND-🙂")
    })
    .await;
    assert!(!a.lock().unwrap().text().contains("SECOND"));
    assert!(!b.lock().unwrap().text().contains("FIRST"));
    one.resize(id, 110, 32).await?;
    until("PTY resize", || {
        fixture.counts.sizes.lock().unwrap().contains(&(110, 32))
    })
    .await;
    one.write(id, "fixture-modes\r".into()).await?;
    until("application cursor mode", || {
        a.lock().unwrap().arrow('A') == b"\x1bOA"
    })
    .await;
    assert_eq!(
        a.lock().unwrap().paste("粘贴"),
        "\x1b[200~粘贴\x1b[201~".as_bytes()
    );
    one.write(id, "fixture-lines\r".into()).await?;
    until("long output", || {
        a.lock().unwrap().text().contains("line-1399")
    })
    .await;
    let before = fixture
        .counts
        .input
        .lock()
        .unwrap()
        .values()
        .map(Vec::len)
        .sum::<usize>();
    a.lock().unwrap().scroll(10);
    assert!(!a.lock().unwrap().text().contains("line-1399"));
    assert_eq!(
        fixture
            .counts
            .input
            .lock()
            .unwrap()
            .values()
            .map(Vec::len)
            .sum::<usize>(),
        before
    );
    one.stop();
    assert!(one.write(id, "stale".into()).await.is_err());
    assert_eq!(two.snapshot()["phase"], "connected");
    until("stopped PTY EOF", || {
        fixture
            .counts
            .terminal_eof
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 1
    })
    .await;
    let id3 = ready(&one, a.clone()).await?;
    assert_ne!(id, id3);
    assert!(fixture
        .counts
        .sessions
        .lock()
        .unwrap()
        .values()
        .all(|v| *v == 1));
    services.terminal.remove(&key, Some(&device)).await?;
    assert_eq!(
        services.terminal.list()?["tabs"].as_array().unwrap().len(),
        1
    );
    assert_eq!(one.snapshot()["phase"], "connected");
    assert_eq!(services.logs.terminal_snapshot("main")["phase"], "ready");
    one.reset(false).await?;
    ready(&one, a.clone()).await?;
    assert!(fixture
        .counts
        .sessions
        .lock()
        .unwrap()
        .values()
        .any(|v| *v == 2));
    services.disconnect().await;
    assert!(one.write(id3, "late".into()).await.is_err());
    services.shutdown().await;
    until("all owned PTYs released", || {
        fixture
            .counts
            .active
            .load(std::sync::atomic::Ordering::SeqCst)
            == 0
    })
    .await;
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&fixture.report())?,
    )?;
    Ok(())
}

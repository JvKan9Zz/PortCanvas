use super::*;
use crate::plugins::tests::{manifest, package, temp};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn entry(payload: &[u8], url: &str) -> Value {
    let mut item = serde_json::to_value(manifest()).unwrap();
    item["downloadUrl"] = json!(url);
    item["sizeBytes"] = json!(payload.len());
    item["sha256"] = json!(format!("{:x}", Sha256::digest(payload)));
    item
}

async fn fixture(payload: Vec<u8>) -> Result<(CatalogClient, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let item = entry(&payload, &format!("{base}/package"));
    let catalog = serde_json::to_vec(&json!({"schema": SCHEMA, "plugins": [item]}))?;
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let Ok(byte) = socket.read_u8().await else {
                    break;
                };
                request.push(byte);
            }
            let body = if request.starts_with(b"GET /catalog ") {
                &catalog
            } else {
                &payload
            };
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(headers.as_bytes()).await;
            let _ = socket.write_all(body).await;
        }
    });
    Ok((
        CatalogClient::new(&format!("{base}/catalog"), true, "")?,
        task,
    ))
}

#[tokio::test]
async fn loopback_catalog_downloads_reports_progress_and_installs_only_verified_payloads(
) -> Result<()> {
    let dir = temp();
    let store = PluginStore::new(dir.path());
    let built_package = std::env::var_os("PORTCANVAS_TEST_ARENA_PACKAGE");
    let bytes = match &built_package {
        Some(path) => fs::read(path)?,
        None => package(&manifest(), "camera"),
    };
    let (client, fixture) = fixture(bytes.clone()).await?;
    let cancel = CancellationToken::new();
    let catalog = client.refresh(&cancel).await?;
    let item = &catalog.plugins[0];
    let mut invalid = item.clone();
    invalid.sha256 = "0".repeat(64);
    assert!(client
        .download_install(&store, &invalid, &cancel, |_, _, _| {})
        .await
        .unwrap_err()
        .to_string()
        .contains("SHA-256"));
    let progress = std::sync::atomic::AtomicUsize::new(0);
    let installed = client
        .download_install(&store, item, &cancel, |chunk, total, _| {
            assert_eq!(total, Some(item.size_bytes));
            progress.fetch_add(chunk, std::sync::atomic::Ordering::Relaxed);
        })
        .await?;
    assert_eq!(progress.into_inner(), bytes.len());
    assert!(installed.official());
    let script = store.plugin_script(ARENA_ID, "cameraList")?.0;
    if built_package.is_some() {
        assert_eq!(
            script.replace("\r\n", "\n"),
            include_str!("../../../../plugins/arena-monitor/scripts/camera_list.py")
        );
    } else {
        assert_eq!(script, "camera");
    }
    store.uninstall(ARENA_ID)?;
    let stop = cancel.clone();
    assert!(client
        .download_install(&store, item, &cancel, move |_, _, _| stop.cancel())
        .await
        .is_err());
    assert!(store.installed()?.is_empty());
    assert!(client.refresh(&cancel).await.is_err());
    fixture.abort();
    Ok(())
}

#[test]
fn catalog_urls_require_https_or_the_debug_http_loopback_boundary() {
    assert!(checked_url("https://example.test/catalog.json", false).is_ok());
    assert!(checked_url("http://127.0.0.1:8080/catalog.json", true).is_ok());
    for (url, loopback) in [
        ("http://127.0.0.1", false),
        ("http://example.test", true),
        ("https://example.test", true),
        ("http://user@127.0.0.1", true),
        ("https://user@example.test", false),
    ] {
        assert!(checked_url(url, loopback).is_err(), "{url}");
    }
}

#[test]
fn optional_minisign_signature_is_verified_with_the_configured_key() -> Result<()> {
    let signed: Value = serde_json::from_str(include_str!(
        "../../../native-probe/tests/fixtures/update.json"
    ))?;
    let bytes = signed["payload"].as_str().unwrap().as_bytes();
    let mut entry: CatalogEntry = serde_json::from_value(entry(bytes, "https://example.test"))?;
    entry.signature = Some(signed["signature"].as_str().unwrap().into());
    let key = signed["publicKey"].as_str().unwrap();
    let pkg = entry.current_package().unwrap();
    verify_package(bytes, &pkg, key)?;
    assert!(verify_package(bytes, &pkg, "invalid").is_err());
    let other = b"other";
    let mut bad_pkg = pkg;
    bad_pkg.size_bytes = other.len() as u64;
    bad_pkg.sha256 = format!("{:x}", Sha256::digest(other));
    assert!(verify_package(other, &bad_pkg, key).is_err());
    Ok(())
}

#[test]
fn multi_platform_catalog_selects_matching_package_only() -> Result<()> {
    let win_pkg = CatalogPackage {
        download_url: "https://example.test/win.pcplugin".into(),
        size_bytes: 123,
        sha256: "a".repeat(64),
        signature: None,
    };
    let mac_pkg = CatalogPackage {
        download_url: "https://example.test/mac.pcplugin".into(),
        size_bytes: 456,
        sha256: "b".repeat(64),
        signature: None,
    };
    let mut item = serde_json::to_value(manifest())?;
    item["packages"] = json!({
        "windows-x86_64": win_pkg,
        "darwin-universal": mac_pkg,
    });
    let entry: CatalogEntry = serde_json::from_value(item)?;
    assert_eq!(
        entry.package_for_platform("windows-x86_64").unwrap().size_bytes,
        123
    );
    assert_eq!(
        entry.package_for_platform("darwin-universal").unwrap().size_bytes,
        456
    );
    assert!(entry.package_for_platform("linux-x86_64").is_none());
    Ok(())
}

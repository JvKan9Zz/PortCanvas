//! End-to-end verification that installing `fast-downloader` replaces the built-in
//! 32-thread downloader with the plugin's 128-thread worker, and uninstalling restores 32 threads.
#![cfg(feature = "downloads")]

use anyhow::Result;
use flate2::{write::GzEncoder, Compression};
use portcanvas_core::{
    download::{active_downloader_summary, client_builder, fetch},
    plugins::{current_platform, Manifest, PluginStore, SCHEMA, SURFACE_DECLARATIVE_V1},
};
use serde_json::json;
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

async fn spawn_range_server(payload: Vec<u8>) -> (reqwest::Url, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/pkg.bin", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let body = Arc::new(payload);
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let body = body.clone();
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut b = [0u8; 1];
                while !req.ends_with(b"\r\n\r\n") {
                    if sock.read_exact(&mut b).await.is_err() {
                        return;
                    }
                    req.push(b[0]);
                }
                let text = String::from_utf8_lossy(&req).to_lowercase();
                let range = text
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|v| v.split_once('-'))
                    .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)));
                if let Some((s, e)) = range {
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {s}-{e}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len(),
                        e - s + 1
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body[s..=e]).await;
                } else {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                }
            });
        }
    });
    (url, handle)
}

fn pack_downloader_plugin(worker_bin: &Path) -> Result<Vec<u8>> {
    let bin_name = worker_bin.file_name().unwrap().to_str().unwrap();
    let entry_path = format!("bin/{bin_name}");
    let manifest: Manifest = serde_json::from_value(json!({
        "schema": SCHEMA,
        "id": "fast-downloader",
        "name": "128线程下载器",
        "nameEn": "128-Thread Downloader",
        "version": "0.1.0",
        "summary": "128-thread downloader",
        "category": "tools",
        "author": "PortCanvas",
        "official": true,
        "minAppVersion": "0.7.2",
        "hostSurface": SURFACE_DECLARATIVE_V1,
        "platform": current_platform(),
        "requiresDevice": false,
        "lifecycle": "background",
        "ui": "ui.json",
        "provides": ["downloader"],
        "runtime": {
            "kind": "local-worker",
            "entry": entry_path,
            "args": ["--worker"],
            "frameChannel": false
        },
        "permissions": ["local.exec", "local.network", "local.fs"]
    }))?;
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let ui_bytes = br#"{"layout":"stack","primary":[{"type":"task-list","title":"Queue","itemsKey":"tasks"}]}"#;
    let worker_bytes = fs::read(worker_bin)?;

    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (name, bytes, mode) in [
        ("manifest.json", manifest_bytes.as_slice(), 0o644u32),
        ("ui.json", ui_bytes.as_slice(), 0o644u32),
        (entry_path.as_str(), worker_bytes.as_slice(), 0o755u32),
    ] {
        let mut header = tar::Header::new_ustar();
        header.set_path(name)?;
        header.set_mode(mode);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        archive.append(&header, bytes)?;
    }
    Ok(archive.into_inner()?.finish()?)
}

#[tokio::test]
async fn installing_downloader_plugin_replaces_builtin_32_and_uninstalling_restores_it() -> Result<()> {
    let payload: Vec<u8> = (0..262_147).map(|i| (i % 251) as u8).collect();
    let (url, server) = spawn_range_server(payload.clone()).await;
    let client = client_builder().no_proxy().build()?;

    // 1. Default built-in 32-thread engine
    assert_eq!(active_downloader_summary(), "builtin-32");
    let max_conns = Arc::new(AtomicUsize::new(0));
    let mc = max_conns.clone();
    let bytes = fetch(&client, &url, move |_, _, conns| {
        mc.fetch_max(conns, Ordering::Relaxed);
    })
    .await?;
    assert_eq!(bytes, payload);
    assert_eq!(max_conns.load(Ordering::Relaxed), 32);

    // 2. If compiled worker binary is supplied, verify live 128-thread takeover & uninstall rollback
    if let Some(worker_path) = std::env::var_os("PORTCANVAS_TEST_DOWNLOADER_WORKER") {
        let worker_path = Path::new(&worker_path);
        let root = std::env::var_os("WCS_TEST_ROOT").expect("WCS_TEST_ROOT");
        let dir = tempfile::tempdir_in(root)?;
        let store = PluginStore::new(dir.path()).with_global_downloader_sync(true);
        let pkg_file = dir.path().join("fast-downloader.pcplugin");
        fs::write(&pkg_file, pack_downloader_plugin(worker_path)?)?;

        let cancel = CancellationToken::new();
        store.import(&pkg_file, &cancel)?;
        assert_eq!(active_downloader_summary(), "plugin-128");

        let max_128 = Arc::new(AtomicUsize::new(0));
        let m128 = max_128.clone();
        let bytes_128 = fetch(&client, &url, move |_, _, conns| {
            m128.fetch_max(conns, Ordering::Relaxed);
        })
        .await?;
        assert_eq!(bytes_128, payload);
        assert_eq!(max_128.load(Ordering::Relaxed), 128);

        // 3. Uninstall restores built-in 32-thread engine immediately
        store.uninstall("fast-downloader")?;
        assert_eq!(active_downloader_summary(), "builtin-32");
        let max_restored = Arc::new(AtomicUsize::new(0));
        let mr = max_restored.clone();
        let bytes_restored = fetch(&client, &url, move |_, _, conns| {
            mr.fetch_max(conns, Ordering::Relaxed);
        })
        .await?;
        assert_eq!(bytes_restored, payload);
        assert_eq!(max_restored.load(Ordering::Relaxed), 32);
    }

    server.abort();
    Ok(())
}

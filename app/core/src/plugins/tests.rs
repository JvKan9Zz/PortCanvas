use super::*;
use flate2::{write::GzEncoder, Compression};

pub(super) fn manifest() -> Manifest {
    serde_json::from_str(include_str!(
        "../../../../plugins/arena-monitor/manifest.json"
    ))
    .unwrap()
}

pub(super) fn package(manifest: &Manifest, payload: &str) -> Vec<u8> {
    let data = serde_json::to_vec(manifest).unwrap();
    let ui_json = include_str!("../../../../plugins/arena-monitor/ui.json");
    let mut files = vec![("manifest.json", tar::EntryType::Regular, data.as_slice())];
    if let Some(ui) = &manifest.ui {
        files.push((ui.as_str(), tar::EntryType::Regular, ui_json.as_bytes()));
    }
    for path in manifest.assets.values().chain(manifest.scripts.values()) {
        files.push((path, tar::EntryType::Regular, payload.as_bytes()));
    }
    archive(&files)
}

fn archive(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (path, kind, bytes) in entries {
        let mut header = tar::Header::new_ustar();
        // Write the test path verbatim, including inputs rejected by tar's builder.
        header.as_ustar_mut().unwrap().name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_entry_type(*kind);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        archive.append(&header, *bytes).unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

pub(super) fn temp() -> TempDir {
    tempfile::tempdir_in(std::env::var_os("WCS_TEST_ROOT").expect("isolated test root")).unwrap()
}

#[tokio::test]
async fn offline_install_updates_atomically_preserves_disabled_state_and_uninstalls() -> Result<()>
{
    let dir = temp();
    let store = PluginStore::new(dir.path());
    let file = dir.path().join("arena.pcplugin");
    let mut manifest = manifest();
    let cancel = CancellationToken::new();
    assert!(store.plugin_script(ARENA_ID, "cameraList").is_err());
    fs::write(&file, package(&manifest, "old"))?;
    let item = store.import(&file, &cancel)?;
    assert!(item.enabled);
    assert!(!item.official());
    assert_eq!(store.plugin_script(ARENA_ID, "cameraList")?.0, "old");
    assert!(store.asset_path(ARENA_ID, "platformImage")?.is_file());
    let (_, session) = store.plugin_script(ARENA_ID, "cameraStream")?;
    let stream = stop_with_plugin(session, &CancellationToken::new());
    store.set_enabled(ARENA_ID, false)?;
    tokio::time::timeout(std::time::Duration::from_secs(1), stream.cancelled()).await?;
    let reopened = PluginStore::new(dir.path());
    assert!(!reopened.installed()?[0].enabled);
    assert!(store.plugin_script(ARENA_ID, "cameraStream").is_err());
    assert!(store.plugin_script(ARENA_ID, "poseStream").is_err());
    manifest.version.patch += 1;
    fs::write(&file, package(&manifest, "new"))?;
    assert!(!store.import(&file, &cancel)?.enabled);
    store.set_enabled(ARENA_ID, true)?;
    let (script, session) = store.plugin_script(ARENA_ID, "poseStream")?;
    assert_eq!(script, "new");
    store.uninstall(ARENA_ID)?;
    assert!(session.is_cancelled());
    assert!(store.installed()?.is_empty());
    assert!(!store.root.join(ARENA_ID).exists());
    Ok(())
}

#[test]
fn unsafe_paths_links_truncation_and_duplicate_entries_are_rejected() -> Result<()> {
    let dir = temp();
    let store = PluginStore::new(dir.path());
    let cancel = CancellationToken::new();
    for path in [
        "../escaped",
        "/escaped",
        "nested/../escaped",
        r"..\escaped",
        "C:/escaped",
        "nested/./file",
        "nested/NUL.txt",
        "a//file",
        "file:stream",
    ] {
        let bytes = archive(&[(path, tar::EntryType::Regular, b"test")]);
        assert!(
            store
                .install(&bytes, None, &cancel)
                .unwrap_err()
                .to_string()
                .contains("路径"),
            "{path}"
        );
    }
    for kind in [
        tar::EntryType::Symlink,
        tar::EntryType::Link,
        tar::EntryType::Fifo,
    ] {
        let bytes = archive(&[("nested/link", kind, b"")]);
        assert!(store
            .install(&bytes, None, &cancel)
            .unwrap_err()
            .to_string()
            .contains("普通文件"));
    }
    let duplicate = archive(&[
        ("same", tar::EntryType::Regular, b"a"),
        ("SAME", tar::EntryType::Regular, b"b"),
    ]);
    assert!(store
        .install(&duplicate, None, &cancel)
        .unwrap_err()
        .to_string()
        .contains("重复"));
    let mut bytes = package(&manifest(), "ok");
    bytes.pop();
    assert!(store.install(&bytes, None, &cancel).is_err());
    assert!(store.installed()?.is_empty());
    assert!(!dir.path().join("escaped").exists());
    assert_eq!(fs::read_dir(&store.root)?.count(), 0);
    Ok(())
}

#[test]
fn invalid_updates_and_cancelled_import_leave_the_previous_install_intact() -> Result<()> {
    let dir = temp();
    let store = PluginStore::new(dir.path());
    let cancel = CancellationToken::new();
    let mut manifest = manifest();
    store.install(&package(&manifest, "kept"), None, &cancel)?;
    let mut wrong = manifest.clone();
    wrong.version.patch += 1;
    assert!(store
        .install(&package(&wrong, "wrong"), Some(&manifest), &cancel)
        .is_err());
    manifest.min_app_version.major += 1;
    assert!(store
        .install(&package(&manifest, "future"), None, &cancel)
        .is_err());
    cancel.cancel();
    assert!(store
        .install(&package(&manifest, "cancelled"), None, &cancel)
        .is_err());
    assert_eq!(store.plugin_script(ARENA_ID, "cameraStream")?.0, "kept");
    Ok(())
}

#[test]
fn failure_persisting_state_restores_the_previous_directory() -> Result<()> {
    let dir = temp();
    let target = dir.path().join("installed");
    fs::create_dir(&target)?;
    fs::write(target.join("resource"), "previous")?;
    let transaction = tempfile::tempdir_in(dir.path())?;
    let staged = transaction.path().join("new");
    fs::create_dir(&staged)?;
    fs::write(staged.join("resource"), "new")?;
    assert!(
        replace_directory(transaction, &target, Some(&staged), || bail!(
            "test write failure"
        ))
        .is_err()
    );
    assert_eq!(fs::read_to_string(target.join("resource"))?, "previous");
    Ok(())
}

#[test]
fn declarative_local_worker_inspect_install_and_downloader_provider_toggle() -> Result<()> {
    let dir = temp();
    let store = PluginStore::new(dir.path()).with_data_root(dir.path().join("plugin-data"));
    let ui_json = r#"{"layout":"stack","primary":[{"type":"task-list","title":"Queue","itemsKey":"tasks"}]}"#;
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "schema": SCHEMA,
        "id": "demo-downloader",
        "name": "测试下载器",
        "nameEn": "Demo Downloader",
        "version": "0.1.0",
        "summary": "Demo",
        "category": "tools",
        "author": "Community",
        "official": false,
        "minAppVersion": "0.7.0",
        "hostSurface": SURFACE_DECLARATIVE_V1,
        "platform": current_platform(),
        "requiresDevice": false,
        "lifecycle": "background",
        "ui": "ui.json",
        "provides": ["downloader"],
        "runtime": {
            "kind": "local-worker",
            "entry": "bin/worker.bin",
            "args": ["--worker"],
            "frameChannel": false
        },
        "permissions": ["local.exec", "local.network", "local.fs"]
    }))?;
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let pkg_bytes = archive(&[
        ("manifest.json", tar::EntryType::Regular, manifest_bytes.as_slice()),
        ("ui.json", tar::EntryType::Regular, ui_json.as_bytes()),
        ("bin/worker.bin", tar::EntryType::Regular, b"fake-bin"),
    ]);
    let pkg_path = dir.path().join("demo.pcplugin");
    fs::write(&pkg_path, &pkg_bytes)?;

    // Inspect does not install anything
    let preview = store.inspect_import(&pkg_path)?;
    assert_eq!(preview.manifest.id, "demo-downloader");
    assert_eq!(preview.runtime_kind, "local-worker");
    assert!(preview.is_local_unsigned);
    assert!(preview.previous_version.is_none());
    assert!(store.installed()?.is_empty());
    assert!(store.active_downloader_plugin_id().is_none());

    // Install activates downloader provider & isolates plugin data dir
    let cancel = CancellationToken::new();
    let installed = store.import(&pkg_path, &cancel)?;
    assert!(!installed.official());
    assert!(installed.manifest.is_background());
    assert!(!installed.manifest.requires_device());
    assert_eq!(
        store.active_downloader_plugin_id().as_deref(),
        Some("demo-downloader")
    );
    let data_dir = store.data_dir("demo-downloader")?;
    fs::write(data_dir.join("prefs.json"), b"{}")?;
    let (exe, args, frame_ch, _) = store.worker_executable("demo-downloader")?;
    assert!(exe.is_file());
    assert_eq!(args, vec!["--worker"]);
    assert!(!frame_ch);

    // Disable reverts downloader provider
    store.set_enabled("demo-downloader", false)?;
    assert!(store.active_downloader_plugin_id().is_none());
    store.set_enabled("demo-downloader", true)?;
    assert_eq!(
        store.active_downloader_plugin_id().as_deref(),
        Some("demo-downloader")
    );

    // Uninstall reverts downloader provider while keeping data_dir intact
    store.uninstall("demo-downloader")?;
    assert!(store.active_downloader_plugin_id().is_none());
    assert!(data_dir.join("prefs.json").is_file());
    Ok(())
}

#[test]
fn developer_template_directory_imports_and_runs_python_worker() -> Result<()> {
    let template_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/_template");
    let manifest_bytes = fs::read(template_dir.join("manifest.json"))?;
    let ui_bytes = fs::read(template_dir.join("ui.json"))?;
    let worker_bytes = fs::read(template_dir.join("bin/worker.py"))?;

    let bytes = archive(&[
        ("manifest.json", tar::EntryType::Regular, manifest_bytes.as_slice()),
        ("ui.json", tar::EntryType::Regular, ui_bytes.as_slice()),
        ("bin/worker.py", tar::EntryType::Regular, worker_bytes.as_slice()),
    ]);
    let root = TempDir::new()?;
    let pkg = root.path().join("hello-toolbox-0.1.0.pcplugin");
    fs::write(&pkg, &bytes)?;

    let store = PluginStore::new(root.path());
    let preview = store.inspect_import(&pkg)?;
    assert_eq!(preview.manifest.id, "hello-toolbox");
    let installed = store.import(&pkg, &CancellationToken::new())?;
    assert_eq!(installed.manifest.id, "hello-toolbox");
    assert!(!installed.manifest.requires_device());
    let (exe, args, frame_ch, cancel) = store.worker_executable("hello-toolbox")?;
    assert!(exe.is_file());
    assert!(exe.ends_with("bin/worker.py") || exe.ends_with(r"bin\worker.py"));
    assert!(args.is_empty());
    assert!(!frame_ch);

    // Spawn the imported .py worker via LocalWorkerHandle and verify live ping round-trip
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = crate::plugin_runtime::LocalWorkerHandle::spawn(
            crate::plugin_runtime::WorkerInit {
                plugin_id: "hello-toolbox".into(),
                executable: exe,
                args,
                working_dir: store.plugin_dir("hello-toolbox")?,
                data_dir: store.data_dir("hello-toolbox")?,
                default_download_dir: root.path().to_path_buf(),
                language: "zh".into(),
                frame_channel: false,
            },
            cancel,
            tx,
        )
        .await?;
        handle.set_state("message", serde_json::json!("template-ok")).await?;
        handle.send_action("ping", None, None).await?;
        let mut echoed = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            let Ok(Some(ev)) = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await else {
                break;
            };
            if let crate::plugin_runtime::WorkerEvent::StatePatch(patch) = ev {
                if patch.get("lastEcho").and_then(|v| v.as_str()) == Some("template-ok") {
                    echoed = true;
                    break;
                }
            }
        }
        handle.shutdown().await;
        assert!(echoed, "template Python worker did not echo ping action");
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

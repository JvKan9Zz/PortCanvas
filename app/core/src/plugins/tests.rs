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
    let mut files = vec![("manifest.json", tar::EntryType::Regular, data.as_slice())];
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

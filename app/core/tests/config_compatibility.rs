use anyhow::Result;
use portcanvas_core::{
    logs::LogService,
    preferences::{Appearance, PreferenceStore},
    profiles::{Profile, ProfileStore},
    session::DeviceManager,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
fn root() -> Result<PathBuf> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated WCS_TEST_ROOT"))
        .join(format!("config-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root)?;
    Ok(root)
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
#[test]
fn preference_extensions_survive_edits_and_deleted_media_stays_deleted() -> Result<()> {
    let root = root()?;
    let store = PreferenceStore {
        file: root.join("settings.json"),
    };
    let opaque = json!({"private":"synthetic-only","items":[null,true,{"v":1.25}],"未知":"原文"});
    fs::write(
        &store.file,
        serde_json::to_vec(
            &json!({"language":"en","theme":"green","futureSettings":opaque,"glassQuality":"full","backgroundImage":"0123456789abcdef0123456789abcdef.png","backgroundCrop":{"x":0.,"y":0.,"width":1.,"height":1.,"futureAnchor":"center"}}),
        )?,
    )?;
    store.save_appearance(Appearance::Dark)?;
    let saved = read(&store.file)?;
    assert_eq!(saved["futureSettings"], opaque);
    assert_eq!(saved["backgroundCrop"]["futureAnchor"], "center");
    assert!(store.info()?.get("futureSettings").is_none());
    store.save_background_color(Some("#112233".into()))?;
    let saved = read(&store.file)?;
    assert!(saved.get("backgroundImage").is_none() && saved.get("backgroundCrop").is_none());
    assert_eq!(saved["glassQuality"], "full");
    assert_eq!(saved["language"], "en");
    assert_eq!(saved["theme"], "green");
    assert_eq!(saved["futureSettings"], opaque);
    store.save_background_volume(27)?;
    assert_eq!(read(&store.file)?["futureSettings"], opaque);
    fs::write(&store.file, b"{broken")?;
    assert!(store.save_appearance(Appearance::Light).is_err());
    assert_eq!(fs::read(&store.file)?, b"{broken");
    fs::remove_dir_all(root)?;
    Ok(())
}
#[cfg(windows)]
#[test]
fn profile_extensions_follow_ids_while_credentials_remain_usable_and_private() -> Result<()> {
    let root = root()?;
    let store = ProfileStore {
        directory: root.clone(),
    };
    let file = root.join("devices.json");
    let make = |id: &str| -> Result<Profile> {
        Ok(serde_json::from_value(
            json!({"id":id,"name":id,"host":"192.0.2.1","port":22,"username":"fixture"}),
        )?)
    };
    let mut a = make("one")?;
    a.remember_password = true;
    store.save(a.clone(), "synthetic-password")?;
    store.save(make("two")?, "")?;
    let mut raw = read(&file)?;
    raw["futureRoot"] = json!({"format":"newer"});
    raw["devices"][0]["futurePrivate"] = json!({"token":"synthetic-A"});
    raw["devices"][1]["futurePrivate"] = json!({"token":"synthetic-B"});
    fs::write(&file, serde_json::to_vec(&raw)?)?;
    let mut data = store.read()?;
    data.devices.reverse();
    store.write(&data)?;
    a.name = "renamed".into();
    let saved = store.save(a.clone(), "")?;
    assert!(saved.public_value().get("futurePrivate").is_none());
    assert!(saved.public_value().get("encryptedSecret").is_none());
    assert_eq!(
        store
            .connection_secret(&a, zeroize::Zeroizing::new(String::new()))?
            .as_str(),
        "synthetic-password"
    );
    store.trust("192.0.2.1:22", &format!("SHA256:{}", "A".repeat(43)))?;
    let raw = read(&file)?;
    assert_eq!(raw["devices"][0]["futurePrivate"]["token"], "synthetic-B");
    assert_eq!(raw["devices"][1]["futurePrivate"]["token"], "synthetic-A");
    assert_eq!(raw["futureRoot"]["format"], "newer");
    let mut data = store.read()?;
    data.devices.retain(|p| p.id == "one");
    store.write(&data)?;
    a.remember_password = false;
    store.save(a.clone(), "")?;
    let raw = read(&file)?;
    assert_eq!(raw["devices"].as_array().unwrap().len(), 1);
    assert_eq!(raw["devices"][0]["futurePrivate"]["token"], "synthetic-A");
    assert!(raw["devices"][0].get("encryptedSecret").is_none());
    let mut unknown_version = raw.clone();
    unknown_version["schemaVersion"] = json!(2);
    fs::write(&file, serde_json::to_vec(&unknown_version)?)?;
    let before = fs::read(&file)?;
    assert!(store.write(&data).is_err());
    assert_eq!(fs::read(&file)?, before);
    for invalid in [Value::Null, json!(""), json!("bad;id")] {
        let mut malformed = raw.clone();
        if invalid.is_null() {
            malformed["devices"][0]
                .as_object_mut()
                .unwrap()
                .remove("id");
        } else {
            malformed["devices"][0]["id"] = invalid;
        }
        fs::write(&file, serde_json::to_vec(&malformed)?)?;
        let before = fs::read(&file)?;
        assert!(store.save(a.clone(), "").is_err());
        assert_eq!(fs::read(&file)?, before);
    }
    fs::write(&file, serde_json::to_vec(&raw)?)?;
    let mut duplicate = raw.clone();
    duplicate["devices"]
        .as_array_mut()
        .unwrap()
        .push(raw["devices"][0].clone());
    fs::write(&file, serde_json::to_vec(&duplicate)?)?;
    let before = fs::read(&file)?;
    assert!(store.save(a, "").is_err());
    assert_eq!(fs::read(&file)?, before);
    fs::remove_dir_all(root)?;
    Ok(())
}
#[tokio::test]
async fn log_preferences_preserve_extensions_and_explicit_repair_keeps_its_backup() -> Result<()> {
    let root = root()?;
    let file = root.join("log-settings.json");
    let original_logs = root.join("old-logs");
    fs::create_dir_all(&original_logs)?;
    fs::write(original_logs.join("keep.txt"), b"untouched")?;
    fs::write(
        &file,
        serde_json::to_vec(
            &json!({"root":original_logs,"projects":{"custom":"中文"},"futureLog":{"retention":"opaque"}}),
        )?,
    )?;
    let service = LogService::new(
        DeviceManager::new(Arc::new(|_, _| {})),
        file.clone(),
        root.join("default-logs"),
    )?;
    service.set_max_bytes(Some(64 * 1024 * 1024)).await?;
    assert_eq!(read(&file)?["futureLog"]["retention"], "opaque");
    assert!(service.snapshot().get("futureLog").is_none());
    let new_root = root.join("new-logs");
    fs::create_dir_all(&new_root)?;
    service.set_root(new_root).await?;
    assert_eq!(read(&file)?["futureLog"]["retention"], "opaque");
    assert_eq!(fs::read(original_logs.join("keep.txt"))?, b"untouched");
    fs::write(&file, b"{broken")?;
    assert!(service.set_max_bytes(None).await.is_err());
    assert_eq!(fs::read(&file)?, b"{broken");
    service.close().await?;
    drop(service);
    let repair = LogService::new(
        DeviceManager::new(Arc::new(|_, _| {})),
        file.clone(),
        root.join("default-logs"),
    )?;
    repair.set_max_bytes(None).await?;
    let backups = fs::read_dir(&root)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("log-settings.invalid-")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(backups[0].path())?, b"{broken");
    assert!(read(&file)?.get("maxBytes").is_none());
    repair.close().await?;
    drop(repair);
    fs::remove_dir_all(root)?;
    Ok(())
}

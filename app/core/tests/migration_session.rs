mod support;
use anyhow::Result;
use portcanvas_core::{
    logs::Settings,
    migration::{MigrationSession, Plan},
    profiles::Profile,
    services::{Paths, Services},
};
#[cfg(any(windows, all(target_os = "macos", debug_assertions)))]
use portcanvas_core::profiles::ProfileStore;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
};

struct Fixture {
    #[cfg(all(target_os = "macos", debug_assertions))]
    _keychain: Arc<support::TestKeychain>,
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
    logs: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self> {
        #[cfg(all(target_os = "macos", debug_assertions))]
        let keychain = support::keychain()?;
        let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated test root"))
            .join(format!("migration-{}", uuid::Uuid::new_v4()));
        let f = Self {
            #[cfg(all(target_os = "macos", debug_assertions))]
            _keychain: keychain,
            source: if cfg!(target_os = "macos") { root.join("Library/Application Support/io.portcanvas.app") } else { root.join("old") },
            target: if cfg!(target_os = "macos") { root.join("Library/Application Support/PortCanvas/Stable") } else { root.join("data") },
            logs: if cfg!(target_os = "macos") { root.join("Library/Application Support/io.portcanvas.app/logs") } else { root.join("old-logs") },
            root,
        };
        for dir in [&f.source, &f.target, &f.logs] {
            fs::create_dir_all(dir)?;
        }
        write(
            &f.source.join("devices.json"),
            json!({"devices":[profile("imported")?],"hostKeys":{},"sourceExtension":{"未知":[1,true,null]}}),
        )?;
        write(
            &f.target.join("devices.json"),
            json!({"devices":[profile("existing")?],"hostKeys":{},"targetExtension":17}),
        )?;
        write(
            &f.source.join("log-settings.json"),
            json!({"root":f.logs,"projects":{"old":"旧项目"},"sourceExtension":{"v":2}}),
        )?;
        fs::write(f.logs.join("keep.log"), b"original log")?;
        Ok(f)
    }
    fn services(&self) -> Result<Arc<Services>> {
        Services::new(
            Paths {
                plugins: None,
                data: self.target.clone(),
                documents: self.root.join("documents"),
                local_files: self.root.join("files"),
                portable: true,
            },
            Arc::new(|_, _| {}),
            None,
        )
    }
    fn plan(&self, settings: Settings) -> Result<Plan> {
        Plan::prepare(
            self.source.clone(),
            self.target.clone(),
            settings,
            self.root.join("no-legacy-logs"),
        )
    }
    fn snapshot(&self) -> Result<[Option<Vec<u8>>; 2]> {
        Ok([
            optional(&self.target.join("devices.json"))?,
            optional(&self.target.join("log-settings.json"))?,
        ])
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn optional(file: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(file) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn read(file: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(file)?)?)
}
fn write(file: &Path, value: Value) -> Result<()> {
    Ok(fs::write(file, serde_json::to_vec_pretty(&value)?)?)
}
fn profile(id: &str) -> Result<Profile> {
    Ok(serde_json::from_value(
        json!({"id":id,"name":id,"host":"192.0.2.1","port":22,"username":"fixture"}),
    )?)
}

// Mac credentials use debug-only disposable Keychain hooks. Keep Windows DPAPI
// coverage in release, and keep all credential-free migration cases enabled on Mac.
#[cfg(any(windows, all(target_os = "macos", debug_assertions)))]
#[tokio::test]
async fn import_preserves_opaque_data_and_usable_credentials_and_restore_is_exact() -> Result<()> {
    let f = Fixture::new()?;
    let store = ProfileStore {
        directory: f.source.clone(),
    };
    let mut p = profile("imported")?;
    p.remember_password = true;
    store.save(p, "synthetic-migration-password")?;
    let mut raw = read(&f.source.join("devices.json"))?;
    raw["devices"][0]["futureProfile"] = json!({"token":"synthetic-only"});
    write(&f.source.join("devices.json"), raw)?;
    write(
        &f.target.join("log-settings.json"),
        json!({"root":f.target.join("logs"),"projects":{},"maxBytes":16777216,"targetExtension":true}),
    )?;
    let services = f.services()?;
    let before = f.snapshot()?;
    let source = fs::read(f.source.join("devices.json"))?;
    let plan = f.plan(services.logs.settings())?;
    assert!(!plan.preview.to_string().contains("synthetic"));
    let result = MigrationSession::begin(services.clone())?
        .apply(plan)
        .await?;
    assert!(!services.migrating.load(Ordering::SeqCst));
    let after = f.snapshot()?;
    let raw = read(&f.target.join("devices.json"))?;
    assert_eq!(raw["sourceExtension"]["未知"], json!([1, true, null]));
    assert_eq!(raw["targetExtension"], 17);
    assert_eq!(
        raw["devices"][1]["futureProfile"]["token"],
        "synthetic-only"
    );
    let p = services
        .profiles
        .lock()
        .unwrap()
        .read()?
        .devices
        .pop()
        .unwrap();
    assert!(p.public_value().get("futureProfile").is_none());
    assert!(p.public_value().get("encryptedSecret").is_none());
    assert_eq!(
        services
            .profiles
            .lock()
            .unwrap()
            .connection_secret(&p, zeroize::Zeroizing::new(String::new()))?
            .as_str(),
        "synthetic-migration-password"
    );
    assert_eq!(services.logs.settings().root, f.logs);
    assert_eq!(services.logs.settings().max_bytes, Some(16777216));
    assert_eq!(
        read(&f.target.join("log-settings.json"))?["sourceExtension"],
        json!({"v":2})
    );
    assert_eq!(
        read(&f.target.join("log-settings.json"))?["targetExtension"],
        true
    );
    assert_eq!(f.plan(services.logs.settings())?.preview["addedDevices"], 0);
    let restored = MigrationSession::begin(services.clone())?
        .restore(PathBuf::from(result["backup"].as_str().unwrap()))
        .await?;
    assert_eq!(f.snapshot()?, before);
    let safety = PathBuf::from(restored["backup"].as_str().unwrap());
    assert_eq!(optional(&safety.join("devices.json"))?, after[0]);
    assert_eq!(optional(&safety.join("log-settings.json"))?, after[1]);
    assert_eq!(services.logs.settings().root, f.target.join("logs"));
    assert_eq!(fs::read(f.source.join("devices.json"))?, source);
    assert_eq!(fs::read(f.logs.join("keep.log"))?, b"original log");
    services.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn extension_conflicts_reject_root_profile_and_log_changes_without_backups() -> Result<()> {
    for location in 0..3 {
        let f = Fixture::new()?;
        let services = f.services()?;
        let mut source = read(&f.source.join("devices.json"))?;
        let mut target = read(&f.target.join("devices.json"))?;
        if location == 0 {
            source["targetExtension"] = json!(18);
        } else if location == 1 {
            target["devices"][0] = source["devices"][0].clone();
            target["devices"][0]["future"] = json!(1);
            source["devices"][0]["future"] = json!(2);
        } else {
            write(
                &f.target.join("log-settings.json"),
                json!({"root":f.target.join("logs"),"sourceExtension":{"v":3}}),
            )?;
        }
        write(&f.source.join("devices.json"), source)?;
        write(&f.target.join("devices.json"), target)?;
        let before = f.snapshot()?;
        assert!(f
            .plan(services.logs.settings())
            .err()
            .unwrap()
            .to_string()
            .contains("扩展字段冲突"));
        assert_eq!(f.snapshot()?, before);
        assert!(!f.target.join("migration-backups").exists());
        services.shutdown().await;
    }
    Ok(())
}
#[tokio::test]
async fn changed_input_or_destination_is_refused_and_runtime_is_restored() -> Result<()> {
    for source in [true, false] {
        let f = Fixture::new()?;
        let services = f.services()?;
        let old = services.logs.settings().root;
        let plan = f.plan(services.logs.settings())?;
        let changed = if source {
            f.source.join("devices.json")
        } else {
            f.target.join("devices.json")
        };
        fs::write(&changed, b"changed while confirming")?;
        let before = f.snapshot()?;
        assert!(MigrationSession::begin(services.clone())?
            .apply(plan)
            .await
            .is_err());
        assert_eq!(f.snapshot()?, before);
        assert_eq!(services.logs.settings().root, old);
        assert!(!services.migrating.load(Ordering::SeqCst));
        assert!(!f.target.join("migration-backups").exists());
        services.shutdown().await;
    }
    Ok(())
}
#[tokio::test]
async fn guard_rejects_overlap_quitting_and_wrong_target_and_releases_on_cancel() -> Result<()> {
    let f = Fixture::new()?;
    let services = f.services()?;
    let before = f.snapshot()?;
    let guard = MigrationSession::begin(services.clone())?;
    assert!(MigrationSession::begin(services.clone()).is_err());
    assert!(services.migrating.load(Ordering::SeqCst));
    drop(guard);
    assert!(!services.migrating.load(Ordering::SeqCst));
    assert_eq!(f.snapshot()?, before);
    let other = Fixture::new()?;
    let plan = other.plan(services.logs.settings())?;
    assert!(MigrationSession::begin(services.clone())?
        .apply(plan)
        .await
        .is_err());
    assert_eq!(f.snapshot()?, before);
    services.quitting.store(true, Ordering::SeqCst);
    assert!(MigrationSession::begin(services.clone()).is_err());
    assert!(!services.migrating.load(Ordering::SeqCst));
    services.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn restoring_missing_log_settings_reinstates_the_new_file_default() -> Result<()> {
    let f = Fixture::new()?;
    let services = f.services()?;
    let before = f.snapshot()?;
    let result = MigrationSession::begin(services.clone())?
        .apply(f.plan(services.logs.settings())?)
        .await?;
    MigrationSession::begin(services.clone())?
        .restore(PathBuf::from(result["backup"].as_str().unwrap()))
        .await?;
    assert_eq!(f.snapshot()?, before);
    assert_eq!(services.logs.settings().root, services.default_log_root);
    assert_eq!(services.logs.settings().max_bytes, Some(1024 * 1024 * 1024));
    services.shutdown().await;
    Ok(())
}
#[cfg(windows)]
#[tokio::test]
async fn failed_second_write_restores_first_file_and_runtime() -> Result<()> {
    let f = Fixture::new()?;
    write(
        &f.target.join("log-settings.json"),
        json!({"root":f.target.join("logs"),"projects":{}}),
    )?;
    let services = f.services()?;
    let before = f.snapshot()?;
    let runtime = services.logs.settings().root;
    let plan = f.plan(services.logs.settings())?;
    let file = f.target.join("log-settings.json");
    let original = fs::metadata(&file)?.permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&file, readonly)?;
    let result = MigrationSession::begin(services.clone())?.apply(plan).await;
    fs::set_permissions(&file, original)?;
    assert!(result.err().unwrap().to_string().contains("回滚已完成"));
    assert_eq!(f.snapshot()?, before);
    assert_eq!(services.logs.settings().root, runtime);
    assert!(!services.migrating.load(Ordering::SeqCst));
    services.shutdown().await;
    Ok(())
}
#[cfg(windows)]
#[tokio::test]
async fn differently_cased_alias_is_still_the_same_source_directory() -> Result<()> {
    let f = Fixture::new()?;
    let services = f.services()?;
    assert!(Plan::prepare(
        f.target.clone(),
        PathBuf::from(f.target.to_string_lossy().to_uppercase()),
        services.logs.settings(),
        f.logs.clone()
    )
    .is_err());
    assert!(!f.target.join("migration-backups").exists());
    services.shutdown().await;
    Ok(())
}

/// Old Mac v0.5.95 (527bf82a) used this AES-GCM envelope and exact profile AAD.
/// All keychain access here is to the explicit disposable test namespace.
#[cfg(all(target_os = "macos", debug_assertions))]
fn legacy_mac_cipher(profile: &Profile, secret: &str) -> Result<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use ring::{aead, rand::{SecureRandom, SystemRandom}};
    let service = portcanvas_core::credentials::debug_test_keychain_service()?;
    anyhow::ensure!(service.starts_with("io.portcanvas.test.credentials."));
    // Initialize only the isolated namespace; construct the legacy wire format independently.
    let _ = portcanvas_core::credentials::seal("synthetic-initialization", b"initialization")?;
    let key = zeroize::Zeroizing::new(security_framework::passwords::get_generic_password(&service, "encryption-key-v1")?);
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| anyhow::anyhow!("invalid fixture key"))?);
    let context = serde_json::to_vec(&json!(["PortCanvas credential v1", profile.id, profile.host, profile.port, profile.username, "password", ""]))?;
    let mut nonce = [0; 12];
    SystemRandom::new().fill(&mut nonce).map_err(|_| anyhow::anyhow!("fixture random failed"))?;
    let mut encrypted = secret.as_bytes().to_vec();
    key.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(&context), &mut encrypted).map_err(|_| anyhow::anyhow!("fixture encryption failed"))?;
    Ok(format!("keychain-v1:{}", STANDARD.encode([nonce.as_slice(), encrypted.as_slice()].concat())))
}

#[cfg(all(target_os = "macos", debug_assertions))]
#[tokio::test]
async fn legacy_mac_full_import_cancel_reopen_forget_and_restore_preserve_user_data() -> Result<()> {
    let f = Fixture::new()?;
    let files = ["devices.json", "log-settings.json", "settings.json", "terminals.json"];
    let image = "11111111111111111111111111111111.png";
    let secret = "synthetic-legacy-mac-password";
    let terminal_device = profile("imported")?.terminal_name();
    let mut source = read(&f.source.join(files[0]))?;
    source["devices"][0]["rememberPassword"] = json!(true);
    source["devices"][0]["encryptedSecret"] = json!(legacy_mac_cipher(&profile("imported")?, secret)?);
    source["devices"][0]["futureProfile"] = json!({"保留":true});
    source["hostKeys"] = json!({"192.0.2.1:22":format!("SHA256:{}", "A".repeat(43))});
    write(&f.source.join(files[0]), source)?;
    write(&f.source.join(files[2]), json!({"language":"en", "backgroundImage":image, "futurePreference":{"保留":true}}))?;
    write(&f.source.join(files[3]), json!({"devices":{(terminal_device.clone()):[{"key":"main","number":1,"futureTab":"保留"}]},"futureCatalog":true}))?;
    write(&f.target.join(files[2]), json!({"glassQuality":"balanced", "futureTarget":17}))?;
    fs::create_dir_all(f.source.join("backgrounds"))?;
    fs::write(f.source.join("backgrounds").join(image), b"synthetic-managed-media")?;
    let source_before = files.map(|name| optional(&f.source.join(name)).unwrap());
    let before = files.map(|name| optional(&f.target.join(name)).unwrap());
    let services = f.services()?;
    let prepare = || Plan::prepare_native(f.source.clone(), f.target.clone(), services.logs.settings(), f.logs.clone());
    let cancelled = prepare()?;
    assert_eq!(cancelled.preview["addedDevices"], 1);
    assert_eq!(cancelled.preview["backgroundFiles"], 1);
    assert!(!cancelled.preview.to_string().contains(secret));
    let guard = MigrationSession::begin(services.clone())?;
    drop(cancelled);
    drop(guard);
    assert_eq!(files.map(|name| optional(&f.target.join(name)).unwrap()), before);
    assert!(!f.target.join("migration-backups").exists());
    let result = MigrationSession::begin(services.clone())?.apply(prepare()?).await?;
    services.shutdown().await;
    drop(services);
    let reopened = f.services()?;
    let store = reopened.profiles.lock().unwrap().clone();
    let mut imported = store.read()?.devices.into_iter().find(|p| p.id == "imported").unwrap();
    assert_eq!(store.connection_secret(&imported, zeroize::Zeroizing::new(String::new()))?.as_str(), secret);
    assert_eq!(read(&f.target.join(files[0]))?["devices"][1]["futureProfile"], json!({"保留":true}));
    assert_eq!(read(&f.target.join(files[2]))?["futureTarget"], 17);
    assert_eq!(read(&f.target.join(files[2]))?["futurePreference"], json!({"保留":true}));
    assert_eq!(read(&f.target.join(files[3]))?["devices"][terminal_device.as_str()][0]["futureTab"], "保留");
    assert_eq!(fs::read(f.target.join("backgrounds").join(image))?, b"synthetic-managed-media");
    imported.remember_password = false;
    assert!(store.save(imported.clone(), "")?.encrypted_secret.is_none());
    assert!(store.connection_secret(&imported, zeroize::Zeroizing::new(String::new())).is_err());
    assert_eq!(store.read()?.host_keys.len(), 1);
    MigrationSession::begin(reopened.clone())?.restore(PathBuf::from(result["backup"].as_str().unwrap())).await?;
    assert_eq!(files.map(|name| optional(&f.target.join(name)).unwrap()), before);
    assert_eq!(files.map(|name| optional(&f.source.join(name)).unwrap()), source_before);
    assert_eq!(fs::read(f.source.join("backgrounds").join(image))?, b"synthetic-managed-media");
    assert_eq!(fs::read(f.logs.join("keep.log"))?, b"original log");
    reopened.shutdown().await;
    Ok(())
}

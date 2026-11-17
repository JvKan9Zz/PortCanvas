#![cfg(target_os = "macos")]
use portcanvas_native_probe::macos_legacy_cleanup as cleanup;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
};
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated test root"))
            .join(format!("macos-legacy-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"preserve exact bytes").unwrap();
        path
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const CACHE: &str = "Library/Caches/io.portcanvas.app/WebKit";
#[test]
fn readiness_cleanup_removes_only_owned_network_caches_preserving_persistent_data() {
    let s = Sandbox::new();
    let cache = s.write(&format!("{CACHE}/NetworkCache/Version 17/Blobs/hash"));
    s.write(&format!("{CACHE}/HSTS/state"));
    s.write(&format!("{CACHE}/AlternativeServices/hosts"));
    s.write(&format!("{CACHE}/CacheStorage/cache"));
    let retained = [
        "Library/WebKit/io.portcanvas.app/WebsiteData/LocalStorage/localstorage",
        "Library/WebKit/io.portcanvas.app/WebsiteData/IndexedDB/db",
        "Library/Application Support/io.portcanvas.app/devices.json",
        "Library/Application Support/PortCanvas/Stable/settings.json",
        "Library/Application Support/io.portcanvas.app/backgrounds/photo.png",
        "Library/Logs/io.portcanvas.app/output.log",
        "Library/Caches/io.portcanvas.app/user.txt",
    ];
    for name in retained {
        s.write(name);
    }
    let plan = cleanup::inspect(&s.0).unwrap();
    assert!(cache.exists(), "inspection must be read-only");
    assert_eq!(plan.candidates.len(), 1);
    let report = plan.execute_after_readiness();
    assert!(report.failures.is_empty(), "{:?}", report);
    assert_eq!(report.removed.len(), 1);
    assert!(!s.0.join(CACHE).exists());
    for name in retained {
        assert_eq!(fs::read(s.0.join(name)).unwrap(), b"preserve exact bytes");
    }
    assert!(cleanup::inspect(&s.0).unwrap().candidates.is_empty());
}
#[test]
fn mixed_cache_directory_and_embedded_source_data_are_rejected_without_partial_deletion() {
    for extra in [
        "notes.txt",
        "UnknownCache/file",
        "NetworkCache/devices.json",
    ] {
        let s = Sandbox::new();
        let old = s.write(&format!("{CACHE}/HSTS/state"));
        s.write(&format!("{CACHE}/{extra}"));
        assert!(cleanup::inspect(&s.0).is_err(), "{extra}");
        assert!(old.exists());
    }
}
#[test]
fn linked_ancestors_targets_and_descendants_cannot_escape_cleanup_scope() {
    for target in [
        "Library",
        "Library/Caches/io.portcanvas.app/WebKit",
        "Library/Caches/io.portcanvas.app/WebKit/NetworkCache/escape",
    ] {
        let s = Sandbox::new();
        let external = Sandbox::new();
        let keep = external.write("user.json");
        let link = s.0.join(target);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(&external.0, &link).unwrap();
        assert!(cleanup::inspect(&s.0).is_err(), "{target}");
        assert!(keep.exists());
    }
}
#[test]
fn changed_plan_or_unsafe_permissions_report_failure_and_preserve_other_data() {
    let s = Sandbox::new();
    let old = s.write(&format!("{CACHE}/NetworkCache/hash"));
    let plan = cleanup::inspect(&s.0).unwrap();
    let outside = Sandbox::new();
    let keep = outside.write("user.json");
    fs::rename(s.0.join(CACHE), s.0.join("saved-cache")).unwrap();
    symlink(&outside.0, s.0.join(CACHE)).unwrap();
    let report = plan.execute_after_readiness();
    assert_eq!(report.failures.len(), 1);
    assert!(keep.exists());
    assert!(s.0.join("saved-cache/NetworkCache/hash").exists());
    fs::remove_file(s.0.join(CACHE)).unwrap();
    fs::rename(s.0.join("saved-cache"), s.0.join(CACHE)).unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(cleanup::inspect(&s.0).is_err());
    assert!(old.exists());
}
#[test]
fn relative_home_is_rejected() {
    assert!(cleanup::inspect(Path::new("relative")).is_err());
}

#[test]
fn a_new_mixed_entry_after_preview_prevents_all_deletion() {
    let s = Sandbox::new();
    let keep = s.write(&format!("{CACHE}/HSTS/state"));
    let plan = cleanup::inspect(&s.0).unwrap();
    s.write(&format!("{CACHE}/personal-notes.txt"));
    let report = plan.execute_after_readiness();
    assert_eq!(report.failures.len(), 1);
    assert!(keep.exists());
}
#[test]
fn hard_linked_user_file_is_retained() {
    let s = Sandbox::new();
    let external = Sandbox::new();
    let keep = external.write("user.txt");
    let destination = s.0.join(format!("{CACHE}/NetworkCache/hash"));
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::hard_link(&keep, &destination).unwrap();
    assert!(cleanup::inspect(&s.0).is_err());
    assert_eq!(fs::read(keep).unwrap(), b"preserve exact bytes");
    assert!(destination.exists());
}
#[test]
fn isolated_update_environment_never_cleans_the_accounts_legacy_cache() {
    if std::env::var_os("PORTCANVAS_CLEANUP_GUARD_CHILD").is_some() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let report = cleanup::current_user_after_readiness();
        assert!(report.removed.is_empty());
        assert!(report.failures.is_empty());
        assert!(home.join(CACHE).join("HSTS/state").exists());
        return;
    }
    let s = Sandbox::new();
    s.write(&format!("{CACHE}/HSTS/state"));
    // Test the guard in a fresh process with a synthetic HOME. Never unset guards
    // or redirect HOME inside this process, where unrelated tests may be running.
    for override_name in ["WCS_TEST_ROOT", "WCS_DATA_DIR"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_update_environment_never_cleans_the_accounts_legacy_cache",
            ])
            .env("PORTCANVAS_CLEANUP_GUARD_CHILD", "1")
            .env("HOME", &s.0)
            .env_remove("WCS_TEST_ROOT")
            .env_remove("WCS_DATA_DIR")
            .env(override_name, &s.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn excessively_deep_cache_is_retained_without_recursing_unboundedly() {
    let s = Sandbox::new();
    let path = format!("{CACHE}/NetworkCache/{}hash", "d/".repeat(70));
    let keep = s.write(&path);
    assert!(cleanup::inspect(&s.0).is_err());
    assert!(keep.exists());
}

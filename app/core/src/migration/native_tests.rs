use super::*;
const IMAGE: &str = "11111111111111111111111111111111.png";
const VIDEO: &str = "22222222222222222222222222222222.mp4";
const DEVICE: &str = "wcs-1234567890abcdef";
const TAB: &str = "00000000-0000-4000-8000-000000000001";
struct Fixture { root: PathBuf, source: PathBuf, target: PathBuf, logs: Settings }
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());
        let source = root.join("old-data");
        let target = root.join("new-data");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(source.join(FILES[0]), include_bytes!("../../tests/fixtures/devices.json")).unwrap();
        let logs = Settings { root: root.join("logs"), projects: Default::default(), max_bytes: None };
        atomic_json(&source.join(FILES[1]), &logs).unwrap();
        Self { root, source, target, logs }
    }
    fn plan(&self) -> Result<Plan> { Plan::prepare_native(self.source.clone(), self.target.clone(), self.logs.clone(), self.root.join("missing")) }
    fn save(&self, source: bool, name: &str, value: Value) {
        atomic_json(&if source { &self.source } else { &self.target }.join(name), &value).unwrap();
    }
    fn media(&self, source: bool, name: &str, bytes: &[u8]) {
        let path = if source { &self.source } else { &self.target }.join("backgrounds");
        fs::create_dir_all(&path).unwrap(); fs::write(path.join(name), bytes).unwrap();
    }
}
impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); } }
fn raw(path: &Path) -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() }

#[test]
fn full_import_preserves_new_preferences_opaque_tabs_and_exact_rollback() {
    let f = Fixture::new();
    f.save(false, FILES[0], json!({"devices":[],"hostKeys":{}}));
    f.save(false, FILES[1], serde_json::to_value(&f.logs).unwrap());
    f.save(false, FILES[2], json!({"glassQuality":"balanced","backgroundVideo":VIDEO,"backgroundVolume":17,"backgroundCrop":{"x":0,"y":0,"width":1,"height":1,"futureCrop":true},"futureSetting":{"x":1}}));
    f.save(false, FILES[3], json!({"futureCatalog":true,"devices":{DEVICE:[{"key":"main","number":1,"futureTab":"kept"}]}}));
    f.media(false, VIDEO, b"old-target-video");
    f.save(true, FILES[2], json!({"language":"en","appearance":"dark","glassOpacity":0,"backgroundImage":IMAGE,"backgroundCrop":{"x":0.1,"y":0.2,"width":0.8,"height":0.7},"futureSetting":{"x":1}}));
    f.save(true, FILES[3], json!({"devices":{DEVICE:[{"key":"main","number":1,"futureTab":"kept"},{"key":TAB,"number":2,"futureTab":"imported"}]}}));
    let mut image = b"\x89PNG\r\n\x1a\n".to_vec(); image.resize(11*1024*1024, 73);
    f.media(true, IMAGE, &image);
    let before = FILES.map(|name| fs::read(f.target.join(name)).unwrap());
    let source = FILES.map(|name| fs::read(f.source.join(name)).unwrap());
    let report = f.plan().unwrap().apply().unwrap();
    let p = raw(&f.target.join(FILES[2]));
    assert_eq!(p["glassQuality"], "balanced"); assert_eq!(p["glassOpacity"], 0);
    assert_eq!(p["backgroundImage"], IMAGE); assert!(p.get("backgroundVideo").is_none());
    assert_eq!(p["backgroundVolume"], 17); assert_eq!(p["futureSetting"]["x"], 1);
    assert_eq!(p["backgroundCrop"]["futureCrop"], true);
    let t = raw(&f.target.join(FILES[3]));
    assert_eq!(t["devices"][DEVICE][1]["key"], TAB);
    assert_eq!(t["devices"][DEVICE][1]["futureTab"], "imported"); assert_eq!(t["futureCatalog"], true);
    assert_eq!(fs::read(f.target.join("backgrounds").join(IMAGE)).unwrap(), image);
    for (index, name) in FILES.iter().enumerate() { assert_eq!(fs::read(f.source.join(name)).unwrap(), source[index]); }
    // Normal preference changes may remove the old referenced media after import.
    fs::remove_file(f.target.join("backgrounds").join(VIDEO)).unwrap();
    let backup = PathBuf::from(report["backup"].as_str().unwrap());
    assert_eq!(raw(&backup.join("report.json"))["beforeExists"].as_array().unwrap().len(), 4);
    rollback(f.target.clone(), backup).unwrap();
    for (index, name) in FILES.iter().enumerate() { assert_eq!(fs::read(f.target.join(name)).unwrap(), before[index]); }
    assert_eq!(fs::read(f.target.join("backgrounds").join(VIDEO)).unwrap(), b"old-target-video");
    assert!(f.source.join("backgrounds").join(IMAGE).exists());
}

#[test]
fn legacy_receipt_only_restores_the_original_two_files() {
    let f = Fixture::new();
    let report = Plan::prepare(f.source.clone(), f.target.clone(), f.logs.clone(), f.root.join("missing")).unwrap().apply().unwrap();
    f.save(false, FILES[2], json!({"glassOpacity":37}));
    f.save(false, FILES[3], json!({"devices":{DEVICE:[]}}));
    let before = [fs::read(f.target.join(FILES[2])).unwrap(), fs::read(f.target.join(FILES[3])).unwrap()];
    rollback(f.target.clone(), report["backup"].as_str().unwrap().into()).unwrap();
    for (i, name) in FILES.iter().skip(2).enumerate() { assert_eq!(fs::read(f.target.join(name)).unwrap(), before[i]); }
}

#[test]
fn opaque_conflicts_and_terminal_identity_conflicts_are_refused_before_writes() {
    let f = Fixture::new();
    f.save(false, FILES[2], json!({"futureSetting":1}));
    f.save(true, FILES[2], json!({"futureSetting":2}));
    assert!(f.plan().is_err());
    f.save(true, FILES[2], json!({"futureSetting":1}));
    f.save(false, FILES[3], json!({"devices":{DEVICE:[{"key":"main","number":1},{"key":TAB,"number":2}]}}));
    f.save(true, FILES[3], json!({"devices":{DEVICE:[{"key":TAB,"number":3}]}}));
    assert!(f.plan().is_err());
    f.save(true, FILES[3], json!({"devices":{DEVICE:[{"key":"00000000-0000-4000-8000-000000000002","number":2}]}}));
    assert!(f.plan().is_err()); assert!(!f.target.join("migration-backups").exists());
}

#[test]
fn managed_media_paths_missing_files_collisions_and_snapshot_changes_are_refused() {
    let f = Fixture::new();
    f.save(true, FILES[2], json!({"backgroundImage":"../escape.png"})); assert!(f.plan().is_err());
    f.save(true, FILES[2], json!({"backgroundImage":IMAGE})); assert!(f.plan().is_err());
    f.media(true, IMAGE, b"source"); f.media(false, IMAGE, b"collision"); assert!(f.plan().is_err());
    fs::remove_file(f.target.join("backgrounds").join(IMAGE)).unwrap();
    let plan = f.plan().unwrap();
    f.media(true, IMAGE, b"source changed"); assert!(plan.apply().is_err());
    assert!(!f.target.join(FILES[0]).exists()); assert!(!f.target.join("migration-backups").exists());
}

#[test]
fn a_changed_fourth_configuration_is_never_overwritten() {
    let f = Fixture::new();
    let plan = f.plan().unwrap();
    f.save(false, FILES[3], json!({"devices":{DEVICE:[]}}));
    let before = fs::read(f.target.join(FILES[3])).unwrap();
    assert!(plan.apply().is_err()); assert_eq!(fs::read(f.target.join(FILES[3])).unwrap(), before);
    assert!(!f.target.join(FILES[0]).exists());
}

#[cfg(windows)]
#[test]
fn failure_writing_the_fourth_file_rolls_back_the_first_three_exactly() {
    let f = Fixture::new();
    f.save(false, FILES[0], json!({"devices":[],"hostKeys":{}}));
    f.save(false, FILES[1], serde_json::to_value(&f.logs).unwrap());
    f.save(false, FILES[2], json!({"language":"zh-CN","glassQuality":"full"}));
    f.save(false, FILES[3], json!({"devices":{DEVICE:[]}}));
    f.save(true, FILES[2], json!({"language":"en"}));
    f.save(true, FILES[3], json!({"devices":{DEVICE:[{"key":"main","number":1}]}}));
    let before = FILES.map(|name| fs::read(f.target.join(name)).unwrap());
    let path = f.target.join(FILES[3]); let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true); fs::set_permissions(&path, perms).unwrap();
    let result = f.plan().unwrap().apply();
    let mut perms = fs::metadata(&path).unwrap().permissions(); perms.set_readonly(false); fs::set_permissions(&path, perms).unwrap();
    assert!(result.unwrap_err().to_string().contains("回滚已完成"));
    for (index, name) in FILES.iter().enumerate() { assert_eq!(fs::read(f.target.join(name)).unwrap(), before[index]); }
}

use crate::{
    logs::Settings,
    profiles::{atomic_bytes, atomic_json, ProfileData},
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
const FILES: [&str; 2] = ["devices.json", "log-settings.json"];
fn optional(path: &Path) -> Result<Option<Vec<u8>>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 1024 * 1024 {
        bail!(
            "配置必须是最多1MiB的普通JSON文件，原件保留：{}",
            path.display()
        );
    }
    Ok(Some(fs::read(path)?))
}
fn known_fields(value: &Value, allowed: &[&str]) -> Result<()> {
    let object = value.as_object().context("旧配置不是JSON对象。")?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        bail!("配置包含不受支持的字段，未进行导入；原件保留。");
    }
    Ok(())
}
fn profiles(bytes: &[u8]) -> Result<ProfileData> {
    let raw: Value = serde_json::from_slice(bytes).context("设备资料JSON无效，原件保留。")?;
    known_fields(&raw, &["devices", "hostKeys", "schemaVersion", "fixture"])?;
    for profile in raw["devices"].as_array().context("设备列表无效")? {
        let id = profile["id"]
            .as_str()
            .context("旧设备缺少稳定ID，未导入。")?;
        if id.is_empty()
            || id.len() > 80
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            bail!("旧设备ID无效，未生成替代ID；原件保留。");
        }
        known_fields(
            profile,
            &[
                "id",
                "name",
                "host",
                "port",
                "username",
                "display",
                "xauthority",
                "authMethod",
                "privateKeyPath",
                "rememberPassword",
                "encryptedSecret",
            ],
        )?;
    }
    let data: ProfileData = serde_json::from_value(raw)?;
    let data = data.validate()?;
    let mut ids = HashSet::new();
    if data.devices.iter().any(|p| !ids.insert(&p.id)) {
        bail!("设备资料包含重复ID。");
    }
    Ok(data)
}
fn settings(bytes: &[u8]) -> Result<Settings> {
    let raw: Value = serde_json::from_slice(bytes).context("日志设置JSON无效，原件保留。")?;
    known_fields(&raw, &["root", "projects", "maxBytes"])?;
    serde_json::from_value::<Settings>(raw)?.validate()
}
pub struct Plan {
    source: PathBuf,
    target: PathBuf,
    before: [Option<Vec<u8>>; 2],
    input: [Option<Vec<u8>>; 2],
    pub profiles: ProfileData,
    pub settings: Settings,
    pub preview: Value,
}
impl Plan {
    pub fn prepare(
        source: PathBuf,
        target: PathBuf,
        current_settings: Settings,
        legacy_log_root: PathBuf,
    ) -> Result<Self> {
        if !source.is_absolute() || !target.is_absolute() || source == target {
            bail!("请选择独立的旧版data目录。");
        }
        let input = [
            optional(&source.join(FILES[0]))?,
            optional(&source.join(FILES[1]))?,
        ];
        let old = profiles(input[0].as_deref().context("所选目录没有devices.json。")?)?;
        let before = [
            optional(&target.join(FILES[0]))?,
            optional(&target.join(FILES[1]))?,
        ];
        let mut merged = match &before[0] {
            Some(bytes) => profiles(bytes)?,
            None => ProfileData::default(),
        };
        if let Some(bytes) = &before[1] {
            let _ = settings(bytes)?;
        }
        let mut devices = 0;
        let mut keys = 0;
        for profile in old.devices {
            if let Some(existing) = merged.devices.iter().find(|p| p.id == profile.id) {
                if existing != &profile {
                    bail!("设备ID冲突，未覆盖现有资料：{}", profile.name);
                }
            } else {
                devices += 1;
                merged.devices.push(profile);
            }
        }
        for (address, key) in old.host_keys {
            if let Some(existing) = merged.host_keys.get(&address) {
                if existing != &key {
                    bail!("主机指纹信任冲突，未自动替换：{address}");
                }
            } else {
                keys += 1;
                merged.host_keys.insert(address, key);
            }
        }
        let mut logs = current_settings;
        let mut warning = String::new();
        if let Some(bytes) = &input[1] {
            let old = settings(bytes)?;
            if old.root.is_dir() {
                logs.root = old.root;
            } else {
                warning = "旧日志位置不可用，未搬移或删除，可在日志页重新选择。".into();
            }
            for (key, name) in old.projects {
                logs.projects.entry(key).or_insert(name);
            }
        } else if legacy_log_root.is_dir() {
            logs.root = legacy_log_root;
        }
        let preview = json!({"source":source,"target":target,"addedDevices":devices,"addedHostKeys":keys,"logRoot":logs.root,"warning":warning,"logsMoved":false});
        Ok(Self {
            source,
            target,
            before,
            input,
            profiles: merged,
            settings: logs.validate()?,
            preview,
        })
    }
    pub fn apply(self) -> Result<Value> {
        for (index, name) in FILES.iter().enumerate() {
            if optional(&self.source.join(name))? != self.input[index]
                || optional(&self.target.join(name))? != self.before[index]
            {
                bail!("配置在确认期间发生变化，请重新导入。");
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let backup = self.target.join("migration-backups").join(&id);
        fs::create_dir_all(backup.join("before"))?;
        fs::create_dir_all(backup.join("input"))?;
        for (index, name) in FILES.iter().enumerate() {
            if let Some(bytes) = &self.before[index] {
                atomic_bytes(&backup.join("before").join(name), bytes)?;
            }
            if let Some(bytes) = &self.input[index] {
                atomic_bytes(&backup.join("input").join(name), bytes)?;
            }
        }
        let report = json!({"schema":"portcanvas.migration/1","id":id,"target":self.target,"source":self.source,"createdAt":crate::log_store::now(),"beforeExists":self.before.iter().map(Option::is_some).collect::<Vec<_>>(),"summary":self.preview});
        atomic_json(&backup.join("report.json"), &report)?;
        let operation: Result<()> = (|| {
            atomic_json(&self.target.join(FILES[0]), &self.profiles)?;
            atomic_json(&self.target.join(FILES[1]), &self.settings)?;
            Ok(())
        })();
        if let Err(error) = operation {
            let rollback = restore_files(&self.target, &self.before);
            bail!(
                "导入失败：{error}；回滚{}。备份：{}",
                if rollback.is_ok() {
                    "已完成"
                } else {
                    "未完成，请检查备份"
                },
                backup.display()
            );
        }
        let mut result = self.preview;
        result["backup"] = json!(backup);
        result["applied"] = json!(true);
        Ok(result)
    }
}
fn restore_files(target: &Path, before: &[Option<Vec<u8>>; 2]) -> Result<()> {
    for (index, name) in FILES.iter().enumerate() {
        let path = target.join(name);
        if let Some(bytes) = &before[index] {
            atomic_bytes(&path, bytes)?;
        } else {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(())
}
pub fn rollback(target: PathBuf, backup: PathBuf) -> Result<Value> {
    let root = dunce::canonicalize(target.join("migration-backups"))?;
    let selected = dunce::canonicalize(&backup)?;
    if selected.parent() != Some(root.as_path())
        || fs::symlink_metadata(&backup)?.file_type().is_symlink()
    {
        bail!("请选择此软件资料目录中的迁移备份。");
    }
    let report: Value =
        serde_json::from_slice(&optional(&selected.join("report.json"))?.context("迁移回执缺失")?)?;
    if report["schema"] != "portcanvas.migration/1"
        || report["id"].as_str() != selected.file_name().and_then(|s| s.to_str())
        || dunce::canonicalize(PathBuf::from(
            report["target"].as_str().context("回执目标缺失")?,
        ))? != dunce::canonicalize(&target)?
    {
        bail!("迁移回执身份或目标不匹配。");
    }
    let mut before = [None, None];
    let flags = report["beforeExists"]
        .as_array()
        .context("迁移回执缺少原文件状态。")?;
    if flags.len() != FILES.len() || flags.iter().any(|v| !v.is_boolean()) {
        bail!("迁移回执中的原文件状态无效。");
    }
    for (index, name) in FILES.iter().enumerate() {
        let saved = optional(&selected.join("before").join(name))?;
        if flags[index].as_bool().unwrap() != saved.is_some() {
            bail!("迁移备份与回执不一致，未恢复。");
        }
        if let Some(bytes) = saved {
            if index == 0 {
                profiles(&bytes)?;
            } else {
                settings(&bytes)?;
            }
            before[index] = Some(bytes);
        }
    }
    let current = [
        optional(&target.join(FILES[0]))?,
        optional(&target.join(FILES[1]))?,
    ];
    let safety = target
        .join("rollback-backups")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&safety)?;
    for (index, name) in FILES.iter().enumerate() {
        if let Some(bytes) = &current[index] {
            atomic_bytes(&safety.join(name), bytes)?;
        }
    }
    if let Err(error) = restore_files(&target, &before) {
        let _ = restore_files(&target, &current);
        bail!("恢复失败：{error}。当前资料备份：{}", safety.display());
    }
    Ok(json!({"restored":true,"backup":safety,"logsDeleted":false}))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root = std::env::var_os("WCS_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .ancestors()
                    .nth(3)
                    .unwrap()
                    .join("05-测试与验证/local/v04/rust-migration-tests")
            });
        let dir = root.join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn source(root: &Path) -> (PathBuf, Settings) {
        let source = root.join("old-data");
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("devices.json"),
            include_bytes!("../tests/fixtures/devices.json"),
        )
        .unwrap();
        let logs = root.join("logs");
        fs::create_dir(&logs).unwrap();
        let settings = Settings {
            root: logs,
            projects: BTreeMap::new(),
            max_bytes: None,
        };
        atomic_json(&source.join("log-settings.json"), &settings).unwrap();
        (source, settings)
    }
    use std::collections::BTreeMap;
    #[test]
    fn import_preserves_ids_trust_and_source_and_is_idempotent() {
        let root = fixture();
        let (source, settings) = source(&root);
        let target = root.join("new-data");
        let bytes = fs::read(source.join("devices.json")).unwrap();
        let plan = Plan::prepare(
            source.clone(),
            target.clone(),
            settings.clone(),
            root.join("missing"),
        )
        .unwrap();
        let expected = plan.profiles.clone();
        let report = plan.apply().unwrap();
        let actual: ProfileData =
            serde_json::from_slice(&fs::read(target.join("devices.json")).unwrap()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(fs::read(source.join("devices.json")).unwrap(), bytes);
        assert!(Path::new(report["backup"].as_str().unwrap())
            .join("report.json")
            .is_file());
        let second = Plan::prepare(source, target, settings, root.join("missing")).unwrap();
        assert_eq!(second.preview["addedDevices"], 0);
        assert_eq!(second.preview["addedHostKeys"], 0);
    }
    #[test]
    fn trust_conflicts_and_extra_secret_fields_are_refused() {
        let root = fixture();
        let (source, settings) = source(&root);
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        let mut data = profiles(&fs::read(source.join("devices.json")).unwrap()).unwrap();
        data.host_keys
            .insert("192.0.2.1:22".into(), format!("SHA256:{}", "B".repeat(43)));
        atomic_json(&target.join("devices.json"), &data).unwrap();
        assert!(Plan::prepare(
            source.clone(),
            target.clone(),
            settings.clone(),
            root.join("none")
        )
        .is_err());
        let mut raw: Value =
            serde_json::from_slice(&fs::read(source.join("devices.json")).unwrap()).unwrap();
        raw["devices"][0]["password"] = json!("test-secret-do-not-import");
        atomic_json(&source.join("devices.json"), &raw).unwrap();
        assert!(Plan::prepare(source, target, settings, root.join("none")).is_err());
    }
    #[test]
    fn changed_destination_is_not_overwritten() {
        let root = fixture();
        let (source, settings) = source(&root);
        let target = root.join("target");
        let plan = Plan::prepare(source, target.clone(), settings, root.join("none")).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("devices.json"), b"changed").unwrap();
        assert!(plan.apply().is_err());
        assert_eq!(fs::read(target.join("devices.json")).unwrap(), b"changed");
    }
    #[test]
    fn malformed_identity_and_backup_flags_never_change_current_files() {
        let root = fixture();
        let (source, settings) = source(&root);
        let target = root.join("target");
        let bytes = fs::read(source.join("devices.json")).unwrap();
        let mut raw: Value = serde_json::from_slice(&bytes).unwrap();
        raw["devices"][0].as_object_mut().unwrap().remove("id");
        assert!(profiles(&serde_json::to_vec(&raw).unwrap()).is_err());
        let report = Plan::prepare(source, target.clone(), settings, root.join("none"))
            .unwrap()
            .apply()
            .unwrap();
        let backup = PathBuf::from(report["backup"].as_str().unwrap());
        let current = fs::read(target.join("devices.json")).unwrap();
        let mut receipt: Value =
            serde_json::from_slice(&fs::read(backup.join("report.json")).unwrap()).unwrap();
        receipt["beforeExists"] = json!([false]);
        atomic_json(&backup.join("report.json"), &receipt).unwrap();
        assert!(rollback(target.clone(), backup).is_err());
        assert_eq!(fs::read(target.join("devices.json")).unwrap(), current);
    }
    #[test]
    fn rollback_restores_exact_bytes_and_keeps_log_files() {
        let root = fixture();
        let (source, settings) = source(&root);
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        let before = b"{\"devices\":[],\"hostKeys\":{}}\n";
        fs::write(target.join("devices.json"), before).unwrap();
        fs::write(settings.root.join("keep.log"), "keep").unwrap();
        let report = Plan::prepare(source, target.clone(), settings.clone(), root.join("none"))
            .unwrap()
            .apply()
            .unwrap();
        rollback(
            target.clone(),
            PathBuf::from(report["backup"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(fs::read(target.join("devices.json")).unwrap(), before);
        assert!(!target.join("log-settings.json").exists());
        assert_eq!(
            fs::read_to_string(settings.root.join("keep.log")).unwrap(),
            "keep"
        );
    }
}

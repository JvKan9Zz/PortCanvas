use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
pub(super) const PROFILE_ROOT: &[&str] = &["devices", "hostKeys", "schemaVersion"];
pub(super) const PROFILE_FIELDS: &[&str] = &[
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
];
pub(super) const LOG_FIELDS: &[&str] = &["root", "projects", "maxBytes"];
pub(super) fn extra(value: &Value, known: &[&str]) -> Result<Map<String, Value>> {
    let object = value.as_object().context("旧配置不是JSON对象。")?;
    let mut result = Map::new();
    for (key, value) in object {
        if !known.contains(&key.as_str()) {
            if matches!(
                key.to_ascii_lowercase().as_str(),
                "password" | "passphrase" | "privatekey" | "private_key" | "secret"
            ) {
                bail!("配置包含明文凭据字段，未进行导入；原件保留。");
            }
            result.insert(key.clone(), value.clone());
        }
    }
    Ok(result)
}
fn combine(target: &mut Map<String, Value>, input: Map<String, Value>) -> Result<()> {
    for (key, value) in input {
        if let Some(old) = target.get(&key) {
            if old != &value {
                bail!("配置扩展字段冲突，未覆盖现有资料。");
            }
        } else {
            target.insert(key, value);
        }
    }
    Ok(())
}
pub(super) fn output(
    input: &[Option<Vec<u8>>; 2],
    before: &[Option<Vec<u8>>; 2],
    profiles: &crate::profiles::ProfileData,
    logs: &crate::logs::Settings,
) -> Result<[Vec<u8>; 2]> {
    let mut root = Map::new();
    let mut records: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut log_extra = Map::new();
    for files in [before, input] {
        if let Some(bytes) = &files[0] {
            let raw: Value = serde_json::from_slice(bytes)?;
            combine(&mut root, extra(&raw, PROFILE_ROOT)?)?;
            for profile in raw["devices"].as_array().context("设备列表无效")? {
                let id = profile["id"]
                    .as_str()
                    .context("旧设备缺少稳定ID，未导入。")?;
                combine(
                    records.entry(id.into()).or_default(),
                    extra(profile, PROFILE_FIELDS)?,
                )?;
            }
        }
        if let Some(bytes) = &files[1] {
            let raw: Value = serde_json::from_slice(bytes)?;
            combine(&mut log_extra, extra(&raw, LOG_FIELDS)?)?;
        }
    }
    let mut profiles = serde_json::to_value(profiles)?;
    profiles.as_object_mut().unwrap().extend(root);
    for record in profiles["devices"].as_array_mut().unwrap() {
        let id = record["id"].as_str().unwrap();
        if let Some(extra) = records.get(id) {
            record.as_object_mut().unwrap().extend(extra.clone());
        }
    }
    let mut logs = serde_json::to_value(logs)?;
    logs.as_object_mut().unwrap().extend(log_extra);
    Ok([
        serde_json::to_vec_pretty(&profiles)?,
        serde_json::to_vec_pretty(&logs)?,
    ])
}

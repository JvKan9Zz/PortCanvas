//! Preserve opaque configuration fields without adding them to UI-facing models.
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

fn identity(value: &Value) -> Option<(&str, &str)> {
    ["id", "key"]
        .into_iter()
        .find_map(|field| value.get(field)?.as_str().map(|value| (field, value)))
}
fn index<'a>(items: &'a [Value], field: &str) -> Result<BTreeMap<&'a str, &'a Value>> {
    let mut result = BTreeMap::new();
    for item in items {
        let key = item
            .get(field)
            .and_then(Value::as_str)
            .context("配置对象标识不完整或重复，原文件保留。")?;
        ensure!(
            result.insert(key, item).is_none(),
            "配置对象标识不完整或重复，原文件保留。"
        );
    }
    Ok(result)
}
fn preserve(original: &Value, before: &Value, after: &mut Value) -> Result<()> {
    match (original, before, after) {
        (Value::Object(raw), Value::Object(known), Value::Object(next)) => {
            for (key, value) in raw {
                match (known.get(key), next.get_mut(key)) {
                    (None, None) => {
                        next.insert(key.clone(), value.clone());
                    }
                    (Some(old), Some(new)) => preserve(value, old, new)?,
                    _ => {} // Removing a known field is intentional, even when it had opaque children.
                }
            }
        }
        (Value::Array(raw), Value::Array(known), Value::Array(next)) => {
            // Device and terminal records have stable identities; never merge by row position.
            let Some((field, _)) = known.first().and_then(identity) else {
                return Ok(());
            };
            let raw = index(raw, field)?;
            let known = index(known, field)?;
            let mut seen = BTreeSet::new();
            for new in next {
                if let Some(key) = new.get(field).and_then(Value::as_str).map(str::to_owned) {
                    ensure!(
                        seen.insert(key.clone()),
                        "配置对象标识不完整或重复，原文件保留。"
                    );
                    if let (Some(raw), Some(old)) = (raw.get(key.as_str()), known.get(key.as_str()))
                    {
                        preserve(raw, old, new)?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn write<T: Serialize + DeserializeOwned>(path: &Path, value: &T) -> Result<()> {
    let mut next = serde_json::to_value(value)?;
    match fs::read(path) {
        Ok(bytes) => {
            let original: Value =
                serde_json::from_slice(&bytes).context("配置读取失败，原文件保留。")?;
            let before: T =
                serde_json::from_value(original.clone()).context("配置读取失败，原文件保留。")?;
            preserve(&original, &serde_json::to_value(before)?, &mut next)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    crate::profiles::atomic_json(path, &next)
}

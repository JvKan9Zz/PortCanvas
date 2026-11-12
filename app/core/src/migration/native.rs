use super::extensions;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, time::SystemTime};

const PREFERENCES: &[&str] = &[
    "language", "theme", "appearance", "desktopMode", "environmentSource",
    "backgroundColor", "backgroundImage", "backgroundVideo", "backgroundCrop",
    "backgroundVolume", "glassOpacity", "glassQuality",
];
const BACKGROUND: &[&str] = &["backgroundColor", "backgroundImage", "backgroundVideo", "backgroundCrop"];

pub(super) fn preferences(bytes: &[u8]) -> Result<Value> {
    crate::preferences::decode(bytes)?;
    let raw: Value = serde_json::from_slice(bytes)?;
    extensions::extra(&raw, PREFERENCES)?;
    Ok(raw)
}

pub(super) fn merge_preferences(input: Option<&[u8]>, before: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut target = before.map(preferences).transpose()?.unwrap_or(json!({}));
    if let Some(bytes) = input {
        let mut source = preferences(bytes)?;
        if let (Some(old), Some(next)) = (target.get("backgroundCrop").filter(|v|v.is_object()), source.get_mut("backgroundCrop").filter(|v|v.is_object())) {
            let fields = &["x", "y", "width", "height"];
            let mut opaque = extensions::extra(old, fields)?;
            extensions::combine(&mut opaque, extensions::extra(next, fields)?)?;
            next.as_object_mut().unwrap().extend(opaque);
        }
        let mut opaque = extensions::extra(&target, PREFERENCES)?;
        extensions::combine(&mut opaque, extensions::extra(&source, PREFERENCES)?)?;
        let object = target.as_object_mut().unwrap();
        // A background is one choice; importing a color must not retain a target video.
        if BACKGROUND.iter().any(|name| source.get(*name).is_some()) {
            for name in BACKGROUND { object.remove(*name); }
        }
        for (key, value) in source.as_object().unwrap() { object.insert(key.clone(), value.clone()); }
        object.extend(opaque);
    }
    let bytes = serde_json::to_vec_pretty(&target)?;
    preferences(&bytes)?;
    Ok(bytes)
}

pub(super) fn terminals(bytes: &[u8]) -> Result<Value> {
    crate::terminal_pool::validate_catalog(bytes)?;
    let raw: Value = serde_json::from_slice(bytes)?;
    extensions::extra(&raw, &["devices"])?;
    for tabs in raw["devices"].as_object().context("终端设备列表无效。")?.values() {
        for tab in tabs.as_array().context("终端列表无效。")? {
            extensions::extra(tab, &["key", "number"])?;
        }
    }
    Ok(raw)
}

pub(super) fn merge_terminals(input: Option<&[u8]>, before: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut target = before.map(terminals).transpose()?.unwrap_or(json!({"devices":{}}));
    if let Some(bytes) = input {
        let source = terminals(bytes)?;
        let mut opaque = extensions::extra(&target, &["devices"])?;
        extensions::combine(&mut opaque, extensions::extra(&source, &["devices"])?)?;
        target.as_object_mut().unwrap().extend(opaque);
        let devices = target["devices"].as_object_mut().unwrap();
        for (device, tabs) in source["devices"].as_object().unwrap() {
            let current = devices.entry(device.clone()).or_insert(json!([])).as_array_mut().unwrap();
            for tab in tabs.as_array().unwrap() {
                if let Some(existing) = current.iter_mut().find(|t| t["key"] == tab["key"]) {
                    if existing["number"] != tab["number"] { bail!("终端编号冲突，未覆盖现有列表。"); }
                    let mut extra = extensions::extra(existing, &["key", "number"])?;
                    extensions::combine(&mut extra, extensions::extra(tab, &["key", "number"])?)?;
                    existing.as_object_mut().unwrap().extend(extra);
                } else {
                    if current.iter().any(|t| t["number"] == tab["number"]) { bail!("终端编号冲突，未覆盖现有列表。"); }
                    current.push(tab.clone());
                }
            }
        }
    }
    let bytes = serde_json::to_vec_pretty(&target)?;
    terminals(&bytes)?;
    Ok(bytes)
}

#[derive(Clone)]
pub(super) struct Media {
    pub name: String,
    path: PathBuf,
    length: u64,
    modified: SystemTime,
}
fn media_path(root: &Path, name: &str) -> Result<PathBuf> {
    anyhow::ensure!(crate::preferences::valid_media_name(name, false) || crate::preferences::valid_media_name(name, true), "背景文件名无效。");
    let directory = root.join("backgrounds");
    match fs::symlink_metadata(&directory) {
        Ok(meta) => anyhow::ensure!(meta.is_dir() && !meta.file_type().is_symlink(), "背景目录必须是普通目录。"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(e) => return Err(e.into()),
    }
    Ok(directory.join(name))
}
impl Media {
    pub(super) fn open(root: &Path, name: &str) -> Result<Self> {
        let path = media_path(root, name)?;
        let meta = fs::symlink_metadata(&path).with_context(|| format!("背景文件缺失，原配置保留：{name}"))?;
        anyhow::ensure!(meta.is_file() && !meta.file_type().is_symlink(), "背景必须是普通文件。");
        Ok(Self { name: name.into(), path, length: meta.len(), modified: meta.modified()? })
    }
    pub fn check(&self) -> Result<()> {
        let meta = fs::symlink_metadata(&self.path)?;
        anyhow::ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() == self.length && meta.modified()? == self.modified, "背景在确认期间发生变化，请重新导入。");
        Ok(())
    }
    pub fn copy_to(&self, root: &Path) -> Result<()> {
        self.check()?;
        let target = media_path(root, &self.name)?;
        if target.try_exists()? {
            let existing = Self::open(root, &self.name)?;
            if same_files(&self.path, &existing.path)? { return Ok(()); }
        }
        fs::create_dir_all(target.parent().unwrap())?;
        let temp = target.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut input = fs::File::open(&self.path)?;
            let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
            let mut buffer = [0; 65536];
            loop { let n = input.read(&mut buffer)?; if n == 0 { break; } output.write_all(&buffer[..n])?; }
            output.sync_all()?;
            drop(output);
            self.check()?;
            anyhow::ensure!(same_files(&self.path, &temp)?, "背景复制期间发生变化，未提交配置。");
            // Same-volume replace uses the same atomic platform primitive as configuration writes.
            fs::rename(&temp, &target)?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&temp); }
        result
    }
}
fn same_files(a: &Path, b: &Path) -> Result<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() { return Ok(false); }
    let (mut a, mut b) = (fs::File::open(a)?, fs::File::open(b)?);
    let (mut left, mut right) = ([0; 65536], [0; 65536]);
    loop {
        let n = a.read(&mut left)?;
        if n == 0 { return Ok(true); }
        b.read_exact(&mut right[..n])?;
        if left[..n] != right[..n] { return Ok(false); }
    }
}
pub(super) fn media(root: &Path, settings: Option<&[u8]>) -> Result<Vec<Media>> {
    let Some(bytes) = settings else { return Ok(vec![]); };
    let p = crate::preferences::decode(bytes)?;
    p.background_image.or(p.background_video).map(|name| Media::open(root, &name)).transpose().map(|m| m.into_iter().collect())
}
pub(super) fn check_media(items: &[Media]) -> Result<()> { for m in items { m.check()?; } Ok(()) }
pub(super) fn copy_media(items: &[Media], root: &Path) -> Result<()> { for m in items { m.copy_to(root)?; } Ok(()) }
pub(super) fn check_collision(items: &[Media], target: &Path) -> Result<()> {
    for item in items {
        let path = media_path(target, &item.name)?;
        if path.try_exists()? {
            let existing = Media::open(target, &item.name)?;
            anyhow::ensure!(same_files(&item.path, &existing.path)?, "同名背景内容冲突，未覆盖现有文件。");
        }
    }
    Ok(())
}
pub(super) fn verify_backup(items: &[Media], backup: &Path) -> Result<()> {
    for item in items {
        item.check()?;
        anyhow::ensure!(same_files(&item.path, &media_path(backup, &item.name)?)?, "背景在备份期间发生变化，请重新导入。");
    }
    Ok(())
}

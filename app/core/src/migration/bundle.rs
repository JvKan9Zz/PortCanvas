use super::{native, optional, profiles, Plan, FILES};
use crate::{logs::Settings, profiles::ProfileData};
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

const FORMAT: &[u8] = b"portcanvas.config/1\n";
const CONFIG_LIMIT: u64 = 1024 * 1024;

pub fn export(source: &Path, destination: &Path, default_log_root: &Path) -> Result<()> {
    let devices = match optional(&source.join(FILES[0]))? {
        Some(bytes) => {
            profiles(&bytes)?;
            let mut data: Value = serde_json::from_slice(&bytes)?;
            for device in data["devices"].as_array_mut().unwrap() {
                device.as_object_mut().unwrap().remove("encryptedSecret");
                device["rememberPassword"] = Value::Bool(false);
            }
            data
        }
        None => serde_json::to_value(ProfileData::default())?,
    };
    let output = tempfile::NamedTempFile::new_in(destination.parent().context("导出位置无效。")?)?;
    let mut archive = tar::Builder::new(output);
    append(&mut archive, "format", FORMAT)?;
    append(&mut archive, FILES[0], &serde_json::to_vec(&devices)?)?;
    for name in &FILES[1..] {
        let Some(mut data) = optional(&source.join(name))? else {
            continue;
        };
        if *name == "log-settings.json" {
            let mut raw: Value = serde_json::from_slice(&data)?;
            if super::settings(&data)?.root == default_log_root {
                raw.as_object_mut().unwrap().remove("root");
                data = serde_json::to_vec(&raw)?;
            }
        }
        if *name == "settings.json" {
            for media in native::media(source, Some(&data))? {
                let relative = format!("backgrounds/{}", media.name);
                archive.append_path_with_name(source.join(&relative), &relative)?;
                media.check()?;
            }
        }
        append(&mut archive, name, &data)?;
    }
    let output = archive.into_inner()?;
    output.as_file().sync_all()?;
    output.persist(destination)?;
    Ok(())
}

fn append(archive: &mut tar::Builder<tempfile::NamedTempFile>, name: &str, data: &[u8]) -> Result<()> {
    ensure!(data.len() as u64 <= CONFIG_LIMIT, "配置文件超过 1 MiB。");
    let mut header = tar::Header::new_ustar();
    header.set_mode(0o600);
    header.set_size(data.len() as u64);
    archive.append_data(&mut header, name, data)?;
    Ok(())
}

pub fn prepare(
    archive: PathBuf,
    target: PathBuf,
    current: Settings,
    default_log_root: PathBuf,
) -> Result<(Plan, tempfile::TempDir)> {
    let input = File::open(&archive)?;
    let length = input.metadata()?.len();
    let stage = tempfile::Builder::new().prefix(".config-import-").tempdir_in(&target)?;
    let mut names = HashSet::new();
    let mut media = None;
    for entry in tar::Archive::new(input).entries()? {
        let mut entry = entry?;
        let path = entry.path_bytes();
        let name = std::str::from_utf8(&path)?.to_owned();
        ensure!(entry.header().entry_type().is_file() && names.insert(name.clone()), "配置包内容无效。");
        match name.as_str() {
            "format" => {
                ensure!(entry.size() == FORMAT.len() as u64, "配置包格式无效。");
                let mut version = Vec::new();
                entry.read_to_end(&mut version)?;
                ensure!(version == FORMAT, "配置包版本不支持。");
            }
            name if FILES.contains(&name) => {
                ensure!(entry.size() <= CONFIG_LIMIT, "配置文件超过 1 MiB。");
                let mut data = Vec::new();
                entry.read_to_end(&mut data)?;
                if name == "log-settings.json" {
                    let mut raw: Value = serde_json::from_slice(&data)?;
                    raw.as_object_mut()
                        .context("日志设置无效。")?
                        .entry("root")
                        .or_insert(serde_json::to_value(&default_log_root)?);
                    data = serde_json::to_vec(&raw)?;
                }
                fs::write(stage.path().join(name), data)?;
            }
            name if name.starts_with("backgrounds/") => {
                let filename = &name["backgrounds/".len()..];
                ensure!(
                    media.is_none() && entry.size() <= length
                        && (crate::preferences::valid_media_name(filename, false)
                            || crate::preferences::valid_media_name(filename, true)),
                    "配置包中的背景文件无效。"
                );
                fs::create_dir(stage.path().join("backgrounds"))?;
                let mut output = File::create(stage.path().join(name))?;
                ensure!(io::copy(&mut entry, &mut output)? == entry.size(), "背景文件不完整。");
                media = Some(filename.to_owned());
            }
            _ => bail!("配置包包含无法识别的文件。"),
        }
    }
    ensure!(names.contains("format"), "请选择 PortCanvas 导出的配置文件。");
    let mut plan = Plan::prepare_input(stage.path().into(), target, current, default_log_root, true)?
        .with_native_files()?;
    ensure!(plan.input_media.first().map(|item| item.name.as_str()) == media.as_deref(), "配置包的背景与设置不一致。");
    plan.preview["source"] = serde_json::to_value(archive)?;
    Ok((plan, stage))
}

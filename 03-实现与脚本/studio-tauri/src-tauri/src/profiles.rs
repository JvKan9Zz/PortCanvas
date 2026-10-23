use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn display() -> String {
    ":0".into()
}
fn schema() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default = "new_id")]
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default = "display")]
    pub display: String,
    #[serde(default)]
    pub xauthority: String,
}
impl Profile {
    pub fn validate(mut self) -> Result<Self> {
        self.name = self.name.trim().into();
        self.host = self.host.trim().into();
        self.username = self.username.trim().into();
        if self.name.is_empty() || self.name.chars().count() > 80 {
            bail!("请输入设备名称（最多 80 字）。");
        }
        if self.host.is_empty()
            || self.host.len() > 253
            || !self
                .host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".:-".contains(&c))
        {
            bail!("请输入有效的 IP 地址或主机名。");
        }
        if self.port == 0 {
            bail!("SSH 端口应为 1–65535。");
        }
        if self.username.is_empty()
            || self.username.len() > 64
            || !self
                .username
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        {
            bail!("请输入有效的 Linux 用户名。");
        }
        if !regex::Regex::new(r"^:\d{1,3}(\.\d)?$")?.is_match(&self.display) {
            bail!("桌面显示编号格式应为 :0 或 :1。");
        }
        if !self.xauthority.is_empty()
            && (!self.xauthority.starts_with('/') || self.xauthority.contains(['\r', '\n', '\0']))
        {
            bail!("桌面授权文件须为 Linux 绝对路径。");
        }
        if self.id.is_empty()
            || self.id.len() > 80
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            self.id = new_id();
        }
        Ok(self)
    }
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
    pub fn terminal_name(&self) -> String {
        use sha2::{Digest, Sha256};
        let source =
            serde_json::to_string(&serde_json::json!([self.host, self.port, self.username]))
                .unwrap();
        let hash = Sha256::digest(source.as_bytes());
        format!(
            "wcs-{}",
            hash.iter()
                .take(8)
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileData {
    #[serde(default = "schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub devices: Vec<Profile>,
    #[serde(default)]
    pub host_keys: BTreeMap<String, String>,
}
impl Default for ProfileData {
    fn default() -> Self {
        Self {
            schema_version: 1,
            devices: vec![],
            host_keys: BTreeMap::new(),
        }
    }
}
impl ProfileData {
    pub fn validate(mut self) -> Result<Self> {
        if self.schema_version != 1 {
            bail!("设备资料版本不受支持，原文件保留。");
        }
        self.devices = self
            .devices
            .into_iter()
            .map(Profile::validate)
            .collect::<Result<_>>()?;
        if self.host_keys.values().any(|v| {
            !regex::Regex::new(r"^SHA256:[A-Za-z0-9+/]{43}$")
                .unwrap()
                .is_match(v)
        }) {
            bail!("主机指纹资料无效，原文件保留。");
        }
        Ok(self)
    }
}
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_bytes(path, &serde_json::to_vec_pretty(value)?)
}
pub fn atomic_bytes(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context("文件位置无效")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".wcs-write-{}.tmp", uuid::Uuid::new_v4()));
    let result: Result<()> = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        use std::io::Write;
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
#[derive(Clone)]
pub struct ProfileStore {
    pub directory: PathBuf,
}
impl ProfileStore {
    pub fn read(&self) -> Result<ProfileData> {
        let path = self.directory.join("devices.json");
        if !path.exists() {
            return Ok(ProfileData::default());
        }
        serde_json::from_slice::<ProfileData>(&fs::read(path)?)
            .context("设备资料读取失败，原文件保留。")?
            .validate()
    }
    pub fn write(&self, data: &ProfileData) -> Result<()> {
        atomic_json(&self.directory.join("devices.json"), data)
    }
    pub fn save(&self, profile: Profile) -> Result<Profile> {
        let profile = profile.validate()?;
        let mut data = self.read()?;
        if let Some(old) = data.devices.iter_mut().find(|p| p.id == profile.id) {
            *old = profile.clone();
        } else {
            data.devices.push(profile.clone());
        }
        self.write(&data)?;
        Ok(profile)
    }
    pub fn trust(&self, address: &str, fingerprint: &str) -> Result<()> {
        let mut data = self.read()?;
        data.host_keys.insert(address.into(), fingerprint.into());
        self.write(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_profile_and_session_identity() {
        let p: Profile = serde_json::from_value(serde_json::json!({"id":"old","name":"设备","host":"192.0.2.1","port":22,"username":"demo","display":":0","xauthority":""})).unwrap();
        assert_eq!(p.terminal_name(), "wcs-a96d1d05b24621e4");
        let mut renamed = p.clone();
        renamed.name = "新名字".into();
        assert_eq!(renamed.terminal_name(), p.terminal_name());
        renamed.host = "bad;host".into();
        assert!(renamed.validate().is_err());
    }
    #[test]
    fn legacy_host_trust_survives_serialization() {
        let raw = serde_json::json!({"devices":[],"hostKeys":{"host:22":format!("SHA256:{}","A".repeat(43))}});
        let data: ProfileData = serde_json::from_value(raw).unwrap();
        let data = data.validate().unwrap();
        assert_eq!(
            serde_json::to_value(data).unwrap()["hostKeys"]["host:22"],
            format!("SHA256:{}", "A".repeat(43))
        );
    }
}

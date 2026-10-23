use crate::profiles::atomic_json;
use anyhow::{Context,Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::path::PathBuf;

pub const REPOSITORY:&str="https://github.com/JvKan9Zz/PortCanvas";
#[derive(Clone,Default,Deserialize,Serialize)]
pub struct Preferences {
    #[serde(default)]
    pub language:Language,
    #[serde(default)]
    pub theme:Theme,
}
#[derive(Clone,Copy,Default,Deserialize,Serialize,PartialEq)]
#[serde(rename_all="lowercase")]
pub enum Theme {#[default] Blue,Green,Graphite}
#[derive(Clone,Copy,Default,Deserialize,Serialize,PartialEq)]
pub enum Language {
    #[default]
    #[serde(rename="zh-CN")]
    Chinese,
    #[serde(rename="en")]
    English,
}
pub struct PreferenceStore {pub file:PathBuf}
impl PreferenceStore {
    pub fn read(&self)->Result<Preferences>{
        if !self.file.exists(){return Ok(Preferences::default());}
        serde_json::from_slice(&std::fs::read(&self.file)?).context("设置读取失败，原文件保留。")
    }
    pub fn language(&self)->Language {self.read().map(|p|p.language).unwrap_or_default()}
    pub fn info(&self)->Result<Value>{let p=self.read()?;Ok(json!({"language":p.language,"theme":p.theme,"version":env!("CARGO_PKG_VERSION"),"repository":REPOSITORY}))}
    pub fn save_language(&self,language:Language)->Result<Value>{let mut p=self.read()?;p.language=language;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_theme(&self,theme:Theme)->Result<Value>{let mut p=self.read()?;p.theme=theme;atomic_json(&self.file,&p)?;self.info()}
}
pub fn update_result(status:u32,body:&[u8])->Result<Value>{
    if status==404{return Ok(json!({"status":"unavailable"}));}
    if status==403||status==429{return Ok(json!({"status":"limited"}));}
    anyhow::ensure!(status==200,"更新服务返回 HTTP {status}。");
    let package:Value=serde_json::from_slice(body).context("更新信息无效。")?;
    anyhow::ensure!(package["name"]=="portcanvas","更新来源无效。");
    let latest=package["version"].as_str().context("更新信息缺少版本号。")?;
    let current=semver::Version::parse(env!("CARGO_PKG_VERSION"))?;let remote=semver::Version::parse(latest).context("更新版本号无效。")?;
    Ok(json!({"status":if remote>current{"available"}else if remote==current{"current"}else{"ahead"},"latest":latest}))
}
pub fn check_update()->Result<Value>{
    #[cfg(windows)] {let(status,body)=github_package()?;update_result(status,&body)}
    #[cfg(not(windows))] {Ok(json!({"status":"unsupported"}))}
}
#[cfg(windows)]
fn github_package()->Result<(u32,Vec<u8>)>{
    use std::{ffi::c_void,ptr::{null,null_mut}};
    use windows_sys::Win32::Networking::WinHttp::*;
    struct Handle(*mut c_void);
    impl Handle {fn new(value:*mut c_void)->Result<Self>{anyhow::ensure!(!value.is_null(),"检查更新失败（系统状态{}）。",std::io::Error::last_os_error());Ok(Self(value))}}
    impl Drop for Handle {fn drop(&mut self){unsafe{WinHttpCloseHandle(self.0);}}}
    fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
    fn ok(value:i32)->Result<()>{anyhow::ensure!(value!=0,"检查更新失败（系统状态{}）。",std::io::Error::last_os_error());Ok(())}
    unsafe {
        let agent=wide(concat!("PortCanvas/",env!("CARGO_PKG_VERSION")));
        let session=Handle::new(WinHttpOpen(agent.as_ptr(),WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,null(),null(),0))?;
        ok(WinHttpSetTimeouts(session.0,5000,5000,5000,10000))?;
        let host=wide("api.github.com");let connection=Handle::new(WinHttpConnect(session.0,host.as_ptr(),443,0))?;
        let method=wide("GET");let path=wide("/repos/JvKan9Zz/PortCanvas/contents/03-%E5%AE%9E%E7%8E%B0%E4%B8%8E%E8%84%9A%E6%9C%AC/studio-tauri/package.json?ref=main");
        let request=Handle::new(WinHttpOpenRequest(connection.0,method.as_ptr(),path.as_ptr(),null(),null(),null(),WINHTTP_FLAG_SECURE))?;
        let header=wide("Accept: application/vnd.github.raw+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n");
        ok(WinHttpSendRequest(request.0,header.as_ptr(),(header.len()-1)as u32,null(),0,0,0))?;ok(WinHttpReceiveResponse(request.0,null_mut()))?;
        let mut status=0u32;let mut size=4u32;ok(WinHttpQueryHeaders(request.0,WINHTTP_QUERY_STATUS_CODE|WINHTTP_QUERY_FLAG_NUMBER,null(),&mut status as *mut _ as *mut c_void,&mut size,null_mut()))?;
        let mut body=Vec::new();let mut buffer=[0u8;8192];let start=std::time::Instant::now();
        loop{anyhow::ensure!(start.elapsed().as_secs()<20,"检查更新超时。");let mut read=0u32;ok(WinHttpReadData(request.0,buffer.as_mut_ptr()as *mut c_void,buffer.len()as u32,&mut read))?;if read==0{break;}anyhow::ensure!(body.len()+read as usize<=262144,"更新信息过长。");body.extend_from_slice(&buffer[..read as usize]);}
        Ok((status,body))
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn preference_language_persists_without_touching_other_files(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());let store=PreferenceStore{file:root.join("settings.json")};
        assert_eq!(store.info().unwrap()["language"],"zh-CN");assert_eq!(store.info().unwrap()["theme"],"blue");
        std::fs::create_dir_all(&root).unwrap();std::fs::write(&store.file,br#"{"language":"en"}"#).unwrap();assert_eq!(store.info().unwrap()["theme"],"blue");
        store.save_theme(Theme::Green).unwrap();assert_eq!(store.info().unwrap()["language"],"en");
        store.save_language(Language::Chinese).unwrap();assert_eq!(store.info().unwrap()["theme"],"green");
        assert!(serde_json::from_value::<Theme>(json!("invalid")).is_err());
        std::fs::write(&store.file,b"{broken").unwrap();assert!(store.save_language(Language::Chinese).is_err());assert_eq!(std::fs::read(&store.file).unwrap(),b"{broken");
    }
    #[test]fn inaccessible_or_invalid_updates_are_never_reported_current(){
        assert_eq!(update_result(404,b"{}").unwrap()["status"],"unavailable");assert_eq!(update_result(403,b"{}").unwrap()["status"],"limited");assert!(update_result(200,br#"{"name":"other","version":"9.0.0"}"#).is_err());assert!(update_result(200,br#"{"name":"portcanvas","version":"bad"}"#).is_err());
        assert_eq!(update_result(200,br#"{"name":"portcanvas","version":"0.4.10"}"#).unwrap()["status"],"available");
        let body=json!({"name":"portcanvas","version":env!("CARGO_PKG_VERSION")}).to_string();assert_eq!(update_result(200,body.as_bytes()).unwrap()["status"],"current");assert_eq!(update_result(200,br#"{"name":"portcanvas","version":"0.4.8"}"#).unwrap()["status"],"ahead");
    }
}

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
    #[serde(default,rename="desktopMode")]
    pub desktop_mode:crate::desktop_settings::DesktopMode,
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
    pub fn info(&self)->Result<Value>{let p=self.read()?;Ok(json!({"language":p.language,"theme":p.theme,"desktopMode":p.desktop_mode,"version":env!("CARGO_PKG_VERSION"),"repository":REPOSITORY,"updatesSupported":cfg!(windows)}))}
    pub fn save_language(&self,language:Language)->Result<Value>{let mut p=self.read()?;p.language=language;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_theme(&self,theme:Theme)->Result<Value>{let mut p=self.read()?;p.theme=theme;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_desktop_mode(&self,mode:crate::desktop_settings::DesktopMode)->Result<Value>{let mut p=self.read()?;p.desktop_mode=mode;atomic_json(&self.file,&p)?;self.info()}
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn preference_language_persists_without_touching_other_files(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());let store=PreferenceStore{file:root.join("settings.json")};
        assert_eq!(store.info().unwrap()["language"],"zh-CN");assert_eq!(store.info().unwrap()["theme"],"blue");
        std::fs::create_dir_all(&root).unwrap();std::fs::write(&store.file,br#"{"language":"en"}"#).unwrap();assert_eq!(store.info().unwrap()["theme"],"blue");
        assert_eq!(store.info().unwrap()["desktopMode"],"smooth");
        store.save_desktop_mode(crate::desktop_settings::DesktopMode::Economy).unwrap();
        store.save_theme(Theme::Green).unwrap();assert_eq!(store.info().unwrap()["language"],"en");
        store.save_language(Language::Chinese).unwrap();assert_eq!(store.info().unwrap()["theme"],"green");
        assert_eq!(store.info().unwrap()["desktopMode"],"economy");
        assert!(serde_json::from_value::<Theme>(json!("invalid")).is_err());
        std::fs::write(&store.file,b"{broken").unwrap();assert!(store.save_language(Language::Chinese).is_err());assert_eq!(std::fs::read(&store.file).unwrap(),b"{broken");
    }
}

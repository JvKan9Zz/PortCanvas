use crate::profiles::atomic_json;
use anyhow::{Context,Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::path::PathBuf;
use base64::Engine;
use std::io::Read;

pub const REPOSITORY:&str="https://github.com/JvKan9Zz/PortCanvas";
#[derive(Clone,Deserialize,Serialize)]
pub struct Preferences {
    #[serde(default)]
    pub language:Language,
    #[serde(default)]
    pub theme:Theme,
    #[serde(default,rename="desktopMode")]
    pub desktop_mode:crate::desktop_settings::DesktopMode,
    #[serde(default,rename="environmentSource")]
    pub environment_source:EnvironmentSource,
    #[serde(default)]
    pub appearance:Appearance,
    #[serde(default,rename="backgroundColor",skip_serializing_if="Option::is_none")]
    pub background_color:Option<String>,
    #[serde(default,rename="backgroundImage",skip_serializing_if="Option::is_none")]
    pub background_image:Option<String>,
    #[serde(default="default_glass_opacity",rename="glassOpacity")]
    pub glass_opacity:u8,
}
fn default_glass_opacity()->u8{100}
impl Default for Preferences{
    fn default()->Self{Self{language:Language::default(),theme:Theme::default(),desktop_mode:Default::default(),environment_source:EnvironmentSource::default(),appearance:Appearance::default(),background_color:None,background_image:None,glass_opacity:100}}
}
#[derive(Clone,Copy,Default,Deserialize,Serialize,PartialEq,Debug)]
#[serde(rename_all="lowercase")]
pub enum Appearance {#[default] Light,Dark,System}
#[derive(Clone,Copy,Default,Deserialize,Serialize,PartialEq,Debug)]
#[serde(rename_all="lowercase")]
pub enum EnvironmentSource {#[default] Computer,Board}
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
fn valid_color(value:&str)->bool{value.len()==7&&value.starts_with('#')&&value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)}
fn valid_image_name(value:&str)->bool{
    value.rsplit_once('.').is_some_and(|(stem,extension)|stem.len()==32&&stem.bytes().all(|b|b.is_ascii_hexdigit())&&matches!(extension,"png"|"jpg"|"webp"))
}
fn image_extension(bytes:&[u8])->Option<&'static str>{
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n"){Some("png")}
    else if bytes.starts_with(b"\xff\xd8\xff"){Some("jpg")}
    else if bytes.len()>=12&&&bytes[..4]==b"RIFF"&&&bytes[8..12]==b"WEBP"{Some("webp")}else{None}
}
fn image_bytes(path:&std::path::Path)->Result<Vec<u8>>{
    let mut bytes=Vec::new();std::fs::File::open(path)?.take(10*1024*1024+1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len()<=10*1024*1024&&image_extension(&bytes).is_some(),"请选择 10 MiB 以内的 PNG、JPEG 或 WebP 图片。");Ok(bytes)
}
impl PreferenceStore {
    pub fn read(&self)->Result<Preferences>{
        if !self.file.exists(){return Ok(Preferences::default());}
        let p:Preferences=serde_json::from_slice(&std::fs::read(&self.file)?).context("设置读取失败，原文件保留。")?;
        anyhow::ensure!(p.glass_opacity<=100,"非透明度设置无效，原设置保留。");
        anyhow::ensure!(p.background_color.as_deref().is_none_or(valid_color),"背景颜色配置无效，原设置保留。");
        anyhow::ensure!(p.background_image.as_deref().is_none_or(valid_image_name),"背景图片配置无效，原设置保留。");Ok(p)
    }
    pub fn language(&self)->Language {self.read().map(|p|p.language).unwrap_or_default()}
    pub fn info(&self)->Result<Value>{let p=self.read()?;Ok(json!({"language":p.language,"theme":p.theme,"appearance":p.appearance,"backgroundColor":p.background_color,"backgroundImage":p.background_image,"glassOpacity":p.glass_opacity,"desktopMode":p.desktop_mode,"environmentSource":p.environment_source,"version":env!("CARGO_PKG_VERSION"),"repository":REPOSITORY,"updatesSupported":cfg!(windows)}))}
    pub fn save_language(&self,language:Language)->Result<Value>{let mut p=self.read()?;p.language=language;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_theme(&self,theme:Theme)->Result<Value>{let mut p=self.read()?;p.theme=theme;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_desktop_mode(&self,mode:crate::desktop_settings::DesktopMode)->Result<Value>{let mut p=self.read()?;p.desktop_mode=mode;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_environment_source(&self,source:EnvironmentSource)->Result<Value>{let mut p=self.read()?;p.environment_source=source;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_glass_opacity(&self,value:u8)->Result<Value>{anyhow::ensure!(value<=100,"非透明度必须在 0–100 之间。");let mut p=self.read()?;p.glass_opacity=value;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_appearance(&self,appearance:Appearance)->Result<Value>{let mut p=self.read()?;p.appearance=appearance;atomic_json(&self.file,&p)?;self.info()}
    fn backgrounds(&self)->Result<PathBuf>{Ok(self.file.parent().context("设置目录无效。")?.join("backgrounds"))}
    fn discard_image(&self,name:Option<String>){if let Some(name)=name{if let Ok(directory)=self.backgrounds(){let _=std::fs::remove_file(directory.join(name));}}}
    pub fn save_background_color(&self,color:Option<String>)->Result<Value>{
        anyhow::ensure!(color.as_deref().is_none_or(valid_color),"请选择有效的背景颜色。");
        let mut p=self.read()?;let old=p.background_image.take();p.background_color=color;atomic_json(&self.file,&p)?;self.discard_image(old);self.info()
    }
    pub fn save_background_image(&self,source:&std::path::Path)->Result<Value>{
        let bytes=image_bytes(source)?;let extension=image_extension(&bytes).unwrap();
        let mut p=self.read()?;let old=p.background_image.clone();let directory=self.backgrounds()?;std::fs::create_dir_all(&directory)?;
        let name=format!("{}.{}",uuid::Uuid::new_v4().simple(),extension);let target=directory.join(&name);
        std::fs::write(&target,bytes)?;p.background_image=Some(name);
        if let Err(error)=atomic_json(&self.file,&p){let _=std::fs::remove_file(&target);return Err(error);}
        self.discard_image(old);self.info()
    }
    pub fn remove_background_image(&self)->Result<Value>{let mut p=self.read()?;let old=p.background_image.take();atomic_json(&self.file,&p)?;self.discard_image(old);self.info()}
    pub fn background_data(&self)->Result<Option<String>>{
        let Some(name)=self.read()?.background_image else{return Ok(None);};
        let bytes=image_bytes(&self.backgrounds()?.join(name))?;
        let mime=if image_extension(&bytes)==Some("jpg"){"jpeg"}else{image_extension(&bytes).unwrap()};
        Ok(Some(format!("data:image/{};base64,{}",mime,base64::engine::general_purpose::STANDARD.encode(bytes))))
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn glass_opacity_defaults_to_current_material_and_preserves_zero(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();let store=PreferenceStore{file:root.join("settings.json")};
        assert_eq!(store.info().unwrap()["glassOpacity"],100);
        std::fs::write(&store.file,b"{}").unwrap();assert_eq!(store.read().unwrap().glass_opacity,100);
        for value in [0,50,100]{store.save_glass_opacity(value).unwrap();assert_eq!(store.read().unwrap().glass_opacity,value);}
        let saved=std::fs::read(&store.file).unwrap();assert!(store.save_glass_opacity(101).is_err());assert_eq!(std::fs::read(&store.file).unwrap(),saved);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn background_uses_managed_copies_and_preserves_source_and_other_preferences(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();
        let store=PreferenceStore{file:root.join("settings.json")};store.save_environment_source(EnvironmentSource::Board).unwrap();store.save_background_color(Some("#123456".into())).unwrap();
        let source=root.join("source.png");let bytes=b"\x89PNG\r\n\x1a\nsynthetic fixture";std::fs::write(&source,bytes).unwrap();
        store.save_background_image(&source).unwrap();let name=store.read().unwrap().background_image.unwrap();assert!(valid_image_name(&name));assert_ne!(source,store.backgrounds().unwrap().join(&name));
        assert_eq!(std::fs::read(&source).unwrap(),bytes);assert!(store.background_data().unwrap().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(store.info().unwrap()["environmentSource"],"board");
        store.remove_background_image().unwrap();assert!(!store.backgrounds().unwrap().join(name).exists());assert_eq!(store.info().unwrap()["backgroundColor"],"#123456");
        let saved=std::fs::read(&store.file).unwrap();assert!(store.save_background_color(Some("url(bad)".into())).is_err());assert_eq!(std::fs::read(&store.file).unwrap(),saved);
        std::fs::write(&source,b"not an image").unwrap();assert!(store.save_background_image(&source).is_err());assert_eq!(std::fs::read(&store.file).unwrap(),saved);
        store.save_background_color(None).unwrap();assert!(store.background_data().unwrap().is_none());
        assert!(!valid_image_name("../../secret.png"));assert!(!valid_image_name("1234.svg"));std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn appearance_is_saved_and_other_settings_are_preserved(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();
        let store=PreferenceStore{file:root.join("settings.json")};assert_eq!(store.info().unwrap()["appearance"],"light");
        store.save_environment_source(EnvironmentSource::Board).unwrap();
        for appearance in [Appearance::Dark,Appearance::System,Appearance::Light]{store.save_appearance(appearance).unwrap();assert_eq!(store.read().unwrap().appearance,appearance);assert_eq!(store.info().unwrap()["environmentSource"],"board");}
        assert!(serde_json::from_value::<Appearance>(json!("invalid")).is_err());
        std::fs::write(&store.file,b"{broken").unwrap();assert!(store.save_appearance(Appearance::Dark).is_err());assert_eq!(std::fs::read(&store.file).unwrap(),b"{broken");std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn environment_source_defaults_migrates_and_preserves_other_preferences(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();let store=PreferenceStore{file:root.join("settings.json")};
        assert_eq!(store.info().unwrap()["environmentSource"],"computer");
        std::fs::write(&store.file,br#"{"language":"en","theme":"green","desktopMode":"economy"}"#).unwrap();
        assert_eq!(store.read().unwrap().environment_source,EnvironmentSource::Computer);
        store.save_environment_source(EnvironmentSource::Board).unwrap();
        assert_eq!(store.read().unwrap().environment_source,EnvironmentSource::Board);
        assert_eq!(store.info().unwrap()["theme"],"green");assert_eq!(store.info().unwrap()["desktopMode"],"economy");
        store.save_language(Language::Chinese).unwrap();assert_eq!(store.info().unwrap()["environmentSource"],"board");
        assert!(serde_json::from_value::<EnvironmentSource>(json!("invalid")).is_err());
        std::fs::write(&store.file,b"{broken").unwrap();assert!(store.save_environment_source(EnvironmentSource::Computer).is_err());
        assert_eq!(std::fs::read(&store.file).unwrap(),b"{broken");std::fs::remove_dir_all(root).unwrap();
    }
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

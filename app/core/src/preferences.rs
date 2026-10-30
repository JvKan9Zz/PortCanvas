use crate::profiles::atomic_json;
use anyhow::{Context,Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::path::PathBuf;
use std::io::{Read,Write};

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
    #[serde(default,rename="backgroundCrop",skip_serializing_if="Option::is_none")]
    pub background_crop:Option<BackgroundCrop>,
    #[serde(default,rename="backgroundVideo",skip_serializing_if="Option::is_none")]
    pub background_video:Option<String>,
    #[serde(default,rename="backgroundVolume")]
    pub background_volume:u8,
    #[serde(default="default_glass_opacity",rename="glassOpacity")]
    pub glass_opacity:u8,
}
fn default_glass_opacity()->u8{100}
impl Default for Preferences{
    fn default()->Self{Self{language:Language::default(),theme:Theme::default(),desktop_mode:Default::default(),environment_source:EnvironmentSource::default(),appearance:Appearance::default(),background_color:None,background_image:None,background_crop:None,background_video:None,background_volume:0,glass_opacity:100}}
}
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq)]
pub struct BackgroundCrop{pub x:f64,pub y:f64,pub width:f64,pub height:f64}
impl BackgroundCrop{
    fn valid(&self)->bool{[self.x,self.y,self.width,self.height].iter().all(|n|n.is_finite())&&self.x>=0.0&&self.y>=0.0&&self.width>0.0&&self.height>0.0&&self.x+self.width<=1.000000001&&self.y+self.height<=1.000000001}
}
pub struct BackgroundDraft{pub id:String,pub path:PathBuf,pub video:bool}
impl Drop for BackgroundDraft{fn drop(&mut self){let _=std::fs::remove_file(&self.path);}}
impl BackgroundDraft{
    pub fn open(source:&std::path::Path,directory:&std::path::Path,video:bool,mut progress:impl FnMut(u64,u64)->Result<()>)->Result<Self>{
        let mut input=std::fs::File::open(source)?;let total=input.metadata()?.len();let mut prefix=[0;32];let count=input.read(&mut prefix)?;
        let extension=media_extension(&prefix[..count],video).context(if video{"请选择 MP4 或 WebM 视频。"}else{"请选择 PNG、JPEG 或 WebP 图片。"})?;
        std::fs::create_dir_all(directory)?;let id=uuid::Uuid::new_v4().simple().to_string();let path=directory.join(format!("draft-{id}.{extension}"));
        let mut output=std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;let draft=Self{path,id,video};output.write_all(&prefix[..count])?;let mut copied=count as u64;progress(copied,total)?;
        let mut buffer=[0;65536];loop{let count=input.read(&mut buffer)?;if count==0{break;}output.write_all(&buffer[..count])?;copied+=count as u64;progress(copied,total)?;}output.sync_all()?;Ok(draft)
    }
    pub fn commit(&self,store:&PreferenceStore,crop:BackgroundCrop)->Result<Value>{
        anyhow::ensure!(crop.valid(),"请选择有效的裁剪范围。");let mut p=store.read()?;let old_image=p.background_image.take();let old_video=p.background_video.take();
        let name=format!("{}.{}",self.id,self.path.extension().unwrap().to_string_lossy());let target=store.backgrounds()?.join(&name);std::fs::rename(&self.path,&target)?;
        if self.video{p.background_video=Some(name);}else{p.background_image=Some(name);}p.background_crop=Some(crop);
        if let Err(error)=atomic_json(&store.file,&p){std::fs::rename(&target,&self.path).context("设置保存失败，背景草稿恢复失败。")?;return Err(error);}
        store.discard_image(old_image);store.discard_image(old_video);store.info()
    }
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
fn valid_image_name(value:&str)->bool{valid_media_name(value,false)}
fn valid_media_name(value:&str,video:bool)->bool{
    value.rsplit_once('.').is_some_and(|(stem,extension)|stem.len()==32&&stem.bytes().all(|b|b.is_ascii_hexdigit())&&if video{matches!(extension,"mp4"|"webm")}else{matches!(extension,"png"|"jpg"|"webp")})
}
fn image_extension(bytes:&[u8])->Option<&'static str>{
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n"){Some("png")}
    else if bytes.starts_with(b"\xff\xd8\xff"){Some("jpg")}
    else if bytes.len()>=12&&&bytes[..4]==b"RIFF"&&&bytes[8..12]==b"WEBP"{Some("webp")}else{None}
}
fn media_extension(bytes:&[u8],video:bool)->Option<&'static str>{
    if !video{return image_extension(bytes);}
    if bytes.len()>=12&&&bytes[4..8]==b"ftyp"{Some("mp4")}else if bytes.starts_with(b"\x1a\x45\xdf\xa3"){Some("webm")}else{None}
}
impl PreferenceStore {
    pub fn read(&self)->Result<Preferences>{
        if !self.file.exists(){return Ok(Preferences::default());}
        let p:Preferences=serde_json::from_slice(&std::fs::read(&self.file)?).context("设置读取失败，原文件保留。")?;
        anyhow::ensure!(p.glass_opacity<=100,"非透明度设置无效，原设置保留。");
        anyhow::ensure!(p.background_volume<=100,"背景音量无效，原设置保留。");
        anyhow::ensure!(p.background_video.as_deref().is_none_or(|name|valid_media_name(name,true)),"背景视频配置无效，原设置保留。");
        anyhow::ensure!(p.background_image.is_none()||p.background_video.is_none(),"背景配置无效，原设置保留。");
        anyhow::ensure!(p.background_color.as_deref().is_none_or(valid_color),"背景颜色配置无效，原设置保留。");
        anyhow::ensure!(p.background_image.as_deref().is_none_or(valid_image_name),"背景图片配置无效，原设置保留。");
        anyhow::ensure!(p.background_crop.is_none_or(|crop|crop.valid()),"背景裁剪范围无效，原设置保留。");Ok(p)
    }
    pub fn language(&self)->Language {self.read().map(|p|p.language).unwrap_or_default()}
    pub fn info(&self)->Result<Value>{let p=self.read()?;Ok(json!({"language":p.language,"theme":p.theme,"appearance":p.appearance,"backgroundColor":p.background_color,"backgroundImage":p.background_image,"backgroundCrop":p.background_crop,"backgroundVideo":p.background_video,"backgroundVolume":p.background_volume,"glassOpacity":p.glass_opacity,"desktopMode":p.desktop_mode,"environmentSource":p.environment_source,"version":env!("CARGO_PKG_VERSION"),"repository":REPOSITORY,"updatesSupported":cfg!(windows)}))}
    pub fn save_language(&self,language:Language)->Result<Value>{let mut p=self.read()?;p.language=language;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_theme(&self,theme:Theme)->Result<Value>{let mut p=self.read()?;p.theme=theme;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_desktop_mode(&self,mode:crate::desktop_settings::DesktopMode)->Result<Value>{let mut p=self.read()?;p.desktop_mode=mode;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_environment_source(&self,source:EnvironmentSource)->Result<Value>{let mut p=self.read()?;p.environment_source=source;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_glass_opacity(&self,value:u8)->Result<Value>{anyhow::ensure!(value<=100,"非透明度必须在 0–100 之间。");let mut p=self.read()?;p.glass_opacity=value;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_appearance(&self,appearance:Appearance)->Result<Value>{let mut p=self.read()?;p.appearance=appearance;atomic_json(&self.file,&p)?;self.info()}
    pub fn save_background_volume(&self,value:u8)->Result<Value>{anyhow::ensure!(value<=100,"背景音量须为 0–100。");let mut p=self.read()?;p.background_volume=value;atomic_json(&self.file,&p)?;self.info()}
    pub fn backgrounds(&self)->Result<PathBuf>{Ok(self.file.parent().context("设置目录无效。")?.join("backgrounds"))}
    pub fn cleanup_drafts(&self)->Result<()>{let directory=self.backgrounds()?;if directory.is_dir(){for entry in std::fs::read_dir(directory)?{let entry=entry?;let name=entry.file_name();let name=name.to_string_lossy();if name.strip_prefix("draft-").is_some_and(|name|valid_media_name(name,false)||valid_media_name(name,true)){std::fs::remove_file(entry.path())?;}}}Ok(())}
    fn discard_image(&self,name:Option<String>){if let Some(name)=name{if let Ok(directory)=self.backgrounds(){let _=std::fs::remove_file(directory.join(name));}}}
    pub fn save_background_color(&self,color:Option<String>)->Result<Value>{
        anyhow::ensure!(color.as_deref().is_none_or(valid_color),"请选择有效的背景颜色。");
        let mut p=self.read()?;let old=p.background_image.take();let video=p.background_video.take();p.background_color=color;p.background_crop=None;atomic_json(&self.file,&p)?;self.discard_image(old);self.discard_image(video);self.info()
    }
    pub fn remove_background_image(&self)->Result<Value>{let mut p=self.read()?;let old=p.background_image.take();let video=p.background_video.take();p.background_crop=None;atomic_json(&self.file,&p)?;self.discard_image(old);self.discard_image(video);self.info()}
    pub fn background_path(&self)->Result<Option<PathBuf>>{let p=self.read()?;p.background_image.or(p.background_video).map(|name|Ok(self.backgrounds()?.join(name))).transpose()}

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
    #[test]fn background_drafts_are_file_backed_unlimited_and_preserve_source(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();let store=PreferenceStore{file:root.join("settings.json")};
        store.save_environment_source(EnvironmentSource::Board).unwrap();store.save_background_color(Some("#123456".into())).unwrap();let source=root.join("source.png");let mut bytes=b"\x89PNG\r\n\x1a\n".to_vec();bytes.resize(11*1024*1024,42);std::fs::write(&source,&bytes).unwrap();
        let before=std::fs::read(&store.file).unwrap();let draft=BackgroundDraft::open(&source,&store.backgrounds().unwrap(),false,|_,_|Ok(())).unwrap();assert_eq!(std::fs::read(&store.file).unwrap(),before);assert_eq!(std::fs::read(&draft.path).unwrap(),bytes);
        std::fs::write(&source,b"source changed after preview").unwrap();let crop=BackgroundCrop{x:0.1,y:0.2,width:0.8,height:0.6};draft.commit(&store,crop).unwrap();assert!(!draft.path.exists());
        assert_eq!(std::fs::read(store.background_path().unwrap().unwrap()).unwrap(),bytes);assert_eq!(std::fs::read(&source).unwrap(),b"source changed after preview");assert_eq!(store.read().unwrap().background_crop,Some(crop));assert_eq!(store.info().unwrap()["environmentSource"],"board");
        let saved=std::fs::read(&store.file).unwrap();std::fs::write(&source,&bytes).unwrap();let canceled=BackgroundDraft::open(&source,&store.backgrounds().unwrap(),false,|_,_|Ok(())).unwrap();let path=canceled.path.clone();drop(canceled);assert!(!path.exists());assert_eq!(std::fs::read(&store.file).unwrap(),saved);
        assert!(BackgroundDraft::open(&source,&store.backgrounds().unwrap(),false,|_,_|anyhow::bail!("canceled")).is_err());assert_eq!(std::fs::read_dir(store.backgrounds().unwrap()).unwrap().count(),1);
        let draft=BackgroundDraft::open(&source,&store.backgrounds().unwrap(),false,|_,_|Ok(())).unwrap();for invalid in [BackgroundCrop{width:0.0,..crop},BackgroundCrop{x:-0.1,..crop},BackgroundCrop{x:0.9,..crop},BackgroundCrop{y:f64::NAN,..crop}]{assert!(draft.commit(&store,invalid).is_err());assert_eq!(std::fs::read(&store.file).unwrap(),saved);}drop(draft);
        store.remove_background_image().unwrap();assert!(store.background_path().unwrap().is_none());assert!(store.read().unwrap().background_crop.is_none());assert!(source.exists());assert!(!valid_image_name("../../secret.png"));std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn video_and_volume_replace_only_the_background_and_keep_legacy_images(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();let store=PreferenceStore{file:root.join("settings.json")};assert_eq!(store.info().unwrap()["backgroundVolume"],0);let source=root.join("clip.mp4");std::fs::write(&source,b"\0\0\0\x18ftypisomfixture").unwrap();
        let draft=BackgroundDraft::open(&source,&store.backgrounds().unwrap(),true,|_,_|Ok(())).unwrap();draft.commit(&store,BackgroundCrop{x:0.0,y:0.0,width:1.0,height:1.0}).unwrap();assert!(store.read().unwrap().background_video.is_some());assert!(store.read().unwrap().background_image.is_none());
        for volume in [0,50,100]{store.save_background_volume(volume).unwrap();assert_eq!(store.info().unwrap()["backgroundVolume"],volume);}assert!(store.save_background_volume(101).is_err());
        let media=store.background_path().unwrap().unwrap();store.save_background_color(None).unwrap();assert!(!media.exists());assert!(source.exists());assert_eq!(store.info().unwrap()["backgroundVolume"],100);std::fs::remove_dir_all(root).unwrap();
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

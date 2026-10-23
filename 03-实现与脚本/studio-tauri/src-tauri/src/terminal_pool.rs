use crate::{logs::LogService, profiles::atomic_json, session::DeviceManager, terminal::TerminalService};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::{atomic::AtomicU64, Arc, Mutex}};

#[derive(Clone, Serialize, Deserialize)]
pub struct Tab { pub key: String, pub number: u32 }
impl Tab {
    fn main() -> Self { Self { key:"main".into(), number:1 } }
    pub fn title(&self)->String {format!("终端 {}",self.number)}
}
#[derive(Default, Serialize, Deserialize)]
struct Catalog { devices: BTreeMap<String, Vec<Tab>> }
pub fn validate_key(key:&str)->Result<()> {
    if key!="main" && !(key.len()==36 && uuid::Uuid::parse_str(key).is_ok()) {bail!("终端标识无效。");} Ok(())
}
impl Catalog {
    fn read(file:&PathBuf)->Result<Self> {
        if !file.exists(){return Ok(Self::default());}
        anyhow::ensure!(fs::metadata(file)?.len()<=1024*1024,"终端列表超过1MiB，原文件保留。");
        let data:Self=serde_json::from_slice(&fs::read(file)?).context("终端列表读取失败，原文件保留。")?;
        for (device,tabs) in &data.devices {
            anyhow::ensure!(regex::Regex::new(r"^wcs-[a-f0-9]{16}$")?.is_match(device),"设备终端标识无效。");
            let mut keys=std::collections::BTreeSet::new();let mut numbers=std::collections::BTreeSet::new();
            for tab in tabs {validate_key(&tab.key)?;anyhow::ensure!(tab.number>0 && (tab.key!="main"||tab.number==1) && keys.insert(&tab.key) && numbers.insert(tab.number),"终端列表存在重复或无效标识。");}
        } Ok(data)
    }
    fn tabs(&self,device:&str)->Vec<Tab> {self.devices.get(device).cloned().unwrap_or_else(||vec![Tab::main()])}
    fn add(&mut self,device:&str)->Result<Tab> {
        let tabs=self.devices.entry(device.into()).or_insert_with(||vec![Tab::main()]);
        if tabs.is_empty(){let tab=Tab::main();tabs.push(tab.clone());return Ok(tab);}
        let number=tabs.iter().map(|tab|tab.number).max().unwrap_or(1).checked_add(1).context("终端编号已用尽。")?;
        let tab=Tab{key:uuid::Uuid::new_v4().to_string(),number};tabs.push(tab.clone());Ok(tab)
    }
}
#[derive(Clone)]
pub struct TerminalPool {
    devices: DeviceManager,
    logs: LogService,
    namespace: Option<String>,
    file: PathBuf,
    catalog_gate: Arc<Mutex<()>>,
    entries: Arc<Mutex<BTreeMap<String,TerminalService>>>,
    ids: Arc<AtomicU64>,
}
impl TerminalPool {
    pub fn new(devices:DeviceManager,logs:LogService,namespace:Option<String>,file:PathBuf)->Self {
        Self{devices,logs,namespace,file,catalog_gate:Arc::new(Mutex::new(())),entries:Arc::new(Mutex::new(BTreeMap::new())),ids:Arc::new(AtomicU64::new(0))}
    }
    fn device(&self,expected:Option<&str>)->Result<String>{
        let device=self.devices.connection()?.profile.terminal_name();
        if let Some(expected)=expected.filter(|s|!s.is_empty()){anyhow::ensure!(expected==device,"设备连接已变化，请重新选择终端。");}Ok(device)
    }
    pub fn get(&self,key:&str,expected:Option<&str>)->Result<TerminalService>{
        validate_key(key)?;let device=self.device(expected)?;
        let _gate=self.catalog_gate.lock().unwrap();let catalog=Catalog::read(&self.file)?;
        anyhow::ensure!(catalog.tabs(&device).iter().any(|tab|tab.key==key),"终端已关闭，请重新选择。");
        let mut entries=self.entries.lock().unwrap();
        Ok(entries.entry(format!("{device}:{key}")).or_insert_with(||TerminalService::new(self.devices.clone(),self.namespace.clone(),Some(self.logs.clone()),key.into(),device,self.ids.clone())).clone())
    }
    pub fn list(&self)->Result<Value>{
        let Ok(device)=self.device(None) else {return Ok(json!({"deviceKey":"","tabs":[{"key":"main","number":1,"title":"终端 1","state":{"key":"main","id":0,"phase":"disconnected","message":"连接设备后可打开终端。"}}]}));};
        let tabs={let _gate=self.catalog_gate.lock().unwrap();Catalog::read(&self.file)?.tabs(&device)};
        let entries=self.entries.lock().unwrap();
        Ok(json!({"deviceKey":device,"tabs":tabs.iter().map(|tab|json!({"key":tab.key,"number":tab.number,"title":tab.title(),"state":entries.get(&format!("{device}:{}",tab.key)).map(TerminalService::snapshot).unwrap_or_else(||json!({"key":tab.key,"deviceKey":device,"id":0,"phase":"disconnected","message":"点击连接或恢复此终端。"}))})).collect::<Vec<_>>()}))
    }
    pub fn snapshot(&self,key:&str)->Result<Value>{
        validate_key(key)?;
        if self.devices.connection().is_err(){return Ok(json!({"key":key,"id":0,"phase":"disconnected","message":"设备尚未连接。"}));}
        Ok(self.get(key,None)?.snapshot())
    }
    pub fn title(&self,key:&str)->Result<String>{
        let device=self.device(None)?;let _gate=self.catalog_gate.lock().unwrap();
        Catalog::read(&self.file)?.tabs(&device).iter().find(|tab|tab.key==key).map(Tab::title).context("终端已关闭。")
    }
    pub fn create(&self,expected:Option<&str>)->Result<Value>{
        let device=self.device(expected)?;
        let tab={let _gate=self.catalog_gate.lock().unwrap();let mut catalog=Catalog::read(&self.file)?;let tab=catalog.add(&device)?;atomic_json(&self.file,&catalog)?;tab};
        Ok(json!({"key":tab.key,"catalog":self.list()?}))
    }
    pub fn stop_all(&self){
        let entries={let mut entries=self.entries.lock().unwrap();std::mem::take(&mut *entries)};
        for service in entries.values(){service.stop();}
    }
    pub fn stop(&self,key:&str)->Result<()> {
        validate_key(key)?;if self.devices.connection().is_ok(){self.get(key,None)?.stop();}Ok(())
    }
    pub fn ack(&self,key:&str,id:u64,sequence:u64){
        for service in self.entries.lock().unwrap().values(){if service.key()==key{service.ack(id,sequence);}}
    }
    pub async fn remove(&self,key:&str,expected:Option<&str>)->Result<Value>{
        let device=self.device(expected)?;let service=self.get(key,Some(&device))?;
        service.reset(true).await?;
        {let _gate=self.catalog_gate.lock().unwrap();let mut catalog=Catalog::read(&self.file)?;
        let mut tabs=catalog.tabs(&device);tabs.retain(|tab|tab.key!=key);catalog.devices.insert(device.clone(),tabs);atomic_json(&self.file,&catalog)?;}
        self.entries.lock().unwrap().remove(&format!("{device}:{key}"));self.logs.forget_terminal(key);self.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_persists_independent_tabs_and_rejects_invalid_targets(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());let file=root.join("terminals.json");
        let mut catalog=Catalog::read(&file).unwrap();let device="wcs-fedcba9876543210";
        let a=catalog.add(device).unwrap();let b=catalog.add(device).unwrap();assert_ne!(a.key,b.key);assert_eq!((a.number,b.number),(2,3));
        atomic_json(&file,&catalog).unwrap();let restored=Catalog::read(&file).unwrap();assert_eq!(restored.tabs(device)[1].key,a.key);
        assert_eq!(restored.tabs("wcs-1111111111111111").len(),1);assert!(validate_key("main;kill-server").is_err());
        catalog.devices.get_mut(device).unwrap().push(a);atomic_json(&file,&catalog).unwrap();assert!(Catalog::read(&file).is_err());
    }
    #[test]
    fn an_explicitly_empty_catalog_stays_closed_until_user_creates(){
        let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());let file=root.join("terminals.json");let device="wcs-fedcba9876543210";
        let mut catalog=Catalog::default();catalog.devices.insert(device.into(),vec![]);atomic_json(&file,&catalog).unwrap();
        let mut restored=Catalog::read(&file).unwrap();assert!(restored.tabs(device).is_empty());assert_eq!(restored.add(device).unwrap().key,"main");assert_eq!(restored.tabs(device).len(),1);
    }
}

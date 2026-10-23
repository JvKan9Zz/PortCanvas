#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::PathBuf,sync::{Arc,Mutex}};
use tauri::Manager;
use serde_json::{json,Value};
use studio_core::{profiles::{Profile,ProfileStore},ssh::{HostCheck,SshConnection}};
struct AppState { profiles:Mutex<ProfileStore> }

#[tauri::command]
async fn studio_call(window:tauri::WebviewWindow,state:tauri::State<'_,AppState>,action:String,args:Vec<Value>)->Result<Value,String>{
    let result: anyhow::Result<Value>=async{
        anyhow::ensure!(window.label()=="main","Invalid caller");
        let url=window.url()?;anyhow::ensure!(matches!(url.scheme(),"tauri"|"http"|"https")&&matches!(url.host_str(),Some("localhost"|"tauri.localhost")),"Invalid origin");
        match action.as_str(){
            "profiles:list"=>Ok(serde_json::to_value(state.profiles.lock().unwrap().read()?.devices)?),
            "profiles:save"=>Ok(serde_json::to_value(state.profiles.lock().unwrap().save(serde_json::from_value(args.first().cloned().unwrap_or(Value::Null))?)?)?),
            "window:fullscreen"=>{let full=!window.is_fullscreen()?;window.set_fullscreen(full)?;Ok(json!(full))},
            "compat:probe"=>{
                let profile:Profile=serde_json::from_value(args.first().cloned().unwrap_or(Value::Null))?;
                let password=zeroize::Zeroizing::new(args.get(1).and_then(Value::as_str).ok_or_else(||anyhow::anyhow!("请输入密码。"))?.to_string());
                let expected=state.profiles.lock().unwrap().read()?.host_keys.get(&profile.address()).cloned().ok_or_else(||anyhow::anyhow!("M1 原型只连接已验证的测试设备。"))?;
                let check:HostCheck=Arc::new(move|actual|{let expected=expected.clone();Box::pin(async move{anyhow::ensure!(expected==actual,"主机指纹不匹配。");Ok(true)})});
                let connection=SshConnection::connect(profile,password,check,tokio_util::sync::CancellationToken::new()).await?;
                let sftp=connection.sftp().await?;let _home=sftp.canonicalize(".").await?;sftp.close().await?;connection.disconnect().await;
                Ok(json!({"ssh":true,"sftp":true,"runtime":"Rust"}))
            },
            _=>anyhow::bail!("M1 原型未提供此操作。")
        }
    }.await;result.map_err(|e|e.to_string())
}
fn main(){
    tauri::Builder::default().setup(|app|{
        let directory=std::env::var_os("WCS_DATA_DIR").map(PathBuf::from).unwrap_or(app.path().app_data_dir()?);
        if !directory.is_absolute(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,"应用数据目录须为绝对路径").into());}
        app.manage(AppState{profiles:Mutex::new(ProfileStore{directory})});Ok(())
    }).invoke_handler(tauri::generate_handler![studio_call]).run(tauri::generate_context!()).expect("Tauri desktop application");
}

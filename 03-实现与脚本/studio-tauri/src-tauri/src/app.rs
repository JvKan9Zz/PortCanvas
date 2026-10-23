use crate::{
    desktop::DesktopService,
    profiles::{Profile, ProfileStore},
    session::{DeviceManager, EventSink},
    ssh::HostCheck,
};
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager};
pub struct AppState {
    pub profiles: Arc<Mutex<ProfileStore>>,
    pub devices: DeviceManager,
    pub desktop: DesktopService,
    pub quitting: AtomicBool,
}
pub fn argument<T: DeserializeOwned>(args: &[Value], index: usize) -> Result<T> {
    serde_json::from_value(args.get(index).cloned().unwrap_or(Value::Null))
        .context("操作参数无效。")
}

pub async fn confirm(window: tauri::WebviewWindow, title: String, message: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let title = title
                .replace('\0', "�")
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let text = message
                .replace('\0', "�")
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let parent = window
                .hwnd()
                .map(|h| h.0 as *mut std::ffi::c_void)
                .unwrap_or(std::ptr::null_mut());
            unsafe {
                MessageBoxW(
                    parent,
                    text.as_ptr(),
                    title.as_ptr(),
                    MB_OKCANCEL | MB_DEFBUTTON2 | MB_ICONQUESTION | MB_SETFOREGROUND,
                ) == IDOK
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (window, title, message);
            false
        }
    })
    .await
    .context("确认窗口未正常结束。")
}

#[tauri::command]
async fn studio_call(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    action: String,
    mut args: Vec<Value>,
) -> std::result::Result<Value, String> {
    let result:Result<Value>=async{
        anyhow::ensure!(window.label()=="main","Invalid caller");let url=window.url()?;anyhow::ensure!(matches!(url.scheme(),"tauri"|"http"|"https")&&matches!(url.host_str(),Some("localhost"|"tauri.localhost")),"Invalid origin");
        match action.as_str(){
            "profiles:list"=>Ok(serde_json::to_value(state.profiles.lock().unwrap().read()?.devices)?),
            "profiles:save"=>{anyhow::ensure!(state.devices.snapshot()["ssh"]!="connected","请先断开设备再修改配置。");Ok(serde_json::to_value(state.profiles.lock().unwrap().save(argument(&args,0)?)?)?)},
            "profiles:remove"=>{let id:String=argument(&args,0)?;if !confirm(window.clone(),"移除设备配置".into(),"仅移除本机连接配置，是否继续？".into()).await?{return Ok(json!(false));}if state.devices.snapshot()["deviceId"]==id{state.devices.disconnect().await;}
                let store=state.profiles.lock().unwrap();let mut data=store.read()?;let removed=data.devices.iter().find(|p|p.id==id).cloned();data.devices.retain(|p|p.id!=id);if let Some(removed)=removed{if !data.devices.iter().any(|p|p.address()==removed.address()){data.host_keys.remove(&removed.address());}}store.write(&data)?;Ok(json!(true))},
            "device:state"=>Ok(state.devices.snapshot()),
            "device:disconnect"=>{state.devices.disconnect().await;state.desktop.stop(None).await;Ok(state.devices.snapshot())},
            "device:connect"=>{
                let profile:Profile=argument(&args,0)?;let password=match args.get_mut(1).map(Value::take){Some(Value::String(p)) if !p.is_empty()=>zeroize::Zeroizing::new(p),_=>bail!("请输入本次连接密码。")};
                let store=state.profiles.clone();let address=profile.address();let dialog=window.clone();
                state.devices.connect(profile,password,move|cancel|{let callback:HostCheck=Arc::new(move|fingerprint|{let store=store.clone();let address=address.clone();let dialog=dialog.clone();let cancel=cancel.clone();Box::pin(async move{
                    let known=store.lock().unwrap().read()?.host_keys.get(&address).cloned();
                    if let Some(known)=known{anyhow::ensure!(known==fingerprint,"主机身份已变化，已停止连接。请核实设备身份，不会自动信任新密钥。");return Ok(!cancel.is_cancelled());}
                    let accepted=confirm(dialog,"首次连接设备".into(),format!("确认这台设备的 SSH 身份：\n{address}\n{fingerprint}\n请与设备或已信任客户端核对，确认后才会保存信任记录。")).await?;
                    if !accepted||cancel.is_cancelled(){return Ok(false);}store.lock().unwrap().trust(&address,&fingerprint)?;Ok(true)
                })});callback}).await
            },
            "desktop:start"=>state.desktop.start(url.origin().ascii_serialization()).await,
            "desktop:stop"=>{state.desktop.stop(None).await;Ok(Value::Null)},
            "desktop:ready"=>{state.desktop.ready(argument(&args,0)?);Ok(Value::Null)},
            "desktop:lost"=>{let id:u64=argument(&args,0)?;state.desktop.stop(Some(id)).await;Ok(Value::Null)},
            "window:fullscreen"=>{let full=!window.is_fullscreen()?;window.set_fullscreen(full)?;Ok(json!(full))},
            _=>bail!("此操作仍在后续迁移工作包中，正式操作请使用 V0.3.5。")
        }
    }.await;
    result.map_err(|e| e.to_string())
}
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = std::env::var_os("WCS_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            if !directory.is_absolute() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "应用数据目录须为绝对路径",
                )
                .into());
            }
            let handle = app.handle().clone();
            let events: EventSink = Arc::new(move |name, data| {
                let _ = handle.emit(name, data);
            });
            let devices = DeviceManager::new(events);
            let desktop = DesktopService::new(devices.clone());
            app.manage(AppState {
                profiles: Arc::new(Mutex::new(ProfileStore { directory })),
                devices,
                desktop,
                quitting: AtomicBool::new(false),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle().clone();
                if !app
                    .state::<AppState>()
                    .quitting
                    .swap(true, Ordering::SeqCst)
                {
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<AppState>();
                        state.desktop.stop(None).await;
                        state.devices.disconnect().await;
                        app.exit(0);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![studio_call])
        .run(tauri::generate_context!())
        .expect("Tauri desktop application");
}

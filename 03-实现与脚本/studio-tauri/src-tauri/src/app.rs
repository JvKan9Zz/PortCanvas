use crate::{
    desktop::DesktopService,
    files::{Confirmation, FileService, Side, Transfer},
    profiles::{Profile, ProfileStore},
    session::{DeviceManager, EventSink},
    ssh::HostCheck,
    terminal::{OutputSink, TerminalService},
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
    pub files: FileService,
    pub terminal: TerminalService,
}
pub fn argument<T: DeserializeOwned>(args: &[Value], index: usize) -> Result<T> {
    serde_json::from_value(args.get(index).cloned().unwrap_or(Value::Null))
        .context("操作参数无效。")
}

fn validate_window(window: &tauri::WebviewWindow) -> Result<String> {
    anyhow::ensure!(window.label() == "main", "Invalid caller");
    let url = window.url()?;
    anyhow::ensure!(
        matches!(url.scheme(), "tauri" | "http" | "https")
            && matches!(url.host_str(), Some("localhost" | "tauri.localhost")),
        "Invalid origin"
    );
    Ok(url.origin().ascii_serialization())
}
fn optional_text(args: &[Value], index: usize) -> Result<String> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        _ => bail!("路径必须为文字。"),
    }
}
#[tauri::command]
async fn terminal_subscribe(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    cols: u32,
    rows: u32,
    channel: tauri::ipc::Channel<tauri::ipc::Response>,
) -> std::result::Result<Value, String> {
    let operation: Result<Value> = async {
        validate_window(&window)?;
        let sink: OutputSink = Arc::new(move |bytes| {
            channel.send(tauri::ipc::Response::new(bytes))?;
            Ok(())
        });
        state.terminal.start(cols, rows, sink).await
    }
    .await;
    operation.map_err(|e| e.to_string())
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
        let origin=validate_window(&window)?;
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
            "desktop:start"=>state.desktop.start(origin).await,
            "desktop:stop"=>{state.desktop.stop(None).await;Ok(Value::Null)},
            "desktop:ready"=>{state.desktop.ready(argument(&args,0)?);Ok(Value::Null)},
            "desktop:lost"=>{let id:u64=argument(&args,0)?;state.desktop.stop(Some(id)).await;Ok(Value::Null)},
            "terminal:state"=>Ok(state.terminal.snapshot()),
            "terminal:stop"=>{state.terminal.stop();Ok(Value::Null)},
            "terminal:write"=>{state.terminal.write(argument(&args,0)?,argument(&args,1)?).await?;Ok(Value::Null)},
            "terminal:resize"=>{state.terminal.resize(argument(&args,0)?,argument(&args,1)?,argument(&args,2)?).await?;Ok(Value::Null)},
            "terminal:ack"=>{state.terminal.ack(argument(&args,0)?,argument(&args,1)?);Ok(Value::Null)},
            "files:state"=>Ok(state.files.snapshot()),
            "files:cancel"=>{state.files.cancel();Ok(Value::Null)},
            "files:list"=>state.files.list(argument::<Side>(&args,0)?,optional_text(&args,1)?).await,
            "files:mkdir"=>Ok(json!(state.files.mkdir(argument(&args,0)?,argument(&args,1)?,argument(&args,2)?).await?)),
            "files:choose-local"=>{let directory=tokio::task::spawn_blocking(move||rfd::FileDialog::new().set_parent(&window).set_title("选择本地文件夹").pick_folder()).await?;Ok(json!(directory.map(|p|p.to_string_lossy().into_owned())))},
            "files:transfer"=>{let options:Transfer=argument(&args,0)?;let callback:Confirmation=Arc::new(move|paths|{let window=window.clone();Box::pin(async move{confirm(window,"确认覆盖文件".into(),format!("目标已有 {} 个同名文件，是否覆盖？\n{}\n取消将保留原文件。",paths.len(),paths.iter().take(8).cloned().collect::<Vec<_>>().join("\n"))).await})});state.files.transfer(options,callback).await},
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
                if name == "device-state" && data["ssh"] != "connected" {
                    if let Some(state) = handle.try_state::<AppState>() {
                        state.files.invalidate();
                        state.terminal.stop();
                    }
                }
                let _ = handle.emit(name, data);
            });
            let devices = DeviceManager::new(events);
            let desktop = DesktopService::new(devices.clone());
            let files = FileService::new(devices.clone(), app.path().document_dir()?);
            let namespace = std::env::var("WCS_SESSION_NAMESPACE")
                .ok()
                .filter(|s| !s.is_empty());
            let terminal = TerminalService::new(devices.clone(), namespace);
            app.manage(AppState {
                profiles: Arc::new(Mutex::new(ProfileStore { directory })),
                devices,
                desktop,
                files,
                terminal,
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
                        state.terminal.stop();
                        state.files.cancel();
                        state.desktop.stop(None).await;
                        state.devices.disconnect().await;
                        app.exit(0);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![studio_call, terminal_subscribe])
        .run(tauri::generate_context!())
        .expect("Tauri desktop application");
}

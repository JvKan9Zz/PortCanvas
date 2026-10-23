use crate::{
    desktop::DesktopService,
    files::{Confirmation, FileService, Side, Transfer},
    log_store::Query as LogQuery,
    logs::{LogService, Settings as LogSettings},
    migration::Plan as MigrationPlan,
    profiles::{Profile, ProfileStore},
    session::{DeviceManager, EventSink},
    ssh::HostCheck,
    terminal::OutputSink,
    terminal_pool::TerminalPool,
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
    pub terminal: TerminalPool,
    pub logs: LogService,
    pub migrating: AtomicBool,
    pub default_log_root: PathBuf,
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
fn terminal_key(args:&[Value],index:usize)->Result<String>{
    let value=optional_text(args,index)?;let key=if value.is_empty(){"main".into()}else{value};crate::terminal_pool::validate_key(&key)?;Ok(key)
}
struct MigrationGuard<'a>(&'a AtomicBool);
impl Drop for MigrationGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
fn migration_guard(state: &AppState) -> Result<MigrationGuard<'_>> {
    if state
        .migrating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        bail!("资料迁移正在进行，请稍后重试。");
    }
    Ok(MigrationGuard(&state.migrating))
}
fn migration_idle(state: &AppState) -> Result<()> {
    if state.quitting.load(Ordering::SeqCst)
        || matches!(
            state.devices.snapshot()["ssh"].as_str(),
            Some("connected" | "connecting")
        )
        || state.files.snapshot()["busy"] == true
    {
        bail!("请先断开设备并等待任务结束，再迁移资料。");
    }
    state.logs.can_change_root()
}
async fn import_profiles(window: tauri::WebviewWindow, state: &AppState) -> Result<Value> {
    let _guard = migration_guard(state)?;
    migration_idle(state)?;
    let picker = window.clone();
    let source = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_parent(&picker)
            .set_title("选择旧版软件的data目录（含devices.json）")
            .pick_folder()
    })
    .await?;
    let Some(source) = source else {
        return Ok(json!({"canceled":true}));
    };
    let target = state.profiles.lock().unwrap().directory.clone();
    let current = state.logs.settings();
    let legacy = window
        .app_handle()
        .path()
        .document_dir()?
        .join("Wheel Combat Studio/Logs");
    let plan = tokio::task::spawn_blocking(move || {
        MigrationPlan::prepare(source, target, current, legacy)
    })
    .await??;
    let summary = &plan.preview;
    let message=format!("导入旧版资料？\n来源：{}\n目标：{}\n新增设备：{}；受信指纹：{}\n日志位置：{}\n{}\n将备份目标资料；不移动日志，不修改旧版目录。",summary["source"].as_str().unwrap_or_default(),summary["target"].as_str().unwrap_or_default(),summary["addedDevices"],summary["addedHostKeys"],summary["logRoot"].as_str().unwrap_or_default(),summary["warning"].as_str().unwrap_or_default());
    if !confirm(window, "导入旧版资料".into(), message).await? {
        return Ok(json!({"canceled":true}));
    }
    migration_idle(state)?;
    let settings = plan.settings.clone();
    let profiles = state.profiles.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _lock = profiles.lock().unwrap();
        plan.apply()
    })
    .await??;
    state.logs.replace_settings(settings, false).await?;
    Ok(result)
}
async fn rollback_profiles(window: tauri::WebviewWindow, state: &AppState) -> Result<Value> {
    let _guard = migration_guard(state)?;
    migration_idle(state)?;
    let target = state.profiles.lock().unwrap().directory.clone();
    let initial = target.join("migration-backups");
    let picker = window.clone();
    let backup = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_parent(&picker)
            .set_title("选择迁移备份目录（含report.json）")
            .set_directory(initial)
            .pick_folder()
    })
    .await?;
    let Some(backup) = backup else {
        return Ok(json!({"canceled":true}));
    };
    if !confirm(
        window,
        "恢复导入前资料".into(),
        format!(
            "恢复所选迁移之前的PortCanvas配置？\n{}\n当前配置会另存备份；日志文件不会删除。",
            backup.display()
        ),
    )
    .await?
    {
        return Ok(json!({"canceled":true}));
    }
    migration_idle(state)?;
    let current = target.clone();
    let profiles = state.profiles.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _lock = profiles.lock().unwrap();
        crate::migration::rollback(current, backup)
    })
    .await??;
    let file = target.join("log-settings.json");
    let settings = if file.exists() {
        serde_json::from_slice::<LogSettings>(&std::fs::read(file)?)?.validate()?
    } else {
        LogSettings {
            root: state.default_log_root.clone(),
            projects: Default::default(),
        }
    };
    state.logs.replace_settings(settings, false).await?;
    Ok(result)
}
async fn open_directory(window: tauri::WebviewWindow, path: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&path)?;
        if !path.is_dir() {
            bail!("该路径不是目录。");
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::{
                Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL,
            };
            let operation = "open".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let file = path
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let parent = window
                .hwnd()
                .map(|h| h.0 as *mut std::ffi::c_void)
                .unwrap_or(std::ptr::null_mut());
            let result = unsafe {
                ShellExecuteW(
                    parent,
                    operation.as_ptr(),
                    file.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            };
            if result as isize <= 32 {
                bail!("无法打开目录（系统状态{}）。", result as isize);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = window;
            bail!("当前交付仅支持Windows目录打开。");
        }
        Ok::<_, anyhow::Error>(())
    })
    .await?
}
#[tauri::command]
async fn terminal_subscribe(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    cols: u32,
    rows: u32,
    terminal_key: Option<String>,
    device_key: Option<String>,
    channel: tauri::ipc::Channel<tauri::ipc::Response>,
) -> std::result::Result<Value, String> {
    let operation: Result<Value> = async {
        validate_window(&window)?;
        anyhow::ensure!(
            !state.migrating.load(Ordering::SeqCst),
            "资料迁移中，请稍后。"
        );
        let sink: OutputSink = Arc::new(move |bytes| {
            channel.send(tauri::ipc::Response::new(bytes))?;
            Ok(())
        });
        state.terminal.get(terminal_key.as_deref().unwrap_or("main"),device_key.as_deref())?.start(cols, rows, sink).await
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
        if state.migrating.load(Ordering::SeqCst)&&matches!(action.as_str(),"device:connect"|"profiles:save"|"profiles:remove"|"files:transfer"|"logs:directory"|"logs:project"|"terminal:new"|"terminal:restart"|"terminal:close"){bail!("资料迁移中，请稍后重试。");}
        match action.as_str(){
            "profiles:list"=>Ok(json!(state.profiles.lock().unwrap().read()?.devices.iter().map(Profile::public_value).collect::<Vec<_>>())),
            "profiles:save"=>{anyhow::ensure!(!matches!(state.devices.snapshot()["ssh"].as_str(),Some("connected"|"connecting")),"请先断开设备再修改配置。");let profile:Profile=argument(&args,0)?;let secret=match args.get_mut(1).map(Value::take){Some(Value::String(p))=>zeroize::Zeroizing::new(p),None|Some(Value::Null)=>zeroize::Zeroizing::new(String::new()),_=>bail!("凭据参数无效。")};Ok(state.profiles.lock().unwrap().save(profile,&secret)?.public_value())},
            "profiles:choose-key"=>{let file=tokio::task::spawn_blocking(move||rfd::FileDialog::new().set_parent(&window).set_title("选择SSH私钥（OpenSSH / PEM / PPK）").add_filter("所有私钥文件",&["*"]).pick_file()).await?;Ok(json!(file.map(|p|p.to_string_lossy().into_owned())))},
            "profiles:remove"=>{let id:String=argument(&args,0)?;if !confirm(window.clone(),"移除设备配置".into(),"仅移除本机连接配置，是否继续？".into()).await?{return Ok(json!(false));}if state.devices.snapshot()["deviceId"]==id{state.devices.disconnect().await;}
                let store=state.profiles.lock().unwrap();let mut data=store.read()?;let removed=data.devices.iter().find(|p|p.id==id).cloned();data.devices.retain(|p|p.id!=id);if let Some(removed)=removed{if !data.devices.iter().any(|p|p.address()==removed.address()){data.host_keys.remove(&removed.address());}}store.write(&data)?;Ok(json!(true))},
            "device:state"=>Ok(state.devices.snapshot()),
            "device:disconnect"=>{state.devices.disconnect().await;state.desktop.stop(None).await;Ok(state.devices.snapshot())},
            "device:connect"=>{
                let mut profile:Profile=argument::<Profile>(&args,0)?.validate()?;profile.encrypted_secret=None;
                let supplied=match args.get_mut(1).map(Value::take){Some(Value::String(p))=>zeroize::Zeroizing::new(p),None|Some(Value::Null)=>zeroize::Zeroizing::new(String::new()),_=>bail!("凭据参数无效。")};
                let password=state.profiles.lock().unwrap().connection_secret(&profile,supplied)?;
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
            "terminal:list"=>state.terminal.list(),
            "terminal:new"=>state.terminal.create(Some(&optional_text(&args,0)?)),
            "terminal:state"=>state.terminal.snapshot(&terminal_key(&args,0)?),
            "terminal:stop"=>{state.terminal.stop(&terminal_key(&args,0)?)?;Ok(Value::Null)},
            "terminal:write"=>{state.terminal.get(&terminal_key(&args,2)?,None)?.write(argument(&args,0)?,argument(&args,1)?).await?;Ok(Value::Null)},
            "terminal:resize"=>{state.terminal.get(&terminal_key(&args,3)?,None)?.resize(argument(&args,0)?,argument(&args,1)?,argument(&args,2)?).await?;Ok(Value::Null)},
            "terminal:ack"=>{state.terminal.ack(&terminal_key(&args,2)?,argument(&args,0)?,argument(&args,1)?);Ok(Value::Null)},
            "terminal:restart"|"terminal:close"=>{
                let key=terminal_key(&args,0)?;let expected=optional_text(&args,1)?;let generation=state.devices.generation();
                let service=state.terminal.get(&key,Some(&expected))?;let removing=action=="terminal:close";
                let tab_title=state.terminal.title(&key)?;let title=format!("{}{tab_title}",if removing{"关闭"}else{"重启"});
                let message=format!("结束「{tab_title}」的Shell和前台程序，并{}？本地日志保留，其他终端不受影响。",if removing{"关闭此标签页"}else{"重新启动Shell"});
                if !confirm(window,title,message).await?{return Ok(json!({"canceled":true}));}
                anyhow::ensure!(generation==state.devices.generation(),"设备连接已变化，请重新操作。");
                if removing{Ok(json!({"canceled":false,"catalog":state.terminal.remove(&key,Some(&expected)).await?}))}
                else{Ok(json!({"canceled":false,"state":service.reset(false).await?}))}
            },
            "files:state"=>Ok(state.files.snapshot()),
            "files:cancel"=>{state.files.cancel();Ok(Value::Null)},
            "files:list"=>state.files.list(argument::<Side>(&args,0)?,optional_text(&args,1)?).await,
            "files:drives"=>crate::files::local_drives(),
            "files:mkdir"=>Ok(json!(state.files.mkdir(argument(&args,0)?,argument(&args,1)?,argument(&args,2)?).await?)),
            "files:choose-local"=>{let directory=tokio::task::spawn_blocking(move||rfd::FileDialog::new().set_parent(&window).set_title("选择本地文件夹").pick_folder()).await?;Ok(json!(directory.map(|p|p.to_string_lossy().into_owned())))},
            "files:transfer"=>{let options:Transfer=argument(&args,0)?;let callback:Confirmation=Arc::new(move|paths|{let window=window.clone();Box::pin(async move{confirm(window,"确认覆盖文件".into(),format!("目标已有 {} 个同名文件，是否覆盖？\n{}\n取消将保留原文件。",paths.len(),paths.iter().take(8).cloned().collect::<Vec<_>>().join("\n"))).await})});state.files.transfer(options,callback).await},
            "profiles:import"=>import_profiles(window,&state).await,
            "profiles:rollback"=>rollback_profiles(window,&state).await,
            "logs:state"=>{let key=optional_text(&args,0)?;Ok(if key.is_empty(){state.logs.snapshot()}else{crate::terminal_pool::validate_key(&key)?;state.logs.terminal_snapshot(&key)})},
            "logs:page"=>state.logs.page(argument::<LogQuery>(&args,0)?).await,
            "logs:read"=>state.logs.detail(argument(&args,0)?).await,
            "logs:project"=>{let key=terminal_key(&args,1)?;state.terminal.get(&key,None)?;state.logs.set_project_for(&key,argument(&args,0)?).await},
            "logs:directory"=>{state.logs.can_change_root()?;let initial=state.logs.settings().root;let picker=window.clone();let folder=tokio::task::spawn_blocking(move||rfd::FileDialog::new().set_parent(&picker).set_title("选择电脑本地日志目录").set_directory(initial).pick_folder()).await?;if let Some(folder)=folder{state.logs.set_root(folder).await}else{Ok(state.logs.snapshot())}},
            "logs:open-folder"=>{let id=optional_text(&args,0)?;let path=if id.is_empty(){state.logs.settings().root}else{let record=state.logs.detail(id).await?;PathBuf::from(record["directory"].as_str().context("日志目录无效")?)};open_directory(window,path).await?;Ok(Value::Null)},
            "logs:remove"=>{let ids:Vec<String>=argument(&args,0)?;let records=state.logs.removable(ids.clone()).await?;if records.is_empty(){return Ok(json!({"removed":0}));}let detail=records.iter().take(8).map(|r|format!("{} / {} / {}",r["project"]["name"].as_str().unwrap_or_default(),r["day"].as_str().unwrap_or_default(),r["command"].as_str().unwrap_or_default().chars().take(70).collect::<String>())).collect::<Vec<_>>().join("\n");if !confirm(window,"清理本地日志".into(),format!("删除选中的{}份已结束或中断日志？\n{}\n仅处理电脑本地记录，运行中的日志保留。",records.len(),detail)).await?{return Ok(json!({"removed":0,"canceled":true}));}state.logs.remove(ids).await},
            "window:fullscreen"=>{let full=!window.is_fullscreen()?;window.set_fullscreen(full)?;Ok(json!(full))},
            _=>bail!("此操作仍在后续迁移工作包中，正式操作请使用 V0.3.5。")
        }
    }.await;
    result.map_err(|e| e.to_string())
}
pub fn run() {
    let executable = std::env::current_exe().expect("application executable");
    let portable = std::env::args().any(|arg| arg == "--portable")
        || executable.with_file_name("portable.flag").is_file();
    if portable && std::env::var_os("WCS_DATA_DIR").is_none() {
        let directory = executable.with_file_name("data");
        std::env::set_var("WCS_DATA_DIR", &directory);
        if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_none() {
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", directory.join("webview"));
        }
    }
    tauri::Builder::default()
        .setup(move |app| {
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
                if name == "device-state" {
                    if let Some(state) = handle.try_state::<AppState>() {
                        if data["ssh"] != "connected" {
                            state.files.invalidate();
                            state.terminal.stop_all();
                            state
                                .logs
                                .detach("设备连接已关闭，未收到的输出保留为缺失。");
                        } else {
                            state.logs.refresh_status();
                        }
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
            let documents = app.path().document_dir()?;
            let legacy = documents.join("Wheel Combat Studio/Logs");
            let default_log_root = if portable {
                directory.join("logs")
            } else if legacy.is_dir() {
                legacy
            } else {
                documents.join("PortCanvas/Logs")
            };
            let logs = LogService::new(
                devices.clone(),
                directory.join("log-settings.json"),
                default_log_root.clone(),
            )?;
            let terminal = TerminalPool::new(devices.clone(),logs.clone(),namespace,directory.join("terminals.json"));
            app.manage(AppState {
                profiles: Arc::new(Mutex::new(ProfileStore { directory })),
                devices,
                desktop,
                files,
                terminal,
                logs,
                migrating: AtomicBool::new(false),
                default_log_root,
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
                        state.terminal.stop_all();
                        state.files.cancel();
                        state.desktop.stop(None).await;
                        state.devices.disconnect().await;
                        let _ = state.logs.close().await;
                        app.exit(0);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![studio_call, terminal_subscribe])
        .run(tauri::generate_context!())
        .expect("Tauri desktop application");
}

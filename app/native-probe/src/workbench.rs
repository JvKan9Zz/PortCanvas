use crate::workbench_ui::{DeviceRow, Workbench};
use anyhow::{Context, Result};
use portcanvas_core::{
    application::HostConfirmation,
    preferences::{Appearance, EnvironmentSource, Language},
    profiles::{AuthMethod, Profile},
    services::{Paths, Services},
};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::oneshot;
use zeroize::Zeroizing;

type PendingHost = Arc<Mutex<Option<(u64, oneshot::Sender<bool>)>>>;
fn text(source: &str, english: bool) -> String {
    if source.is_empty() {
        return String::new();
    }
    portcanvas_core::i18n::text(
        source,
        if english {
            Language::English
        } else {
            Language::Chinese
        },
    )
}
fn form(ui: &Workbench) -> Result<Profile> {
    Ok(Profile {
        id: ui.get_selected_id().into(),
        name: ui.get_device_name().into(),
        host: ui.get_host().into(),
        port: ui
            .get_port()
            .trim()
            .parse()
            .context("SSH 端口应为 1–65535。")?,
        username: ui.get_username().into(),
        display: ui.get_display().into(),
        xauthority: ui.get_xauthority().into(),
        auth_method: if ui.get_auth() == 1 {
            AuthMethod::Key
        } else {
            AuthMethod::Password
        },
        private_key_path: ui.get_key_path().into(),
        remember_password: ui.get_remember(),
        encrypted_secret: None,
    })
}
fn fill(ui: &Workbench, profile: Option<&Profile>) {
    ui.set_password("".into());
    ui.set_error("".into());
    if let Some(p) = profile {
        ui.set_selected_id(p.id.as_str().into());
        ui.set_device_name(p.name.as_str().into());
        ui.set_host(p.host.as_str().into());
        ui.set_port(p.port.to_string().into());
        ui.set_username(p.username.as_str().into());
        ui.set_auth(i32::from(p.auth_method == AuthMethod::Key));
        ui.set_key_path(p.private_key_path.as_str().into());
        ui.set_remember(p.remember_password);
        ui.set_display(p.display.as_str().into());
        ui.set_xauthority(p.xauthority.as_str().into());
    } else {
        ui.set_selected_id("".into());
        ui.set_device_name("".into());
        ui.set_host("".into());
        ui.set_port("22".into());
        ui.set_username("".into());
        ui.set_auth(0);
        ui.set_key_path("".into());
        ui.set_remember(false);
        ui.set_display(":0".into());
        ui.set_xauthority("".into());
    }
}
fn profiles(ui: &Workbench, services: &Services) -> Result<()> {
    let data = services.profiles.lock().unwrap().read()?;
    ui.set_devices(ModelRc::new(VecModel::from(
        data.devices
            .into_iter()
            .map(|p| DeviceRow {
                id: p.id.as_str().into(),
                name: p.name.as_str().into(),
                address: format!("{} · {}", p.address(), p.username).into(),
            })
            .collect::<Vec<_>>(),
    )));
    Ok(())
}
fn preferences(ui: &Workbench, services: &Services) -> Result<()> {
    let p = services.preferences.lock().unwrap().read()?;
    ui.set_english(p.language == Language::English);
    ui.set_appearance(match p.appearance {
        Appearance::Light => 0,
        Appearance::Dark => 1,
        Appearance::System => 2,
    });
    ui.set_environment_source(i32::from(p.environment_source == EnvironmentSource::Board));
    ui.set_desktop_mode(match p.desktop_mode {
        portcanvas_core::desktop_settings::DesktopMode::Smooth => 0,
        portcanvas_core::desktop_settings::DesktopMode::Clear => 1,
        portcanvas_core::desktop_settings::DesktopMode::Economy => 2,
    });
    ui.set_log_limit(
        services
            .logs
            .settings()
            .max_bytes
            .map(|n| (n / 1024 / 1024).to_string())
            .unwrap_or_default()
            .into(),
    );
    Ok(())
}
fn device_state(ui: &Workbench, value: &serde_json::Value, pending: &PendingHost) {
    let state = value["ssh"].as_str().unwrap_or("disconnected");
    ui.set_busy(state == "connecting");
    ui.set_connected(state == "connected");
    ui.set_connection_status(
        match state {
            "connected" => "已连接",
            "connecting" => "正在连接",
            "error" => "连接失败",
            _ => "尚未连接设备",
        }
        .into(),
    );
    let stage = value["connectionStage"].as_str().unwrap_or("");
    let label = match stage {
        "prepare" => "准备认证资料",
        "resolve" => "解析设备地址",
        "tcp" => "建立 TCP 连接",
        "handshake" => "SSH 密钥交换",
        "identity" => "验证主机身份",
        "secure" => "完成 SSH 握手",
        "authenticate" => "登录认证",
        "ready" => "连接完成",
        _ => "",
    };
    ui.set_connection_detail(
        format!(
            "{}\n{}",
            text(label, ui.get_english()),
            text(value["message"].as_str().unwrap_or(""), ui.get_english())
        )
        .trim()
        .into(),
    );
    if state != "connecting" && ui.get_dialog_kind() == 2 {
        pending.lock().unwrap().take();
        ui.set_dialog_kind(0);
    }
}
fn data_paths() -> Result<Paths> {
    #[cfg(windows)]
    fn known_folder(id: &windows::core::GUID) -> Result<PathBuf> {
        use windows::Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT},
        };
        unsafe {
            let value = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
            let text = value.to_string();
            CoTaskMemFree(Some(value.0 as *const _));
            Ok(PathBuf::from(text?))
        }
    }
    use windows::Win32::UI::Shell::{FOLDERID_Documents, FOLDERID_LocalAppData};
    let data = if let Some(data) = std::env::var_os("WCS_DATA_DIR") {
        PathBuf::from(data)
    } else {
        known_folder(&FOLDERID_LocalAppData)?.join("PortCanvas/NativeDevelopment")
    };
    anyhow::ensure!(data.is_absolute(), "应用数据目录须为绝对路径");
    std::fs::create_dir_all(&data)?;
    let documents = known_folder(&FOLDERID_Documents)?;
    Ok(Paths {
        data,
        documents: documents.clone(),
        local_files: documents,
        portable: true,
    })
}
pub fn run() -> Result<()> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("femtovg".into())
        .select()?;
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?,
    );
    let ui = Workbench::new()?;
    ui.on_tr(|source, english| text(&source, english).into());
    let pending: PendingHost = Arc::new(Mutex::new(None));
    let request_id = Arc::new(AtomicU64::new(1));
    let generation = Arc::new(AtomicU64::new(0));
    let weak = ui.as_weak();
    let host_pending = pending.clone();
    let latest = generation.clone();
    let events: portcanvas_core::session::EventSink = Arc::new(move |topic, value| {
        if topic == "device-state" {
            let number = value["generation"].as_u64().unwrap_or(0);
            latest.fetch_max(number, Ordering::SeqCst);
            let latest = latest.clone();
            let pending = host_pending.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                if number >= latest.load(Ordering::SeqCst) {
                    device_state(&ui, &value, &pending);
                }
            });
        } else if topic == "discovery-state" {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_discovering(value["status"] == "searching");
                ui.set_discovery_progress(if value["status"] == "searching" {
                    format!("{} / {}", value["checked"], value["total"]).into()
                } else {
                    text(value["message"].as_str().unwrap_or(""), ui.get_english()).into()
                });
                let rows = value["devices"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|d| {
                        let host = d["host"].as_str()?;
                        Some(DeviceRow {
                            id: host.into(),
                            name: host.into(),
                            address: format!("{host}:22").into(),
                        })
                    })
                    .collect::<Vec<_>>();
                ui.set_found(ModelRc::new(VecModel::from(rows)));
            });
        }
    });
    let paths = data_paths()?;
    ui.set_data_directory(paths.data.to_string_lossy().as_ref().into());
    let services = Services::new(paths, events, None)?;
    if let Err(e) = profiles(&ui, &services).and_then(|_| preferences(&ui, &services)) {
        ui.set_error(e.to_string().into());
    }
    device_state(&ui, &services.devices.snapshot(), &pending);
    ui.on_new_device({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                fill(&ui, None);
            }
        }
    });
    ui.on_select_device({
        let weak = ui.as_weak();
        let services = services.clone();
        move |id| {
            if let Some(ui) = weak.upgrade() {
                match services.profiles.lock().unwrap().read() {
                    Ok(data) => fill(&ui, data.devices.iter().find(|p| p.id == id.as_str())),
                    Err(e) => ui.set_error(e.to_string().into()),
                }
            }
        }
    });
    ui.on_select_found({
        let weak = ui.as_weak();
        let services = services.clone();
        move |host| {
            if let Some(ui) = weak.upgrade() {
                match services.profiles.lock().unwrap().read() {
                    Ok(data) => {
                        if let Some(p) = data
                            .devices
                            .iter()
                            .find(|p| p.host == host.as_str() && p.port == 22)
                        {
                            fill(&ui, Some(p));
                        } else {
                            fill(&ui, None);
                            ui.set_host(host);
                        }
                    }
                    Err(e) => ui.set_error(e.to_string().into()),
                }
            }
        }
    });
    ui.on_choose_key({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                let result = ui
                    .window()
                    .with_winit_window(|window| {
                        rfd::FileDialog::new().set_parent(window).pick_file()
                    })
                    .flatten();
                if let Some(path) = result {
                    ui.set_key_path(path.to_string_lossy().as_ref().into());
                }
            }
        }
    });
    ui.on_save_device({
        let weak = ui.as_weak();
        let services = services.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                let secret = Zeroizing::new(ui.get_password().to_string());
                match form(&ui).and_then(|p| services.save_profile(p, &secret)) {
                    Ok(saved) => {
                        fill(&ui, Some(&saved));
                        let _ = profiles(&ui, &services);
                    }
                    Err(e) => ui.set_error(e.to_string().into()),
                }
            }
        }
    });
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let operation = Arc::new(AtomicU64::new(0));
    ui.on_connect_device({
        let weak = ui.as_weak();
        let services = services.clone();
        let runtime = runtime.clone();
        let pending = pending.clone();
        let ids = request_id.clone();
        let jobs = jobs.clone();
        let operation = operation.clone();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let secret = Zeroizing::new(ui.get_password().to_string());
            let profile = match form(&ui).and_then(|p| services.save_profile(p, &secret)) {
                Ok(p) => p,
                Err(e) => {
                    ui.set_error(e.to_string().into());
                    return;
                }
            };
            fill(&ui, Some(&profile));
            let _ = profiles(&ui, &services);
            ui.set_busy(true);
            let serial = operation.fetch_add(1, Ordering::SeqCst) + 1;
            let prompt_ui = weak.clone();
            let host_pending = pending.clone();
            let ids = ids.clone();
            let confirm: HostConfirmation = Arc::new(move |identity, cancel| {
                let id = ids.fetch_add(1, Ordering::Relaxed);
                let (send, receive) = oneshot::channel();
                *host_pending.lock().unwrap() = Some((id, send));
                let pending = host_pending.clone();
                let show_pending = pending.clone();
                let show_cancel = cancel.clone();
                let _ = prompt_ui.upgrade_in_event_loop(move |ui| {
                    if !show_cancel.is_cancelled()
                        && show_pending
                            .lock()
                            .unwrap()
                            .as_ref()
                            .is_some_and(|(current, _)| *current == id)
                    {
                        ui.set_dialog_title("首次连接设备".into());
                        ui.set_dialog_message(
                            format!(
                                "{}\n{}\n{}",
                                text("请核对目标设备的SSH身份", ui.get_english()),
                                identity.address,
                                identity.fingerprint
                            )
                            .into(),
                        );
                        ui.set_dialog_kind(2);
                    }
                });
                Box::pin(async move {
                    let accepted = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => false,
                        result = receive => result.unwrap_or(false)
                    };
                    let mut slot = pending.lock().unwrap();
                    if slot.as_ref().is_some_and(|(current, _)| *current == id) {
                        slot.take();
                    }
                    Ok(accepted)
                })
            });
            let service = services.clone();
            let weak = weak.clone();
            let operation = operation.clone();
            let pending = pending.clone();
            jobs.lock().unwrap().push(runtime.spawn(async move {
                let result = service.connect(profile, secret, confirm).await;
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    if operation.load(Ordering::SeqCst) == serial {
                        if let Err(e) = result {
                            ui.set_error(e.to_string().into());
                        }
                        device_state(&ui, &service.devices.snapshot(), &pending);
                    }
                });
            }));
        }
    });
    ui.on_disconnect_device({
        let services = services.clone();
        let runtime = runtime.clone();
        let jobs = jobs.clone();
        let pending = pending.clone();
        let operation = operation.clone();
        move || {
            operation.fetch_add(1, Ordering::SeqCst);
            pending.lock().unwrap().take();
            let service = services.clone();
            jobs.lock().unwrap().push(runtime.spawn(async move {
                service.disconnect().await;
            }));
        }
    });
    let remove = Arc::new(Mutex::new(None::<String>));
    ui.on_remove_device({
        let weak = ui.as_weak();
        let remove = remove.clone();
        let services = services.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                let profile = match services.profiles.lock().unwrap().read() {
                    Ok(data) => data
                        .devices
                        .into_iter()
                        .find(|p| p.id == ui.get_selected_id().as_str()),
                    Err(e) => {
                        ui.set_error(e.to_string().into());
                        return;
                    }
                };
                let Some(profile) = profile else { return };
                *remove.lock().unwrap() = Some(profile.id.clone());
                ui.set_dialog_title("移除设备配置".into());
                ui.set_dialog_message(
                    format!(
                        "{}\n{}\n{}",
                        text("仅移除本机连接配置，是否继续？", ui.get_english()),
                        profile.name,
                        profile.address()
                    )
                    .into(),
                );
                ui.set_dialog_kind(1);
            }
        }
    });
    ui.on_respond({
        let weak = ui.as_weak();
        let pending = pending.clone();
        let remove = remove.clone();
        let services = services.clone();
        let runtime = runtime.clone();
        let jobs = jobs.clone();
        move |accepted| {
            if let Some(ui) = weak.upgrade() {
                let kind = ui.get_dialog_kind();
                ui.set_dialog_kind(0);
                if kind == 2 {
                    if let Some((_, sender)) = pending.lock().unwrap().take() {
                        let _ = sender.send(accepted);
                    }
                } else if kind == 1 {
                    if let Some(id) = remove.lock().unwrap().take() {
                        if accepted {
                            let service = services.clone();
                            let weak = weak.clone();
                            jobs.lock().unwrap().push(runtime.spawn(async move {
                                let result = service.remove_profile(&id).await;
                                let _ = weak.upgrade_in_event_loop(move |ui| match result {
                                    Ok(()) => {
                                        fill(&ui, None);
                                        let _ = profiles(&ui, &service);
                                    }
                                    Err(e) => ui.set_error(e.to_string().into()),
                                });
                            }));
                        }
                    }
                }
            }
        }
    });
    ui.on_discover({
        let services = services.clone();
        let runtime = runtime.clone();
        move || {
            let _entered = runtime.enter();
            services.discovery.start();
        }
    });
    ui.on_cancel_discovery({
        let services = services.clone();
        move || services.discovery.cancel()
    });
    ui.on_set_language({
        let weak = ui.as_weak();
        let services = services.clone();
        let pending = pending.clone();
        move |index| {
            if let Some(ui) = weak.upgrade() {
                match services
                    .preferences
                    .lock()
                    .unwrap()
                    .save_language(if index == 1 {
                        Language::English
                    } else {
                        Language::Chinese
                    }) {
                    Ok(_) => {
                        ui.set_english(index == 1);
                        device_state(&ui, &services.devices.snapshot(), &pending)
                    }
                    Err(e) => ui.set_error(e.to_string().into()),
                }
            }
        }
    });
    ui.on_set_appearance({
        let weak = ui.as_weak();
        let services = services.clone();
        move |index| {
            if let Some(ui) = weak.upgrade() {
                match services
                    .preferences
                    .lock()
                    .unwrap()
                    .save_appearance(match index {
                        1 => Appearance::Dark,
                        2 => Appearance::System,
                        _ => Appearance::Light,
                    }) {
                    Ok(_) => ui.set_appearance(index),
                    Err(e) => ui.set_error(e.to_string().into()),
                }
            }
        }
    });
    ui.on_set_environment({
        let weak = ui.as_weak();
        let services = services.clone();
        move |index| {
            if let Some(ui) = weak.upgrade() {
                let source = if index == 1 {
                    EnvironmentSource::Board
                } else {
                    EnvironmentSource::Computer
                };
                match services
                    .preferences
                    .lock()
                    .unwrap()
                    .save_environment_source(source)
                {
                    Ok(_) => {
                        services.devices.set_environment_source(source);
                        ui.set_environment_source(index)
                    }
                    Err(e) => ui.set_error(e.to_string().into()),
                }
            }
        }
    });
    ui.on_set_desktop_mode({
        let weak = ui.as_weak();
        let services = services.clone();
        move |index| {
            if let Some(ui) = weak.upgrade() {
                let mode = match index {
                    1 => portcanvas_core::desktop_settings::DesktopMode::Clear,
                    2 => portcanvas_core::desktop_settings::DesktopMode::Economy,
                    _ => portcanvas_core::desktop_settings::DesktopMode::Smooth,
                };
                match services.preferences.lock().unwrap().save_desktop_mode(mode) {
                    Ok(_) => ui.set_desktop_mode(index),
                    Err(e) => ui.set_error(e.to_string().into()),
                }
            }
        }
    });
    ui.on_save_log_limit({
        let weak = ui.as_weak();
        let services = services.clone();
        let runtime = runtime.clone();
        let jobs = jobs.clone();
        move |value| {
            let value = if value.trim().is_empty() {
                Ok(None)
            } else {
                value
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| n.checked_mul(1024 * 1024))
                    .map(Some)
                    .ok_or_else(|| {
                        anyhow::anyhow!("日志上限应为 16–1048576 MiB，留空表示不设上限。")
                    })
            };
            match value {
                Err(e) => {
                    let _ =
                        weak.upgrade_in_event_loop(move |ui| ui.set_error(e.to_string().into()));
                }
                Ok(bytes) => {
                    let service = services.clone();
                    let weak = weak.clone();
                    jobs.lock().unwrap().push(runtime.spawn(async move {
                        let result = service.logs.set_max_bytes(bytes).await;
                        let _ = weak.upgrade_in_event_loop(move |ui| match result {
                            Ok(_) => ui.set_error("".into()),
                            Err(e) => ui.set_error(e.to_string().into()),
                        });
                    }));
                }
            }
        }
    });
    let result = ui.run();
    pending.lock().unwrap().take();
    runtime.block_on(services.shutdown());
    for job in jobs.lock().unwrap().drain(..) {
        runtime.block_on(job)?;
    }
    if let Some(path) = std::env::var_os("PORTCANVAS_WORKBENCH_REPORT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &serde_json::json!({"profiles":services.profiles.lock().unwrap().read()?.devices.len(),"language":ui.get_english(),"appearance":ui.get_appearance(),"ssh":services.devices.snapshot()["ssh"]}),
            )?,
        )?;
    }
    result?;
    Ok(())
}

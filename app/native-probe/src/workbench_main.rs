#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod workbench;
mod workbench_material;
mod material_gpu;
mod workbench_background;
mod workbench_system;
mod workbench_update;
mod workbench_glass;
mod workbench_declarative;
mod workbench_monitor;
mod workbench_plugins;
mod workbench_desktop;
mod workbench_logs;
mod workbench_migration;
mod workbench_files;
mod workbench_terminal;
mod workbench_ui;
#[cfg(windows)]
mod windows_chrome;
fn main() {
    #[cfg(target_os="macos")]
    match portcanvas_native_probe::macos_update::dispatch_helper() {
        Ok(true)=>return,Ok(false)=>{},Err(error)=>{eprintln!("{error:#}");std::process::exit(1);}
    }
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--cleanup-legacy-install")) {
        let result = (|| -> anyhow::Result<()> {
            let exe=std::env::current_exe()?;
            let local=std::env::args_os().nth(2).ok_or_else(||anyhow::anyhow!("缺少应用缓存根目录。"))?;
            portcanvas_native_probe::legacy_cleanup::clean(exe.parent().unwrap(),std::path::Path::new(&local))
        })();
        if let Err(error)=result {eprintln!("{error:#}");std::process::exit(1);}
        return;
    }
    portcanvas_native_probe::debug_log::init(None);
    if let Err(error) = workbench::run() {
        portcanvas_native_probe::debug_log::error("startup", &format!("fatal error: {error:#}"));
        eprintln!("{error:#}");
        rfd::MessageDialog::new()
            .set_title("PortCanvas")
            .set_description(format!("{error:#}"))
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        std::process::exit(1);
    }
    portcanvas_native_probe::debug_log::info("shutdown", "PortCanvas exited cleanly");
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod workbench;
mod workbench_desktop;
mod workbench_logs;
mod workbench_terminal;
mod workbench_ui;
fn main() {
    if let Err(error) = workbench::run() {
        eprintln!("{error:#}");
        rfd::MessageDialog::new()
            .set_title("PortCanvas")
            .set_description(format!("{error:#}"))
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        std::process::exit(1);
    }
}

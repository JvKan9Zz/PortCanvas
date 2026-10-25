#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    if studio_core::portable_update::run_helper() { return; }
    studio_core::app::run();
}

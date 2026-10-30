fn main() {
    #[cfg(feature = "desktop")]
    {
        tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&["studio_call","terminal_subscribe"]),
        )).expect("Tauri build metadata");
    }
}

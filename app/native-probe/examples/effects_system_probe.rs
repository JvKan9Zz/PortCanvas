#[cfg(any(windows, target_os = "macos"))]
fn main() {
    #[cfg(windows)]
    use portcanvas_native_probe::windows_effects::WindowsEffects as Effects;
    #[cfg(target_os = "macos")]
    use portcanvas_native_probe::macos_effects::MacEffects as Effects;

    let mut settings = Effects::new(|| {});
    let report = settings.report();
    let errors = settings.close();
    println!(
        "{}",
        serde_json::json!({"system":report,"unsubscribeErrors":errors})
    );
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("System effects probing is supported on Windows and macOS.");
}

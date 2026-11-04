fn main() {
    let mut settings = portcanvas_native_probe::windows_effects::WindowsEffects::new(|| {});
    let report = settings.report();
    let errors = settings.close();
    println!(
        "{}",
        serde_json::json!({"system":report,"unsubscribeErrors":errors})
    );
}

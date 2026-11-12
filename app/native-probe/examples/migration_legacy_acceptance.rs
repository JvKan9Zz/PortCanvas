//! Accept only the owned legacy GUI fixture; report no credential material.
use anyhow::{ensure, Result};
use portcanvas_core::profiles::ProfileStore;
use std::{fs, path::PathBuf};
fn main() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("owned test root"));
    let source = root.join("old-client/data");
    let target = root.join("update-lab/data");
    let before = ProfileStore { directory: source.clone() }.read()?;
    let store = ProfileStore { directory: target.clone() };
    let after = store.read()?;
    ensure!(before.devices.len() == 1 && before.devices[0].name == "v0595 原生迁移验证" && before.devices[0].host == "127.0.0.1", "This acceptance tool only handles the owned loopback fixture");
    let profile = &before.devices[0];
    ensure!(after.devices == before.devices && after.host_keys == before.host_keys, "Legacy profile/trust identity differs");
    let secret = store.connection_secret(profile, zeroize::Zeroizing::new(String::new()))?;
    ensure!(secret.as_str() == "synthetic-only", "Legacy credential could not be reused");
    ensure!(!fs::read_to_string(target.join("devices.json"))?.contains("synthetic-only"), "Plaintext credential found");
    let preferences = portcanvas_core::preferences::PreferenceStore { file: target.join("settings.json") }.read()?;
    ensure!(preferences.language == portcanvas_core::preferences::Language::English && preferences.appearance == portcanvas_core::preferences::Appearance::Dark, "Legacy preferences differ");
    ensure!(preferences.glass_quality == portcanvas_core::preferences::GlassQuality::Full, "New-only default was lost");
    let media = preferences.background_image.as_deref().expect("owned image");
    ensure!(fs::read(source.join("backgrounds").join(media))? == fs::read(target.join("backgrounds").join(media))?, "Background differs");
    ensure!(fs::read(source.join("terminals.json"))? == fs::read(target.join("terminals.json"))?, "Terminal catalog differs");
    let receipt = serde_json::json!({"legacyVersion":"0.5.95","nativeVersion":env!("CARGO_PKG_VERSION"),"profileAndTrustPreserved":true,"protectedCredentialReused":true,"plaintextCredentialAbsent":true,"languageAndAppearanceImported":true,"newQualityDefaultPreserved":true,"backgroundBytesEqual":true,"terminalCatalogEqual":true,"networkAuthentication":false});
    fs::write(root.join("legacy-compatibility.json"), serde_json::to_vec_pretty(&receipt)?)?;
    println!("Owned legacy fixture: credentials, trust, preferences, media and terminal catalog accepted.");
    Ok(())
}

use anyhow::Result;
use portcanvas_core::preferences::{GlassQuality as Quality, PreferenceStore};
use serde_json::{json, Value};
use std::path::PathBuf;
fn store() -> Result<PreferenceStore> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated WCS_TEST_ROOT"))
        .join(format!("glass-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root)?;
    Ok(PreferenceStore {
        file: root.join("settings.json"),
    })
}
#[test]
fn legacy_settings_and_quality_opacity_round_trip_preserve_other_fields() -> Result<()> {
    let store = store()?;
    assert_eq!(store.read()?.glass_quality, Quality::Full);
    assert_eq!(store.read()?.glass_opacity, 100);
    for opacity in [None, Some(0), Some(1), Some(50), Some(99), Some(100)] {
        let mut legacy = json!({"language":"en","theme":"graphite","appearance":"dark","desktopMode":"economy","environmentSource":"board","backgroundColor":"#123456","backgroundImage":"0123456789abcdef0123456789abcdef.webp","backgroundCrop":{"x":0.1,"y":0.2,"width":0.5,"height":0.5},"backgroundVolume":27,"futureSettings":{"unknown":[null,"原文",true]}});
        if let Some(value) = opacity {
            legacy["glassOpacity"] = json!(value);
        }
        std::fs::write(&store.file, serde_json::to_vec(&legacy)?)?;
        assert_eq!(store.read()?.glass_quality, Quality::Full);
        let expected = opacity.unwrap_or(100);
        assert_eq!(store.read()?.glass_opacity, expected);
        for quality in [Quality::Performance, Quality::Full, Quality::Balanced] {
            store.save_glass(quality, expected)?;
            let reopened = PreferenceStore {
                file: store.file.clone(),
            };
            assert_eq!(reopened.read()?.glass_quality, quality);
            assert_eq!(reopened.read()?.glass_opacity, expected);
            let saved: Value = serde_json::from_slice(&std::fs::read(&store.file)?)?;
            for (key, value) in legacy.as_object().unwrap() {
                assert_eq!(saved[key], *value, "{key}");
            }
            for changed in [0, 50, 100] {
                store.save_glass_opacity(changed)?;
                assert_eq!(store.read()?.glass_quality, quality);
            }
        }
    }
    std::fs::remove_dir_all(store.file.parent().unwrap())?;
    Ok(())
}
#[test]
fn malformed_or_invalid_quality_cannot_be_overwritten() -> Result<()> {
    let store = store()?;
    for bytes in [
        br#"{"glassQuality":"ultra","glassOpacity":50}"#.as_slice(),
        br#"{"glassQuality":null}"#,
        br#"{"glassQuality":1}"#,
        br#"{"glassQuality":"FULL"}"#,
        br#"{"glassQuality":"full","glassOpacity":101}"#,
        b"{broken",
    ] {
        std::fs::write(&store.file, bytes)?;
        assert!(store.read().is_err());
        assert!(store.save_glass(Quality::Balanced, 50).is_err());
        assert_eq!(std::fs::read(&store.file)?, bytes);
    }
    std::fs::write(&store.file, b"{}")?;
    let before = std::fs::read(&store.file)?;
    assert!(store.save_glass(Quality::Full, 101).is_err());
    assert_eq!(std::fs::read(&store.file)?, before);
    std::fs::remove_dir_all(store.file.parent().unwrap())?;
    Ok(())
}
#[cfg(windows)]
#[test]
fn failed_atomic_save_keeps_the_previous_glass_settings() -> Result<()> {
    let store = store()?;
    store.save_glass(Quality::Full, 50)?;
    let before = std::fs::read(&store.file)?;
    let mut permissions = std::fs::metadata(&store.file)?.permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&store.file, permissions.clone())?;
    let result = store.save_glass(Quality::Performance, 0);
    permissions.set_readonly(false);
    std::fs::set_permissions(&store.file, permissions)?;
    assert!(result.is_err());
    assert_eq!(std::fs::read(&store.file)?, before);
    assert_eq!(std::fs::read_dir(store.file.parent().unwrap())?.count(), 1);
    std::fs::remove_dir_all(store.file.parent().unwrap())?;
    Ok(())
}

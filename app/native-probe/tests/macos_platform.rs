#![cfg(target_os = "macos")]
use portcanvas_native_probe::{instance, platform_paths::{macos_paths, Channel}};
use std::path::PathBuf;

#[test]
fn macos_channels_and_overrides_do_not_share_writers() {
    let home = PathBuf::from("/Users/synthetic");
    let stable = macos_paths(&home, None, Channel::Stable).unwrap();
    let beta = macos_paths(&home, None, Channel::Beta).unwrap();
    assert_eq!(stable.data, home.join("Library/Application Support/PortCanvas/Stable"));
    assert_eq!(beta.data, home.join("Library/Application Support/PortCanvas/Beta"));
    assert_eq!(stable.documents, home.join("Documents"));
    assert!(macos_paths(&home, Some(PathBuf::from("relative")), Channel::Stable).is_err());
    assert!(macos_paths(&PathBuf::from("relative"), None, Channel::Stable).is_err());
}

#[test]
fn lease_is_per_directory_and_released_without_removing_its_inode() {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());
    let stable = root.join("Stable"); let beta = root.join("Beta");
    std::fs::create_dir_all(&stable).unwrap(); std::fs::create_dir_all(&beta).unwrap();
    let first = instance::acquire(&stable).unwrap();
    assert!(instance::acquire(&stable).is_err());
    let second = instance::acquire(&beta).unwrap();
    drop(first); drop(instance::acquire(&stable).unwrap()); drop(second);
    std::fs::remove_dir_all(root).unwrap();
}

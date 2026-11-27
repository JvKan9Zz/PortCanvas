pub mod fixture;
pub mod glass_model;
pub mod effects_policy;
#[cfg(windows)]
pub mod windows_effects;
pub mod arena_model;
pub mod monitor_model;
pub mod background_model;
pub mod background_image;
#[cfg(any(windows,target_os="macos"))]
pub mod background_playback;
#[cfg(windows)]
pub mod media_audio;
pub mod camera_client;
pub mod file_browser;
pub mod desktop_client;
pub mod log_browser;
#[cfg(windows)]
pub mod media;
pub mod rfb;
pub mod terminal;
pub mod terminal_editor;
pub mod terminal_sessions;
pub mod update;
pub mod update_client;
pub mod legacy_cleanup;
pub mod color_model;
pub mod instance;
pub mod platform_paths;

#[cfg(target_os="macos")] #[path="media_macos.rs"] pub mod media;
#[cfg(target_os="macos")] #[path="media_audio_macos.rs"] pub mod media_audio;

#[cfg(target_os="macos")] pub mod macos_effects;
#[cfg(all(target_os="macos",feature="gui"))] pub mod macos_window;

#[cfg(target_os="macos")] pub mod macos_update;

#[cfg(target_os="macos")] pub mod macos_legacy_cleanup;

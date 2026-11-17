//! Cleanup owns only a validated, executable-specific synthetic Keychain item.
//! Mac credential integration runs only with debug assertions; release builds have no test hooks.
#[cfg(all(target_os = "macos", debug_assertions))]
use std::sync::{Arc, Mutex, OnceLock, Weak};
#[cfg(all(target_os = "macos", debug_assertions))]
pub struct TestKeychain(String);
#[cfg(all(target_os = "macos", debug_assertions))]
static SHARED: OnceLock<Mutex<Weak<TestKeychain>>> = OnceLock::new();
#[cfg(all(target_os = "macos", debug_assertions))]
impl Drop for TestKeychain {
    fn drop(&mut self) {
        let shared = SHARED.get_or_init(|| Mutex::new(Weak::new())).lock().unwrap();
        // A new fixture may acquire ownership while the last old Arc is being dropped.
        // Only the current owner can delete; serialize that check with acquisitions.
        if std::ptr::eq(shared.as_ptr(), self) && portcanvas_core::credentials::debug_test_keychain_service().is_ok_and(|service| service == self.0) {
            let _ = portcanvas_core::credentials::delete_debug_test_keychain();
        }
    }
}
#[cfg(all(target_os = "macos", debug_assertions))]
pub fn keychain() -> anyhow::Result<Arc<TestKeychain>> {
    let mut shared = SHARED.get_or_init(|| Mutex::new(Weak::new())).lock().unwrap();
    if let Some(keychain) = shared.upgrade() { return Ok(keychain); }
    let service = portcanvas_core::credentials::debug_test_keychain_service()?;
    let keychain = Arc::new(TestKeychain(service));
    *shared = Arc::downgrade(&keychain);
    Ok(keychain)
}

use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use zeroize::Zeroizing;

const WINDOWS_PREFIX: &str = "dpapi-v1:";
const MACOS_PREFIX: &str = "keychain-v1:";
#[cfg(target_os="macos")]
static KEYCHAIN_OPERATIONS: std::sync::Mutex<()> = std::sync::Mutex::new(());
const PREFIX: &str = if cfg!(target_os="macos"){MACOS_PREFIX}else{WINDOWS_PREFIX};
pub fn validate(value: &str) -> Result<()> {
    let encoded=value.strip_prefix(WINDOWS_PREFIX).or_else(||value.strip_prefix(MACOS_PREFIX));
    if value.len() > 16 * 1024 || encoded.is_none() {
        bail!("保存的加密凭据格式无效，原文件保留。");
    }
    STANDARD.decode(encoded.unwrap()).map_err(|_| anyhow::anyhow!("保存的加密凭据格式无效，原文件保留。"))?;
    Ok(())
}
pub fn seal(secret: &str, context: &[u8]) -> Result<String> {
    if secret.len() > 8192 { bail!("密码或密钥口令过长（最多8192字节）。"); }
    Ok(format!("{PREFIX}{}", STANDARD.encode(crypt(secret.as_bytes(), context, true)?)))
}
pub fn open(value: &str, context: &[u8]) -> Result<Zeroizing<String>> {
    validate(value)?;
    anyhow::ensure!(value.starts_with(PREFIX),"保存的凭据属于其他系统，请重新输入并保存。");
    let cipher = STANDARD.decode(&value[PREFIX.len()..])?;
    let plain = Zeroizing::new(crypt(&cipher, context, false)?);
    Ok(Zeroizing::new(String::from_utf8(plain.to_vec()).map_err(|_| anyhow::anyhow!("保存的凭据无效，请重新输入。"))?))
}
#[cfg(windows)]
fn crypt(input: &[u8], context: &[u8], protect: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    use zeroize::Zeroize;
    let input = CRYPT_INTEGER_BLOB { cbData: input.len() as u32, pbData: input.as_ptr() as *mut u8 };
    let entropy = CRYPT_INTEGER_BLOB { cbData: context.len() as u32, pbData: context.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        if protect { CryptProtectData(&input, std::ptr::null(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
        else { CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
    };
    if ok == 0 { bail!("{}", if protect { "Windows加密保存失败，配置未更改。" } else { "当前Windows账户无法解密保存的凭据，请重新输入并保存。" }); }
    let result = if output.cbData == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() } };
    unsafe {
        if !protect && output.cbData > 0 { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize).zeroize(); }
        LocalFree(output.pbData as *mut std::ffi::c_void);
    }
    Ok(result)
}
#[cfg(all(target_os="macos", not(test)))]
fn keychain_service() -> Result<String> {
    #[cfg(debug_assertions)]
    {
        if let Some(service) = std::env::var_os("PORTCANVAS_TEST_KEYCHAIN_SERVICE") {
            let root = std::env::var_os("WCS_TEST_ROOT").map(std::path::PathBuf::from);
            let service = service.to_str().ok_or_else(|| anyhow::anyhow!("测试钥匙串命名空间无效。"))?;
            validate_test_keychain_service(service, root.as_deref())?;
            // Unsigned test/debug executables must not share an ACL-bound Keychain item.
            use sha2::{Digest, Sha256};
            let executable = std::fs::canonicalize(std::env::current_exe()?)?;
            let digest = Sha256::digest(executable.as_os_str().as_encoded_bytes());
            let suffix: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
            return Ok(format!("{service}.{suffix}"));
        }
        anyhow::ensure!(std::env::var_os("WCS_TEST_ROOT").is_none(), "隔离测试必须指定测试钥匙串命名空间，未访问正式凭据。");
    }
    Ok("io.portcanvas.app.credentials".into())
}
#[cfg(all(target_os="macos", any(test, debug_assertions)))]
fn validate_test_keychain_service(service: &str, root: Option<&std::path::Path>) -> Result<()> {
    let suffix = service.strip_prefix("io.portcanvas.test.credentials.").unwrap_or("");
    anyhow::ensure!(!suffix.is_empty() && suffix.len() <= 120 && suffix.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)), "测试钥匙串命名空间无效，未访问正式凭据。");
    anyhow::ensure!(root.is_some_and(|p| p.is_absolute() && p.is_dir()), "测试钥匙串必须配合独立的 WCS_TEST_ROOT 目录。");
    Ok(())
}
/// The validated, executable-specific disposable namespace used by debug test harnesses.
#[cfg(all(target_os="macos", debug_assertions, not(test)))]
pub fn debug_test_keychain_service() -> Result<String> {
    anyhow::ensure!(std::env::var_os("PORTCANVAS_TEST_KEYCHAIN_SERVICE").is_some(), "测试钥匙串命名空间缺失。");
    keychain_service()
}
/// Remove only the currently configured disposable debug namespace.
#[cfg(all(target_os="macos", debug_assertions, not(test)))]
pub fn delete_debug_test_keychain() -> Result<()> {
    let service = debug_test_keychain_service()?;
    let _lock = KEYCHAIN_OPERATIONS.lock().unwrap_or_else(|e| e.into_inner());
    match security_framework::passwords::delete_generic_password(&service, "encryption-key-v1") {
        Ok(()) => Ok(()),
        Err(error) if error.code() == -25300 => Ok(()),
        Err(error) => Err(error.into()),
    }
}
#[cfg(all(target_os="macos", test))]
thread_local! {
    static TEST_KEYCHAIN: TestKeychain = TestKeychain::new();
}
#[cfg(all(target_os="macos", test))]
struct TestKeychain(String);
#[cfg(all(target_os="macos", test))]
impl TestKeychain {
    fn new() -> Self {
        use std::io::Write;
        let service = format!("io.portcanvas.test.credentials.{}", uuid::Uuid::new_v4());
        let root = std::path::PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated WCS_TEST_ROOT"));
        assert!(root.is_absolute() && root.is_dir(), "isolated test root must already exist");
        // Keep exact ownership evidence even if a test crashes before its Drop guard runs.
        let _manifest_lock = KEYCHAIN_OPERATIONS.lock().unwrap_or_else(|e| e.into_inner());
        let mut manifest = std::fs::OpenOptions::new().create(true).append(true).open(root.join("test-keychain-items.tsv")).unwrap();
        writeln!(manifest, "{}\t{}", std::process::id(), service).unwrap();
        Self(service)
    }
}
#[cfg(all(target_os="macos", test))]
impl Drop for TestKeychain {
    fn drop(&mut self) {
        let _lock = KEYCHAIN_OPERATIONS.lock().unwrap_or_else(|e| e.into_inner());
        let _=security_framework::passwords::delete_generic_password(&self.0,"encryption-key-v1");
    }
}
#[cfg(all(target_os="macos", test))]
fn keychain_service() -> Result<String> { Ok(TEST_KEYCHAIN.with(|key|key.0.clone())) }
#[cfg(target_os="macos")]
fn crypt(input:&[u8],context:&[u8],protect:bool)->Result<Vec<u8>>{
    crypt_for_service(input,context,protect,&keychain_service()?)
}
#[cfg(target_os="macos")]
fn crypt_for_service(input:&[u8],context:&[u8],protect:bool,service:&str)->Result<Vec<u8>>{
    use anyhow::Context;
    use ring::{aead,rand::{SecureRandom,SystemRandom}};
    use security_framework::passwords::get_generic_password;
    let account="encryption-key-v1";
    let random=SystemRandom::new();
    let key = {
    // The file-backed macOS Keychain can deadlock inside Security when a duplicate
    // add races an in-process read. Serialize this short key-loading transaction;
    // add-only still protects the winner against other processes.
    let _lock = KEYCHAIN_OPERATIONS.lock().unwrap_or_else(|e| e.into_inner());
    Zeroizing::new(match get_generic_password(&service,account){
        Ok(key)=>key,
        Err(error) if error.code()==-25300&&protect=>{
            let mut key=Zeroizing::new(vec![0;32]);random.fill(&mut key).map_err(|_|anyhow::anyhow!("系统随机数生成失败。"))?;
            // Add only: another process/thread may have created the account since our read.
            // Updating here would invalidate every ciphertext sealed with its winning key.
            persist_created_key(service, account, &key)?
        },
        Err(error)=>return Err(error).context("无法读取钥匙串凭据，请重新输入或检查钥匙串访问权限。"),
    })
    };
    let key=aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM,&key).map_err(|_|anyhow::anyhow!("钥匙串加密密钥无效。"))?);
    if protect{
        let mut nonce=[0;12];random.fill(&mut nonce).map_err(|_|anyhow::anyhow!("系统随机数生成失败。"))?;
        let mut output=input.to_vec();key.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce),aead::Aad::from(context),&mut output).map_err(|_|anyhow::anyhow!("凭据加密失败。"))?;
        Ok([nonce.as_slice(),output.as_slice()].concat())
    }else{
        anyhow::ensure!(input.len()>=28,"保存的凭据无效，请重新输入。");
        let nonce=input[..12].try_into().unwrap();let mut output=input[12..].to_vec();
        let plain=key.open_in_place(aead::Nonce::assume_unique_for_key(nonce),aead::Aad::from(context),&mut output).map_err(|_|anyhow::anyhow!("保存的凭据无法解密，请重新输入并保存。"))?;
        Ok(plain.to_vec())
    }
}
/// Caller serializes local Keychain operations. A duplicate must never update the winner.
#[cfg(target_os="macos")]
fn persist_created_key(service: &str, account: &str, candidate: &[u8]) -> Result<Vec<u8>> {
    use anyhow::Context;
    use core_foundation::data::CFData;
    use security_framework::{passwords::get_generic_password, item::{ItemAddOptions, ItemAddValue, ItemClass}};
    let mut add = ItemAddOptions::new(ItemAddValue::Data { class: ItemClass::generic_password(), data: CFData::from_buffer(candidate) });
    add.set_service(service).set_account_name(account);
    match add.add() {
        Ok(()) => Ok(candidate.to_vec()),
        Err(error) if error.code() == -25299 => get_generic_password(service,account).context("无法读取已创建的钥匙串密钥，配置未更改。"),
        Err(error) => Err(error).context("钥匙串保存失败，配置未更改。"),
    }
}
#[cfg(not(any(windows,target_os="macos")))]
fn crypt(_: &[u8], _: &[u8], _: bool) -> Result<Vec<u8>> { bail!("当前平台暂不支持记住凭据；仍可手动输入连接。"); }

#[cfg(all(test, any(windows,target_os="macos")))]
mod tests {
    use super::*;
    #[cfg(target_os="macos")]
    #[test]
    fn keychain_tests_use_a_disposable_namespace() {
        assert!(keychain_service().unwrap().starts_with("io.portcanvas.test.credentials."));
        assert_ne!(keychain_service().unwrap(), "io.portcanvas.app.credentials");
    }
    #[cfg(target_os="macos")]
    #[test]
    fn test_keychain_override_requires_namespace_and_an_isolated_root() {
        let root = std::env::temp_dir();
        assert!(validate_test_keychain_service("io.portcanvas.test.credentials.synthetic", Some(&root)).is_ok());
        for name in ["io.portcanvas.app.credentials", "io.portcanvas.test.credentials.", "io.portcanvas.test.credentials.invalid/name"] {
            assert!(validate_test_keychain_service(name, Some(&root)).is_err());
        }
        assert!(validate_test_keychain_service("io.portcanvas.test.credentials.synthetic", None).is_err());
        assert!(validate_test_keychain_service("io.portcanvas.test.credentials.synthetic", Some(std::path::Path::new("relative"))).is_err());
    }
    #[cfg(target_os="macos")]
    #[test]
    fn simultaneous_first_saves_share_one_persistent_key() {
        let service = TestKeychain::new();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let values: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..16).map(|_| {
                let barrier = barrier.clone();
                let service = &service.0;
                scope.spawn(move || {
                    barrier.wait();
                    crypt_for_service(b"synthetic-first-save", b"context", true, service).unwrap()
                })
            }).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for value in values {
            assert_eq!(crypt_for_service(&value, b"context", false, &service.0).unwrap(), b"synthetic-first-save");
        }
    }
    #[cfg(target_os="macos")]
    #[test]
    fn losing_creator_retains_the_existing_key_instead_of_overwriting_it() {
        let service = TestKeychain::new();
        let _lock = KEYCHAIN_OPERATIONS.lock().unwrap_or_else(|e| e.into_inner());
        let winner = [1; 32];
        assert_eq!(persist_created_key(&service.0, "encryption-key-v1", &winner).unwrap(), winner);
        assert_eq!(persist_created_key(&service.0, "encryption-key-v1", &[2; 32]).unwrap(), winner);
        assert_eq!(security_framework::passwords::get_generic_password(&service.0, "encryption-key-v1").unwrap(), winner);
    }
    #[test]
    fn user_bound_credentials_are_encrypted_and_context_bound() {
        let secret = "synthetic-中文-passphrase";
        let value = seal(secret, b"profile-one").unwrap();
        assert!(!value.contains(secret));
        assert_eq!(open(&value, b"profile-one").unwrap().as_str(), secret);
        assert!(open(&value, b"profile-two").is_err());
        assert_eq!(open(&seal("", b"empty").unwrap(), b"empty").unwrap().as_str(), "");
        assert!(open("plaintext-is-not-accepted", b"profile-one").is_err());
    }
}

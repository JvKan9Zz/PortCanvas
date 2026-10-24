use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use zeroize::Zeroizing;

const PREFIX: &str = "dpapi-v1:";
pub fn validate(value: &str) -> Result<()> {
    if value.len() > 16 * 1024 || !value.starts_with(PREFIX) {
        bail!("保存的加密凭据格式无效，原文件保留。");
    }
    STANDARD.decode(&value[PREFIX.len()..]).map_err(|_| anyhow::anyhow!("保存的加密凭据格式无效，原文件保留。"))?;
    Ok(())
}
pub fn seal(secret: &str, context: &[u8]) -> Result<String> {
    if secret.len() > 8192 { bail!("密码或密钥口令过长（最多8192字节）。"); }
    Ok(format!("{PREFIX}{}", STANDARD.encode(crypt(secret.as_bytes(), context, true)?)))
}
pub fn open(value: &str, context: &[u8]) -> Result<Zeroizing<String>> {
    validate(value)?;
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
#[cfg(not(windows))]
fn crypt(_: &[u8], _: &[u8], _: bool) -> Result<Vec<u8>> { bail!("当前平台暂不支持记住凭据；仍可手动输入连接。"); }

#[cfg(all(test, windows))]
mod tests {
    use super::*;
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

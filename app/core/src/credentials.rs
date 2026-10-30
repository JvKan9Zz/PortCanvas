use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use zeroize::Zeroizing;

const WINDOWS_PREFIX: &str = "dpapi-v1:";
const MACOS_PREFIX: &str = "keychain-v1:";
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
#[cfg(target_os="macos")]
fn crypt(input:&[u8],context:&[u8],protect:bool)->Result<Vec<u8>>{
    use anyhow::Context;
    use ring::{aead,rand::{SecureRandom,SystemRandom}};
    use security_framework::passwords::{get_generic_password,set_generic_password};
    let service="io.portcanvas.app.credentials";
    let account="encryption-key-v1";
    let random=SystemRandom::new();
    let key=Zeroizing::new(match get_generic_password(service,account){
        Ok(key)=>key,
        Err(error) if error.code()==-25300&&protect=>{
            let mut key=vec![0;32];random.fill(&mut key).map_err(|_|anyhow::anyhow!("系统随机数生成失败。"))?;
            set_generic_password(service,account,&key).context("钥匙串保存失败，配置未更改。")?;key
        },
        Err(error)=>return Err(error).context("无法读取钥匙串凭据，请重新输入或检查钥匙串访问权限。"),
    });
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
#[cfg(not(any(windows,target_os="macos")))]
fn crypt(_: &[u8], _: &[u8], _: bool) -> Result<Vec<u8>> { bail!("当前平台暂不支持记住凭据；仍可手动输入连接。"); }

#[cfg(all(test, any(windows,target_os="macos")))]
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

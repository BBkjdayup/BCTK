use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use super::{LicenseError, link_or_reparse};

pub(crate) const DEVICE_KEY_FILENAME: &str = "device-key.dpapi";
const MAX_PROTECTED_KEY_BYTES: u64 = 16 * 1024;

pub struct DeviceIdentity {
    signing_key: SigningKey,
    device_id: String,
}

impl DeviceIdentity {
    pub fn ephemeral() -> Self {
        let mut secret = [0_u8; 32];
        OsRng.fill_bytes(&mut secret);
        let signing_key = SigningKey::from_bytes(&secret);
        let device_id = device_id_for_public_key(&signing_key.verifying_key());
        Self {
            signing_key,
            device_id,
        }
    }

    pub fn load_or_create(root: &Path) -> Result<Self, LicenseError> {
        fs::create_dir_all(root)
            .map_err(|error| LicenseError::storage(format!("无法创建授权数据目录：{error}")))?;
        let path = root.join(DEVICE_KEY_FILENAME);
        let secret = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if link_or_reparse(&metadata)
                    || !metadata.is_file()
                    || metadata.len() == 0
                    || metadata.len() > MAX_PROTECTED_KEY_BYTES
                {
                    return Err(LicenseError::storage("本机授权身份文件无效。"));
                }
                let protected = fs::read(&path).map_err(|error| {
                    LicenseError::storage(format!("无法读取本机授权身份：{error}"))
                })?;
                unprotect_for_current_user(&protected)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut secret = [0_u8; 32];
                OsRng.fill_bytes(&mut secret);
                let protected = protect_for_current_user(&secret)?;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|error| {
                        LicenseError::storage(format!("无法保存本机授权身份：{error}"))
                    })?;
                file.write_all(&protected)
                    .and_then(|_| file.sync_all())
                    .map_err(|error| {
                        LicenseError::storage(format!("无法持久化本机授权身份：{error}"))
                    })?;
                secret.to_vec()
            }
            Err(error) => {
                return Err(LicenseError::storage(format!(
                    "无法检查本机授权身份：{error}"
                )));
            }
        };

        let secret: [u8; 32] = secret.try_into().map_err(|_| {
            LicenseError::storage("本机授权身份长度无效，可能来自另一台电脑或其他 Windows 用户。")
        })?;
        let signing_key = SigningKey::from_bytes(&secret);
        let device_id = device_id_for_public_key(&signing_key.verifying_key());
        Ok(Self {
            signing_key,
            device_id,
        })
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn public_key_base64(&self) -> String {
        STANDARD.encode(self.signing_key.verifying_key().to_bytes())
    }

    pub fn sign(&self, payload: &[u8]) -> String {
        STANDARD.encode(self.signing_key.sign(payload).to_bytes())
    }
}

pub fn device_id_for_public_key(public_key: &VerifyingKey) -> String {
    let digest = Sha256::digest(public_key.to_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(windows)]
pub(crate) fn protect_for_current_user(input: &[u8]) -> Result<Vec<u8>, LicenseError> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{GetLastError, LocalFree},
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData},
    };

    let input_len =
        u32::try_from(input.len()).map_err(|_| LicenseError::storage("本机授权身份内容过大。"))?;
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input_len,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input points to `input` for the duration of the call. Windows
    // allocates output_blob.pbData; it is copied before LocalFree releases it.
    let success = unsafe {
        CryptProtectData(
            &input_blob,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output_blob,
        )
    };
    if success == 0 {
        let code = unsafe { GetLastError() };
        return Err(LicenseError::storage(format!(
            "Windows 无法保护本机授权身份（错误 {code}）。"
        )));
    }
    let protected = unsafe {
        std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec()
    };
    unsafe { LocalFree(output_blob.pbData.cast()) };
    Ok(protected)
}

#[cfg(windows)]
pub(crate) fn unprotect_for_current_user(input: &[u8]) -> Result<Vec<u8>, LicenseError> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{GetLastError, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
        },
    };

    let input_len =
        u32::try_from(input.len()).map_err(|_| LicenseError::storage("本机授权身份内容过大。"))?;
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input_len,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB::default();
    let mut description = ptr::null_mut();
    // SAFETY: all pointers are valid for the duration of the call. Windows
    // allocates output buffers; both are released with LocalFree below.
    let success = unsafe {
        CryptUnprotectData(
            &input_blob,
            &mut description,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output_blob,
        )
    };
    if success == 0 {
        let code = unsafe { GetLastError() };
        return Err(LicenseError::storage(format!(
            "本机授权身份无法解密，可能已更换电脑或 Windows 用户（错误 {code}）。"
        )));
    }
    let secret = unsafe {
        std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec()
    };
    unsafe {
        LocalFree(output_blob.pbData.cast());
        if !description.is_null() {
            LocalFree(description.cast());
        }
    }
    Ok(secret)
}

#[cfg(not(windows))]
pub(crate) fn protect_for_current_user(input: &[u8]) -> Result<Vec<u8>, LicenseError> {
    Ok(input.to_vec())
}

#[cfg(not(windows))]
pub(crate) fn unprotect_for_current_user(input: &[u8]) -> Result<Vec<u8>, LicenseError> {
    Ok(input.to_vec())
}

use super::transport::{NativeError, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Store(pub PathBuf);

impl Store {
    pub fn read<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>> {
        let bytes = match std::fs::read(self.0.join(name)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(NativeError::storage()),
        };
        let mut plaintext = unprotect(&bytes)?;
        let value = serde_json::from_slice(&plaintext).map_err(|_| NativeError::storage());
        plaintext.fill(0);
        value.map(Some)
    }
    pub fn write(&self, name: &str, value: &impl Serialize) -> Result<()> {
        let mut plaintext = serde_json::to_vec(value).map_err(|_| NativeError::storage())?;
        let bytes = protect(&plaintext);
        plaintext.fill(0);
        atomic_write(&self.0.join(name), &bytes?)
    }
    pub fn remove(&self, name: &str) -> Result<()> {
        match std::fs::remove_file(self.0.join(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(NativeError::storage()),
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().ok_or_else(NativeError::storage)?)
        .map_err(|_| NativeError::storage())?;
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|_| NativeError::storage())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| NativeError::storage())?;
        drop(file);
        std::fs::rename(&tmp, path).map_err(|_| NativeError::storage())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

#[cfg(windows)]
fn crypt(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().map_err(|_| NativeError::storage())?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let entropy_bytes = b"MonoCode Antigravity native v1";
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy_bytes.len() as u32,
        pbData: entropy_bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // DPAPI scopes encryption to the current Windows user, never the machine.
    unsafe {
        let result = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if result == 0 {
            return Err(NativeError::storage());
        }
        let value = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        LocalFree(output.pbData.cast());
        Ok(value)
    }
}

#[cfg(windows)]
fn protect(bytes: &[u8]) -> Result<Vec<u8>> {
    crypt(bytes, true)
}
#[cfg(windows)]
fn unprotect(bytes: &[u8]) -> Result<Vec<u8>> {
    crypt(bytes, false)
}
#[cfg(not(windows))]
fn protect(_: &[u8]) -> Result<Vec<u8>> {
    Err(NativeError::new(
        "platform",
        "Native Antigravity is available on Windows.",
    ))
}
#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Vec<u8>> {
    Err(NativeError::new(
        "platform",
        "Native Antigravity is available on Windows.",
    ))
}

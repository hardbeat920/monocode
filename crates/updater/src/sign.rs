//! Release signing, replacing `tauri signer generate` and `tauri signer sign`.
//!
//! Keys and signatures keep the Tauri format, so the existing
//! `TAURI_SIGNING_PRIVATE_KEY` secret signs native packages and installs that
//! trust `TAURI_UPDATER_PUBKEY` accept them:
//!
//! - A private key is base64 of the minisign secret key file, encrypted with
//!   the key password.
//! - A public key is base64 of the minisign public key file.
//! - A `.sig` file is base64 of the minisign signature file, with the trusted
//!   comment `timestamp:<unix>\tfile:<name>`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use minisign::{KeyPair, PublicKey, SecretKey, SecretKeyBox};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// A new key pair in the Tauri format.
#[derive(Debug, Clone)]
pub struct GeneratedKeys {
    /// The value for `TAURI_SIGNING_PRIVATE_KEY`.
    pub private_key: String,
    /// The value for `TAURI_UPDATER_PUBKEY`.
    pub public_key: String,
}

/// Generates a key pair. `Some(password)` encrypts the secret key, as
/// `tauri signer generate` always did (an empty password is allowed). `None`
/// leaves it unencrypted, for throwaway test keys.
pub fn generate_keys(password: Option<&str>) -> Result<GeneratedKeys> {
    let KeyPair { pk, sk } = match password {
        Some(password) => KeyPair::generate_encrypted_keypair(Some(password.to_string()))?,
        None => KeyPair::generate_unencrypted_keypair()?,
    };
    Ok(GeneratedKeys {
        private_key: B64.encode(sk.to_box(None)?.to_string()),
        public_key: B64.encode(pk.to_box()?.to_string()),
    })
}

/// Reads a private key: the base64 text itself, or a path to a file that
/// holds it, as `TAURI_SIGNING_PRIVATE_KEY` allowed. A missing password reads
/// as empty instead of prompting.
pub fn decode_secret_key(private_key: &str, password: Option<&str>) -> Result<SecretKey> {
    let private_key = private_key.trim();
    let text = if !private_key.is_empty() && Path::new(private_key).is_file() {
        std::fs::read_to_string(private_key)
            .with_context(|| format!("reading the private key from {private_key}"))?
    } else {
        private_key.to_string()
    };
    let decoded = B64
        .decode(text.trim())
        .context("the private key is not valid base64")?;
    let decoded = String::from_utf8(decoded).context("the private key is not UTF-8")?;
    let sk_box = SecretKeyBox::from_string(&decoded)?;
    match SecretKey::from_box(sk_box, Some(password.unwrap_or_default().to_string())) {
        Ok(sk) => Ok(sk),
        Err(err) if err.to_string().contains("not encrypted") => {
            Ok(SecretKeyBox::from_string(&decoded)?.into_unencrypted_secret_key()?)
        }
        Err(err) => Err(err).context("decrypting the private key"),
    }
}

/// Signs `data` and returns the `.sig` contents. `file_name` goes into the
/// trusted comment.
pub fn sign_bytes(sk: &SecretKey, data: &[u8], file_name: &str) -> Result<String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let trusted_comment = format!("timestamp:{timestamp}\tfile:{file_name}");
    let pk = PublicKey::from_secret_key(sk)?;
    let signature = minisign::sign(
        Some(&pk),
        sk,
        data,
        Some(trusted_comment.as_str()),
        Some("signature from tauri secret key"),
    )?;
    Ok(B64.encode(signature.to_string()))
}

/// Signs the file at `path` and writes `<path>.sig`, which it returns.
pub fn sign_file(sk: &SecretKey, path: &Path) -> Result<PathBuf> {
    let Some(file_name) = path.file_name() else {
        bail!("{} has no file name", path.display());
    };
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let signature = sign_bytes(sk, &data, &file_name.to_string_lossy())?;
    let mut sig_path = path.as_os_str().to_os_string();
    sig_path.push(".sig");
    let sig_path = PathBuf::from(sig_path);
    std::fs::write(&sig_path, &signature)
        .with_context(|| format!("writing {}", sig_path.display()))?;
    Ok(sig_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::verify_signature;

    #[test]
    fn signatures_verify_with_the_updater_check() {
        let keys = generate_keys(None).unwrap();
        let sk = decode_secret_key(&keys.private_key, None).unwrap();
        let signature = sign_bytes(&sk, b"package bytes", "MonoCode.app.tar.gz").unwrap();

        verify_signature(b"package bytes", &signature, &keys.public_key).unwrap();
        assert!(verify_signature(b"other bytes", &signature, &keys.public_key).is_err());
    }

    #[test]
    fn encrypted_keys_need_their_password() {
        let keys = generate_keys(Some("hunter2")).unwrap();
        assert!(decode_secret_key(&keys.private_key, Some("wrong")).is_err());
        let sk = decode_secret_key(&keys.private_key, Some("hunter2")).unwrap();
        let signature = sign_bytes(&sk, b"data", "file").unwrap();
        verify_signature(b"data", &signature, &keys.public_key).unwrap();
    }

    #[test]
    fn the_signature_file_matches_the_tauri_layout() {
        let keys = generate_keys(None).unwrap();
        let sk = decode_secret_key(&keys.private_key, None).unwrap();
        let signature = sign_bytes(&sk, b"data", "MonoCode.app.tar.gz").unwrap();
        let text = String::from_utf8(B64.decode(signature).unwrap()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "untrusted comment: signature from tauri secret key"
        );
        assert!(lines[2].starts_with("trusted comment: timestamp:"));
        assert!(lines[2].ends_with("\tfile:MonoCode.app.tar.gz"));
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn reads_the_key_from_a_file_path() {
        let keys = generate_keys(None).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("updater.key");
        std::fs::write(&key_path, format!("{}\n", keys.private_key)).unwrap();
        let package = dir.path().join("MonoCode.app.tar.gz");
        std::fs::write(&package, b"tarball").unwrap();

        let sk = decode_secret_key(key_path.to_str().unwrap(), None).unwrap();
        let sig_path = sign_file(&sk, &package).unwrap();

        assert_eq!(sig_path, dir.path().join("MonoCode.app.tar.gz.sig"));
        let signature = std::fs::read_to_string(sig_path).unwrap();
        verify_signature(b"tarball", &signature, &keys.public_key).unwrap();
    }
}

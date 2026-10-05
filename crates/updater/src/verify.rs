//! Minisign verification, as tauri-plugin-updater 2.10.1 did it
//! (`verify_signature` in `src/updater.rs`, Apache-2.0 OR MIT).
//!
//! The public key and the signature both arrive as base64 of the minisign
//! text files: the key from the build, the signature from the feed (the
//! contents of the release's `.sig` file).

use base64::Engine as _;
use minisign_verify::{PublicKey, Signature};

use crate::error::{Error, Result};

/// Checks `data` against `release_signature` with `pub_key`. Legacy
/// (non-prehashed) signatures are accepted, as the plugin accepted them.
pub fn verify_signature(data: &[u8], release_signature: &str, pub_key: &str) -> Result<()> {
    let pub_key_decoded = base64_to_string(pub_key)?;
    let public_key = PublicKey::decode(&pub_key_decoded)?;
    let signature_decoded = base64_to_string(release_signature)?;
    let signature = Signature::decode(&signature_decoded)?;
    public_key.verify(data, &signature, true)?;
    Ok(())
}

fn base64_to_string(base64_string: &str) -> Result<String> {
    let decoded = base64::engine::general_purpose::STANDARD.decode(base64_string.trim())?;
    String::from_utf8(decoded).map_err(|_| Error::SignatureUtf8(base64_string.into()))
}

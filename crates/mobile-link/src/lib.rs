//! Direct, mutually authenticated QUIC used by desktop and the Expo native module.
mod ffi;
mod tls;
mod transcript;
mod transport;

use base64::{engine::general_purpose::STANDARD, Engine};
use p256::ecdsa::{
    signature::{Signer, Verifier},
    Signature, SigningKey, VerifyingKey,
};
use p256::pkcs8::DecodePrivateKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

pub use transport::{
    exchange, read_frame, serve, server_endpoint, watch, write_frame, Watch, WatchSink,
};
pub type LinkResult<T> = Result<T, String>;
pub const VERSION: u32 = 1;
pub const PORT: u16 = 38416;
pub const ALPN: &[u8] = b"monocode-mobile/1";
/// Room for a message's attachments: the phone caps them at 24 MiB of base64.
pub const REQUEST_LIMIT: usize = 32 * 1024 * 1024;
pub const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub certificate: String,
    pub private_key: String,
}
impl Identity {
    pub fn generate() -> LinkResult<Self> {
        let key = rcgen::KeyPair::generate().map_err(|e| e.to_string())?;
        let cert = rcgen::CertificateParams::new(vec!["monocode.local".into()])
            .map_err(|e| e.to_string())?
            .self_signed(&key)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            certificate: STANDARD.encode(cert.der()),
            private_key: STANDARD.encode(key.serialize_der()),
        })
    }
    pub fn certificate_der(&self) -> LinkResult<Vec<u8>> {
        decode(&self.certificate)
    }
    pub fn key_der(&self) -> LinkResult<Vec<u8>> {
        decode(&self.private_key)
    }
    pub fn fingerprint(&self) -> LinkResult<String> {
        Ok(fingerprint(&self.certificate_der()?))
    }
    pub fn proof(&self, invitation: &str) -> LinkResult<String> {
        let key =
            SigningKey::from_pkcs8_der(&self.key_der()?).map_err(|_| "Invalid device identity")?;
        let signature: Signature = key.sign(&proof_message(invitation));
        Ok(STANDARD.encode(signature.to_bytes()))
    }
}
fn proof_message(invitation: &str) -> Vec<u8> {
    format!("monocode-mobile-enroll/v1:{invitation}").into_bytes()
}
pub fn verify_proof(certificate: &str, invitation: &str, proof: &str) -> LinkResult<String> {
    let der = decode(certificate)?;
    let (rest, cert) =
        x509_parser::parse_x509_certificate(&der).map_err(|_| "Invalid device certificate")?;
    if !rest.is_empty() {
        return Err("Invalid device certificate".into());
    }
    let key = VerifyingKey::from_sec1_bytes(&cert.public_key().subject_public_key.data)
        .map_err(|_| "Invalid device key")?;
    let signature = Signature::from_slice(&decode(proof)?).map_err(|_| "Invalid device proof")?;
    key.verify(&proof_message(invitation), &signature)
        .map_err(|_| "Device could not prove its identity")?;
    Ok(fingerprint(&der))
}
pub fn decode(value: &str) -> LinkResult<Vec<u8>> {
    if value.len() > 16_384 {
        return Err("Identity is too large".into());
    }
    STANDARD
        .decode(value)
        .map_err(|_| "Invalid identity encoding".into())
}
pub fn fingerprint(certificate: &[u8]) -> String {
    Sha256::digest(certificate)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    pub host: String,
    pub port: u16,
}
impl Address {
    pub fn parse(value: &str) -> LinkResult<Self> {
        let url = url_parts(value)?;
        Ok(url)
    }
}
fn url_parts(value: &str) -> LinkResult<Address> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 253
        || value
            .chars()
            .any(|c| c.is_whitespace() || "/?#@".contains(c))
    {
        return Err("Enter a hostname or IP address with an optional port".into());
    }
    let (host, port) = if let Some(value) = value.strip_prefix('[') {
        let (host, tail) = value.split_once(']').ok_or("Invalid IPv6 address")?;
        host.parse::<std::net::Ipv6Addr>()
            .map_err(|_| "Invalid IPv6 address")?;
        (
            host,
            if tail.is_empty() {
                PORT
            } else {
                tail.strip_prefix(':')
                    .ok_or("Invalid port")?
                    .parse()
                    .map_err(|_| "Invalid port")?
            },
        )
    } else if value.matches(':').count() == 1 {
        let (host, port) = value.split_once(':').unwrap();
        (host, port.parse().map_err(|_| "Invalid port")?)
    } else {
        (value, PORT)
    };
    if host.is_empty()
        || port == 0
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:_".contains(c))
    {
        return Err("Invalid address".into());
    }
    Ok(Address {
        host: host.into(),
        port,
    })
}

/// TLS admits only paired identities and unexpired invitation identities.
#[derive(Clone, Default, Debug)]
pub struct Gate(pub Arc<RwLock<HashMap<String, Option<u64>>>>);
impl Gate {
    pub fn allow(&self, key: String, expires: Option<u64>) {
        self.0.write().unwrap().insert(key, expires);
    }
    pub fn remove(&self, key: &str) {
        self.0.write().unwrap().remove(key);
    }
    pub fn admits(&self, key: &str) -> bool {
        self.0
            .read()
            .unwrap()
            .get(key)
            .is_some_and(|expires| expires.is_none_or(|expires| expires > now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enrollment_proof_binds_both_device_and_invitation() {
        let device = Identity::generate().unwrap();
        let other = Identity::generate().unwrap();
        let proof = device.proof("invite-a").unwrap();
        assert_eq!(
            verify_proof(&device.certificate, "invite-a", &proof).unwrap(),
            device.fingerprint().unwrap()
        );
        assert!(verify_proof(&other.certificate, "invite-a", &proof).is_err());
        assert!(verify_proof(&device.certificate, "invite-b", &proof).is_err());
    }
    #[test]
    fn expired_and_revoked_keys_are_refused() {
        let gate = Gate::default();
        gate.allow("expired".into(), Some(now() - 1));
        gate.allow("paired".into(), None);
        assert!(!gate.admits("expired"));
        assert!(gate.admits("paired"));
        gate.remove("paired");
        assert!(!gate.admits("paired"));
    }
    #[test]
    fn addresses_do_not_accept_credentials_or_routes() {
        assert_eq!(Address::parse("host.example:4000").unwrap().port, 4000);
        assert_eq!(Address::parse("[::1]:4000").unwrap().host, "::1");
        for value in [
            "",
            "host:0",
            "https://host",
            "user@host",
            "host/path",
            "host?token=abc",
        ] {
            assert!(Address::parse(value).is_err(), "{value}");
        }
    }
}

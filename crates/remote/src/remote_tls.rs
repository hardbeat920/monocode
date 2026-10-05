//! TLS to a paired host. Hosts use a self-signed certificate; the pairing
//! link carries its SHA-256 fingerprint, which this verifier pins. Hostname
//! and CA checks do not apply: the pin is the identity. The handshake
//! signature is still verified, so only the holder of the certificate's
//! private key can complete it.
//!
//! Moved from src-tauri/src/remote_tls.rs.
use base64::Engine as _;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, SignatureScheme};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
struct Pinned {
    fingerprint: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if Sha256::digest(end_entity.as_ref()).as_slice() == self.fingerprint.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Decodes a pairing-link fingerprint: base64url SHA-256, 43 characters.
pub fn decode_fingerprint(value: &str) -> Result<Vec<u8>, String> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value.trim())
        .map_err(|_| "The pairing link has an invalid certificate fingerprint")?;
    if bytes.len() != 32 {
        return Err("The pairing link has an invalid certificate fingerprint".into());
    }
    Ok(bytes)
}

/// An HTTP agent for one host. `fingerprint` pins its certificate for
/// `https://` URLs; plain `http://` is used only for loopback forwards.
pub fn agent(
    fingerprint: Option<&str>,
    connect: Duration,
    total: Duration,
) -> Result<ureq::Agent, String> {
    let mut builder = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(connect)
        .timeout(total);
    if let Some(fingerprint) = fingerprint {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pinned {
                fingerprint: decode_fingerprint(fingerprint)?,
                provider,
            }))
            .with_no_client_auth();
        builder = builder.tls_config(Arc::new(config));
    }
    Ok(builder.build())
}

/// Whether a failed request ended in the TLS handshake, before any request
/// bytes were sent, and whether the certificate did not match the pin.
pub fn handshake_failure(error: &(dyn std::error::Error + 'static)) -> Option<bool> {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        // io::Error::source() skips the error it wraps, so look inside it.
        let inner = error
            .downcast_ref::<std::io::Error>()
            .and_then(|io| io.get_ref())
            .map(|inner| inner as &(dyn std::error::Error + 'static));
        for candidate in [Some(error), inner].into_iter().flatten() {
            if let Some(tls) = candidate.downcast_ref::<Error>() {
                return Some(matches!(
                    tls,
                    Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
                ));
            }
        }
        source = error.source();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_must_be_sha256() {
        assert_eq!(decode_fingerprint(&"A".repeat(43)).unwrap().len(), 32);
        assert!(decode_fingerprint("short").is_err());
        assert!(decode_fingerprint(&"A".repeat(44)).is_err());
        assert!(agent(Some("bad"), Duration::from_secs(1), Duration::from_secs(1)).is_err());
        assert!(agent(None, Duration::from_secs(1), Duration::from_secs(1)).is_ok());
    }
}

use crate::{fingerprint, Gate, Identity, LinkResult, ALPN};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    DigitallySignedStruct, DistinguishedName, Error, SignatureScheme,
};
use std::sync::Arc;

#[derive(Debug)]
struct Verifier {
    gate: Gate,
}
impl Verifier {
    fn signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
    fn check(&self, cert: &CertificateDer<'_>) -> Result<(), Error> {
        if self.gate.admits(&fingerprint(cert.as_ref())) {
            Ok(())
        } else {
            Err(Error::General("Device identity is not trusted".into()))
        }
    }
}
impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.check(cert)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("TLS 1.3 required".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.signature(message, cert, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}
impl ClientCertVerifier for Verifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        self.check(cert)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("TLS 1.3 required".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.signature(message, cert, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}
pub fn server(identity: &Identity, gate: Gate) -> LinkResult<quinn::ServerConfig> {
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|e| e.to_string())?
    .with_client_cert_verifier(Arc::new(Verifier { gate }))
    .with_single_cert(
        vec![CertificateDer::from(identity.certificate_der()?)],
        PrivatePkcs8KeyDer::from(identity.key_der()?).into(),
    )
    .map_err(|e| e.to_string())?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto =
        quinn::crypto::rustls::QuicServerConfig::try_from(tls).map_err(|e| e.to_string())?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(16_u32.into());
    transport.max_concurrent_uni_streams(0_u32.into());
    transport.max_idle_timeout(Some(std::time::Duration::from_secs(30).try_into().unwrap()));
    config.transport_config(Arc::new(transport));
    Ok(config)
}
pub fn client(identity: &Identity, pin: &str) -> LinkResult<quinn::ClientConfig> {
    if pin.len() != 64 || !pin.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid desktop identity".into());
    }
    let gate = Gate::default();
    gate.allow(pin.to_lowercase(), None);
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|e| e.to_string())?
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(Verifier { gate }))
    .with_client_auth_cert(
        vec![CertificateDer::from(identity.certificate_der()?)],
        PrivatePkcs8KeyDer::from(identity.key_der()?).into(),
    )
    .map_err(|e| e.to_string())?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto =
        quinn::crypto::rustls::QuicClientConfig::try_from(tls).map_err(|e| e.to_string())?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    // Connections stay open between requests: pings keep an idle one alive,
    // and one the desktop no longer answers fails within seconds rather than
    // holding up the next request.
    let mut transport = quinn::TransportConfig::default();
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(4)));
    transport.max_idle_timeout(Some(std::time::Duration::from_secs(12).try_into().unwrap()));
    config.transport_config(Arc::new(transport));
    Ok(config)
}

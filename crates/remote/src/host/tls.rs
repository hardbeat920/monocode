//! Port of host/tls.ts.
//!
//! The host's TLS identity is a self-signed ECDSA P-256 certificate in
//! `<data dir>/tls`. Desktops pin its SHA-256 fingerprint from the pairing
//! link, so an existing `cert.pem` and `key.pem` must keep being used as they
//! are. New certificates have the layout `host/tls.ts` wrote: a CN subject,
//! a non-critical basicConstraints that is not a CA, and the DNS name
//! `monocode-host`, valid from a day ago for 20 years.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use rcgen::{
    CertificateParams, CustomExtension, DistinguishedName, DnType, DnValue, IsCa, KeyPair,
    SerialNumber,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

/// The host's TLS identity. Desktops pin `fingerprint`, the SHA-256 of the
/// certificate, so the certificate needs no public CA or hostname.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostIdentity {
    /// PKCS #8 private key, PEM.
    pub key: String,
    /// X.509 certificate, PEM.
    pub cert: String,
    /// base64url SHA-256 of the certificate's DER bytes, 43 characters.
    pub fingerprint: String,
}

fn pem(label: &str, der: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let lines: Vec<&str> = encoded
        .as_bytes()
        .chunks(64)
        .map(|line| std::str::from_utf8(line).unwrap_or_default())
        .collect();
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        lines.join("\n")
    )
}

fn date(time: SystemTime) -> Result<time::OffsetDateTime, String> {
    let seconds = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    time::OffsetDateTime::from_unix_timestamp(seconds as i64).map_err(|error| error.to_string())
}

/// DER of `SEQUENCE { [2] IA5String name }`, a subjectAltName with one DNS
/// name.
fn dns_alt_name(name: &str) -> Vec<u8> {
    let mut entry = vec![0x82, name.len() as u8];
    entry.extend_from_slice(name.as_bytes());
    let mut sequence = vec![0x30, entry.len() as u8];
    sequence.extend(entry);
    sequence
}

pub fn create_host_certificate(common_name: &str, now: SystemTime) -> Result<HostIdentity, String> {
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(|e| e.to_string())?;
    let mut serial = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut serial)
        .map_err(|_| "The system random source failed".to_string())?;
    serial[0] &= 0x7f; // a positive INTEGER
    serial[0] |= 0x01; // with no leading zero byte
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, DnValue::Utf8String(common_name.into()));
    let mut params = CertificateParams::default();
    params.serial_number = Some(SerialNumber::from_slice(&serial));
    params.distinguished_name = name;
    params.is_ca = IsCa::NoCa;
    params.not_before = date(now - Duration::from_secs(24 * 3600))?;
    params.not_after = date(now + Duration::from_secs(20 * 365 * 24 * 3600))?;
    params.custom_extensions = vec![
        // basicConstraints: not a CA
        CustomExtension::from_oid_content(&[2, 5, 29, 19], vec![0x30, 0x00]),
        // subjectAltName: dNSName monocode-host
        CustomExtension::from_oid_content(&[2, 5, 29, 17], dns_alt_name("monocode-host")),
    ];
    let certificate = params.self_signed(&key).map_err(|e| e.to_string())?;
    let cert = pem("CERTIFICATE", certificate.der());
    Ok(HostIdentity {
        key: pem("PRIVATE KEY", &key.serialize_der()),
        fingerprint: certificate_fingerprint(&cert)?,
        cert,
    })
}

pub fn certificate_fingerprint(pem: &str) -> Result<String, String> {
    let der = CertificateDer::from_pem_slice(pem.as_bytes())
        .map_err(|_| "The host certificate is not valid PEM".to_string())?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(der.as_ref())))
}

fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(&mut options.open(path)?, contents.as_bytes())
}

/// `mkdtempSync(prefix)`: a new private directory with a random suffix.
pub(crate) fn make_temporary_dir(prefix: &Path) -> std::io::Result<std::path::PathBuf> {
    loop {
        let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..6].into();
        let path = std::path::PathBuf::from(format!("{}{suffix}", prefix.display()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Reads the identity in `<directory>/tls`, creating it on first use. The
/// CLI and a starting host may race; each writes a complete temporary
/// directory and only one rename wins, so both end up with the same
/// certificate.
pub fn load_host_identity(directory: &Path) -> Result<HostIdentity, String> {
    let folder = directory.join("tls");
    let read = || -> std::io::Result<HostIdentity> {
        let cert = fs::read_to_string(folder.join("cert.pem"))?;
        let key = fs::read_to_string(folder.join("key.pem"))?;
        let fingerprint = certificate_fingerprint(&cert)
            .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))?;
        Ok(HostIdentity {
            key,
            cert,
            fingerprint,
        })
    };
    match read() {
        Ok(identity) => return Ok(identity),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let identity = create_host_certificate("MonoCode Host", SystemTime::now())?;
    let temporary = make_temporary_dir(&directory.join(".tls-")).map_err(|e| e.to_string())?;
    let written = write_private(&temporary.join("key.pem"), &identity.key)
        .and_then(|_| write_private(&temporary.join("cert.pem"), &identity.cert))
        .and_then(|_| fs::rename(&temporary, &folder));
    match written {
        Ok(()) => Ok(identity),
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            // Another process created the folder first.
            if folder.join("cert.pem").exists()
                || matches!(
                    error.kind(),
                    ErrorKind::AlreadyExists
                        | ErrorKind::DirectoryNotEmpty
                        | ErrorKind::PermissionDenied
                )
            {
                read().map_err(|error| error.to_string())
            } else {
                Err(error.to_string())
            }
        }
    }
}

/// The rustls server configuration for an identity: TLS 1.2 or newer, as
/// the TypeScript listener's `minVersion`.
pub fn server_config(identity: &HostIdentity) -> Result<Arc<rustls::ServerConfig>, String> {
    let certs = CertificateDer::pem_slice_iter(identity.cert.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "The host certificate is not valid PEM".to_string())?;
    let key = PrivateKeyDer::from_pem_slice(identity.key.as_bytes())
        .map_err(|_| "The host private key is not valid PEM".to_string())?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|error| error.to_string())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use x509_parser::prelude::{FromDer, GeneralName, X509Certificate};

    // Generated by host/tls.ts, as in remote.rs's tests. Inline because the
    // repository ignores .pem and .key files. Test only.
    pub(crate) const NODE_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBXjCCAQWgAwIBAgIQcT7HxNtLrgCn81joEqO4JjAKBggqhkjOPQQDAjAdMRsw
GQYDVQQDDBJNb25vQ29kZSB0ZXN0IGhvc3QwHhcNMjUxMjMxMDAwMDAwWhcNNDUx
MjI3MDAwMDAwWjAdMRswGQYDVQQDDBJNb25vQ29kZSB0ZXN0IGhvc3QwWTATBgcq
hkjOPQIBBggqhkjOPQMBBwNCAASLCXUeQOtw5x9bmnqffwefGVFH2NWOCgAQCZOp
9ryb4fYlBO4HfeeJrWwh6XNlAg5VL5wWSUBdgwZut41c+ND7oycwJTAJBgNVHRME
AjAAMBgGA1UdEQQRMA+CDW1vbm9jb2RlLWhvc3QwCgYIKoZIzj0EAwIDRwAwRAIg
aKsltyMKcOisqD8EiaVH2+9jbNaov2BGuizMOUVxRfMCIBJKDYslwBN2x5t/39C0
2CdA6i4dNG1LEY4kAJb88byy
-----END CERTIFICATE-----
";
    pub(crate) const NODE_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgLYVZ/ljgngB0PYx7
xupDHdtkkbtP1uPFMs0c1xkn57OhRANCAASLCXUeQOtw5x9bmnqffwefGVFH2NWO
CgAQCZOp9ryb4fYlBO4HfeeJrWwh6XNlAg5VL5wWSUBdgwZut41c+ND7
-----END PRIVATE KEY-----
";
    pub(crate) const NODE_FINGERPRINT: &str = "-EcUWBSssurVvQMouW-dKgMaHMfKn1kMzWGr6lKqIIc";

    fn at(text: &str) -> SystemTime {
        let date =
            time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
                .unwrap();
        SystemTime::UNIX_EPOCH + Duration::from_secs(date.unix_timestamp() as u64)
    }

    fn der(identity: &HostIdentity) -> Vec<u8> {
        CertificateDer::from_pem_slice(identity.cert.as_bytes())
            .unwrap()
            .as_ref()
            .to_vec()
    }

    /// Serves one TLS connection and returns the fingerprint a pinned client saw.
    fn handshake(identity: &HostIdentity) -> String {
        let config = server_config(identity).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let connection = rustls::ServerConnection::new(config).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
            );
            stream.conn.send_close_notify();
            let _ = stream.flush();
        });
        let agent = crate::remote_tls::agent(
            Some(&identity.fingerprint),
            Duration::from_secs(5),
            Duration::from_secs(5),
        )
        .unwrap();
        let body = agent
            .get(&format!("https://127.0.0.1:{port}/"))
            .call()
            .unwrap()
            .into_string()
            .unwrap();
        server.join().unwrap();
        assert_eq!(body, "hello");
        identity.fingerprint.clone()
    }

    #[test]
    fn creates_a_valid_self_signed_certificate_that_a_tls_server_accepts() {
        let identity = create_host_certificate("Test host", at("2026-01-01T00:00:00Z")).unwrap();
        let der = der(&identity);
        let (_, certificate) = X509Certificate::from_der(&der).unwrap();
        assert_eq!(certificate.subject().to_string(), "CN=Test host");
        assert_eq!(certificate.issuer().to_string(), "CN=Test host");
        certificate.verify_signature(None).unwrap();
        let names = certificate.subject_alternative_name().unwrap().unwrap();
        assert_eq!(
            names.value.general_names,
            vec![GeneralName::DNSName("monocode-host")]
        );
        assert!(!names.critical);
        let constraints = certificate.basic_constraints().unwrap().unwrap();
        assert!(!constraints.value.ca);
        assert!(!constraints.critical);
        assert_eq!(certificate.extensions().len(), 2);
        assert_eq!(certificate.validity().not_after.to_datetime().year(), 2045);
        assert_eq!(certificate.version().0, 2);
        assert_eq!(
            identity.fingerprint,
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(&der))
        );
        assert_eq!(identity.fingerprint.len(), 43);
        assert!(identity.cert.lines().all(|line| line.len() <= 64));
        assert!(identity.key.starts_with("-----BEGIN PRIVATE KEY-----\n"));
        assert_eq!(handshake(&identity), identity.fingerprint);
    }

    #[test]
    fn uses_generalized_time_for_validity_dates_from_2050() {
        let identity = create_host_certificate("Later", at("2040-06-01T00:00:00Z")).unwrap();
        let der = der(&identity);
        let (_, certificate) = X509Certificate::from_der(&der).unwrap();
        assert_eq!(certificate.validity().not_after.to_datetime().year(), 2060);
        // GeneralizedTime is tag 0x18 followed by 15 bytes, YYYYMMDDHHMMSSZ.
        assert!(der.windows(2).any(|pair| pair == [0x18, 0x0f]));
        assert_eq!(
            certificate_fingerprint(&identity.cert).unwrap(),
            identity.fingerprint
        );
    }

    #[test]
    fn creates_one_identity_per_data_directory_and_reuses_it() {
        let directory = crate::host::store::tests::temporary("monocode-tls-");
        let first = load_host_identity(directory.path()).unwrap();
        let second = load_host_identity(directory.path()).unwrap();
        assert_eq!(second, first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(directory.path().join("tls").join("key.pem"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
    }

    #[test]
    fn keeps_serving_a_certificate_that_the_typescript_host_created() {
        let directory = crate::host::store::tests::temporary("monocode-tls-");
        let folder = directory.path().join("tls");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("cert.pem"), NODE_CERT).unwrap();
        fs::write(folder.join("key.pem"), NODE_KEY).unwrap();
        let identity = load_host_identity(directory.path()).unwrap();
        assert_eq!(identity.fingerprint, NODE_FINGERPRINT);
        assert_eq!(handshake(&identity), NODE_FINGERPRINT);
    }
}

//! TLS for FTPS: checking the server's certificate.
//!
//! A certificate is accepted when an authority this system trusts vouches for it (the
//! usual web check, against the system's certificate store), or when the user chose to
//! trust that exact certificate for that server before: its SHA-256 fingerprint is
//! remembered in `trusted-certificates.json` in the config folder. That second way is
//! what makes self-signed certificates, common on FTP servers, usable; it is the same
//! "do you trust this server?" decision as for an unknown SSH host key.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::{self, ConfigError};

/// Why a certificate was refused, recorded during the handshake for the error message.
#[derive(Debug, Clone)]
pub(super) struct Rejected {
    /// SHA-256 fingerprint of the server's certificate.
    pub(super) fingerprint: String,
    /// What the check found.
    pub(super) reason: String,
}

/// Where the handshake records a refused certificate.
pub(super) type RejectedSlot = Arc<Mutex<Option<Rejected>>>;

/// Certificates the user trusts, by server ("host:port").
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct TrustedCertificates {
    servers: BTreeMap<String, Vec<String>>,
}

/// Where trusted certificates are remembered.
fn trusted_path() -> PathBuf {
    config::config_dir().join("trusted-certificates.json")
}

/// The key of a server in the trusted list.
fn server_key(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

/// Remember that the user trusts the certificate with `fingerprint` for this server.
///
/// # Errors
///
/// Returns a [`ConfigError`] if the list cannot be read or written
pub fn trust_certificate(host: &str, port: u16, fingerprint: &str) -> Result<(), ConfigError> {
    let path = trusted_path();
    let mut trusted: TrustedCertificates = config::load_json(&path)?;
    let list = trusted.servers.entry(server_key(host, port)).or_default();
    if !list.iter().any(|known| known == fingerprint) {
        list.push(fingerprint.to_owned());
    }
    config::save_json(&trusted, &path)?;
    tracing::info!("trusted the TLS certificate {fingerprint} for {host}:{port}");
    Ok(())
}

/// Fingerprints of the certificates the user trusts for this server.
fn trusted_for(host: &str, port: u16) -> Vec<String> {
    match config::load_json::<TrustedCertificates>(&trusted_path()) {
        Ok(mut trusted) => trusted
            .servers
            .remove(&server_key(host, port))
            .unwrap_or_default(),
        Err(error) => {
            tracing::warn!("{error}; no certificates are trusted by fingerprint");
            Vec::new()
        }
    }
}

/// SHA-256 fingerprint of a certificate, as browsers show it ("SHA256:AB:CD:...").
fn fingerprint(certificate: &CertificateDer<'_>) -> String {
    let digest = Sha256::digest(certificate.as_ref());
    let mut text = String::from("SHA256:");
    for (index, byte) in digest.iter().enumerate() {
        if index > 0 {
            text.push(':');
        }
        // Writing into a `String` cannot fail.
        let _ = write!(text, "{byte:02X}");
    }
    text
}

/// Why a certificate was refused, in plain words.
fn plain_reason(error: &rustls::Error) -> String {
    use rustls::CertificateError as Problem;
    let rustls::Error::InvalidCertificate(problem) = error else {
        return error.to_string();
    };
    match problem {
        Problem::UnknownIssuer => "it is not signed by an authority this computer trusts",
        Problem::Expired | Problem::ExpiredContext { .. } => "it has expired",
        Problem::NotValidYet | Problem::NotValidYetContext { .. } => "it is not valid yet",
        Problem::NotValidForName | Problem::NotValidForNameContext { .. } => {
            "it was issued for a different server name"
        }
        Problem::Revoked => "it has been revoked",
        // webpki's way of saying the certificate signed itself.
        Problem::Other(other) if format!("{other:?}").contains("CaUsedAsEndEntity") => {
            "it is self-signed"
        }
        other => return format!("{other:?}"),
    }
    .to_owned()
}

/// TLS settings for connecting to `host:port`, and where a refused certificate is
/// recorded.
///
/// # Errors
///
/// Returns a message if the TLS settings cannot be built (should not happen)
pub(super) fn client_config(
    host: &str,
    port: u16,
) -> Result<(Arc<ClientConfig>, RejectedSlot), String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for error in &native.errors {
        tracing::debug!("system certificate store: {error}");
    }
    let (added, ignored) = roots.add_parsable_certificates(native.certs);
    tracing::debug!("{added} trusted certificate authorities loaded ({ignored} unusable)");
    let webpki =
        WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
            .build()
            .inspect_err(|error| {
                tracing::warn!(
                    "cannot check certificates against the system's authorities: {error}"
                )
            })
            .ok();
    let rejected: RejectedSlot = Arc::default();
    let verifier = Verifier {
        webpki,
        provider: Arc::clone(&provider),
        trusted: trusted_for(host, port),
        rejected: Arc::clone(&rejected),
    };
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("TLS setup failed: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok((Arc::new(config), rejected))
}

/// Checks server certificates (see the module documentation).
#[derive(Debug)]
struct Verifier {
    /// The usual check against the system's authorities; `None` if the system has none.
    webpki: Option<Arc<WebPkiServerVerifier>>,
    provider: Arc<CryptoProvider>,
    /// Fingerprints the user trusts for this server.
    trusted: Vec<String>,
    rejected: RejectedSlot,
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint = fingerprint(end_entity);
        if self.trusted.contains(&fingerprint) {
            return Ok(ServerCertVerified::assertion());
        }
        let result = match &self.webpki {
            Some(webpki) => webpki.verify_server_cert(
                end_entity,
                intermediates,
                server_name,
                ocsp_response,
                now,
            ),
            None => Err(rustls::Error::General(String::from(
                "this system has no trusted certificate authorities",
            ))),
        };
        if let Err(error) = &result
            && let Ok(mut slot) = self.rejected.lock()
        {
            *slot = Some(Rejected {
                fingerprint,
                reason: plain_reason(error),
            });
        }
        result
    }

    // The handshake signatures are checked the same way whichever way the certificate
    // was accepted: they prove the server holds the certificate's private key.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
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
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_reason_explains_common_problems() {
        let invalid = |problem| rustls::Error::InvalidCertificate(problem);
        assert_eq!(
            plain_reason(&invalid(rustls::CertificateError::UnknownIssuer)),
            "it is not signed by an authority this computer trusts"
        );
        assert_eq!(
            plain_reason(&invalid(rustls::CertificateError::Expired)),
            "it has expired"
        );
    }

    #[test]
    fn fingerprint_is_colon_separated_sha256() {
        let text = fingerprint(&CertificateDer::from(b"abc".to_vec()));
        assert!(text.starts_with("SHA256:BA:78:16:BF:"));
        // 32 bytes: 32 pairs of hex digits and 31 colons after the prefix.
        assert_eq!(text.len(), "SHA256:".len() + 32 * 2 + 31);
    }
}

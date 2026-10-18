//! Portable export and import of saved sites, for moving them to another installation.
//!
//! The export is a JSON file holding every site's connection details. Passwords can be
//! included, in which case each is encrypted:
//!
//! * a 256-bit key is derived from a passphrase the user chooses, with **Argon2id** (a
//!   memory-hard password hash) and a random 16-byte **salt** stored in the file;
//! * each password is encrypted with **XChaCha20-Poly1305** under that key, with its own
//!   random 24-byte nonce and the site id as authenticated data, so a password cannot be
//!   moved to another site or altered without detection.
//!
//! The passphrase itself is never stored; a wrong one is reported on import.

use std::fs;
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::config;
use crate::sites::{Protocol, Site};

/// Value of the `format` field, identifying GuiSCP site exports.
const FORMAT: &str = "guiscp-sites";
/// Current export format version.
const VERSION: u32 = 1;
/// Name of the key derivation function recorded in the file.
const KDF_NAME: &str = "argon2id";
/// Name of the cipher recorded in the file.
const CIPHER_NAME: &str = "xchacha20poly1305";
/// Salt length in bytes.
const SALT_LEN: usize = 16;
/// XChaCha20 nonce length in bytes.
const NONCE_LEN: usize = 24;
/// Shortest passphrase accepted for encrypting passwords.
pub const MIN_PASSPHRASE_CHARS: usize = 8;

/// Argon2id cost settings, stored in the file so imports can repeat the derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfCost {
    /// Memory to use, in KiB.
    pub memory_kib: u32,
    /// Number of passes over the memory.
    pub iterations: u32,
    /// Degree of parallelism.
    pub parallelism: u32,
}

/// Costs recommended by OWASP for Argon2id (19 MiB, 2 passes, 1 lane).
pub const DEFAULT_COST: KdfCost = KdfCost {
    memory_kib: 19 * 1024,
    iterations: 2,
    parallelism: 1,
};

/// Error exporting or importing sites.
#[derive(Debug, Error)]
pub enum ExportError {
    /// The file could not be read or written.
    #[error("cannot access {}: {source}", path.display())]
    Io {
        /// File involved.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Writing the file failed.
    #[error(transparent)]
    Write(#[from] config::ConfigError),
    /// The file is not valid JSON.
    #[error("{} is not a valid site export: {source}", path.display())]
    Json {
        /// File involved.
        path: PathBuf,
        /// Underlying parse error.
        #[source]
        source: serde_json::Error,
    },
    /// The file is JSON but not a GuiSCP site export.
    #[error("the file is not a GuiSCP site export")]
    NotAnExport,
    /// The file was written by a newer version of the app.
    #[error("unsupported export version {0}")]
    UnsupportedVersion(u32),
    /// The file holds encrypted passwords but no passphrase was given.
    #[error("a passphrase is needed to import the saved passwords")]
    PassphraseRequired,
    /// Decryption failed: wrong passphrase, or the file was altered.
    #[error("wrong passphrase, or the file has been altered")]
    WrongPassphrase,
    /// The encryption settings in the file are invalid.
    #[error("invalid encryption settings in the file: {0}")]
    BadSettings(String),
    /// The system random number source failed.
    #[error("cannot generate random data: {0}")]
    Random(#[from] getrandom::Error),
}

/// Encryption settings recorded in an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptionInfo {
    /// Key derivation function (always "argon2id").
    pub kdf: String,
    /// Base64 salt for the key derivation.
    pub salt: String,
    /// Argon2 memory cost in KiB.
    pub memory_kib: u32,
    /// Argon2 passes.
    pub iterations: u32,
    /// Argon2 parallelism.
    pub parallelism: u32,
    /// Cipher (always "xchacha20poly1305").
    pub cipher: String,
}

/// One site in an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedSite {
    /// The site's id.
    pub id: String,
    /// Protocol used to connect.
    pub protocol: Protocol,
    /// Server host name or IP address.
    pub host: String,
    /// Server port.
    pub port: u16,
    /// Login user name.
    pub user: String,
    /// Base64 of nonce followed by ciphertext, when the password was exported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

/// Contents of an export file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFile {
    /// Always "guiscp-sites".
    pub format: String,
    /// Format version.
    pub version: u32,
    /// Present when any password is included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionInfo>,
    /// The exported sites.
    pub sites: Vec<ExportedSite>,
}

impl ExportFile {
    /// Whether the file holds encrypted passwords (and so needs a passphrase).
    pub fn has_passwords(&self) -> bool {
        self.sites.iter().any(|site| site.password.is_some())
    }

    /// Read and check an export file.
    ///
    /// # Arguments
    ///
    /// * `path` - File to read
    ///
    /// # Errors
    ///
    /// Returns [`ExportError`] if the file cannot be read, is not a GuiSCP export, or
    /// comes from a newer version
    pub fn read(path: &Path) -> Result<Self, ExportError> {
        let text = fs::read_to_string(path).map_err(|source| ExportError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: Self = serde_json::from_str(&text).map_err(|source| ExportError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        if file.format != FORMAT {
            return Err(ExportError::NotAnExport);
        }
        if file.version > VERSION {
            return Err(ExportError::UnsupportedVersion(file.version));
        }
        Ok(file)
    }

    /// Write the export to `path`.
    ///
    /// # Arguments
    ///
    /// * `path` - File to write
    ///
    /// # Errors
    ///
    /// Returns [`ExportError::Write`] if the file cannot be written
    pub fn write(&self, path: &Path) -> Result<(), ExportError> {
        Ok(config::save_json(self, path)?)
    }
}

/// Build an export of `sites`.
///
/// # Arguments
///
/// * `sites` - Sites to export
/// * `passwords` - `(site id, password)` pairs to include; ignored without a passphrase
/// * `passphrase` - Passphrase protecting the passwords, or `None` to leave them out
/// * `cost` - Argon2id cost settings
///
/// # Errors
///
/// Returns [`ExportError`] if random data or the key cannot be produced
pub fn export_sites(
    sites: &[Site],
    passwords: &[(String, SecretString)],
    passphrase: Option<&SecretString>,
    cost: KdfCost,
) -> Result<ExportFile, ExportError> {
    let protection = match passphrase.filter(|_| !passwords.is_empty()) {
        Some(passphrase) => {
            let mut salt = [0u8; SALT_LEN];
            getrandom::fill(&mut salt)?;
            let cipher = cipher_for(passphrase, &salt, cost)?;
            let info = EncryptionInfo {
                kdf: KDF_NAME.to_owned(),
                salt: BASE64.encode(salt),
                memory_kib: cost.memory_kib,
                iterations: cost.iterations,
                parallelism: cost.parallelism,
                cipher: CIPHER_NAME.to_owned(),
            };
            Some((info, cipher))
        }
        None => None,
    };

    let mut exported = Vec::with_capacity(sites.len());
    for site in sites {
        let password = match (&protection, passwords.iter().find(|(id, _)| *id == site.id)) {
            (Some((_, cipher)), Some((_, password))) => Some(encrypt(cipher, &site.id, password)?),
            _ => None,
        };
        exported.push(ExportedSite {
            id: site.id.clone(),
            protocol: site.protocol,
            host: site.host.clone(),
            port: site.port,
            user: site.user.clone(),
            password,
        });
    }
    Ok(ExportFile {
        format: FORMAT.to_owned(),
        version: VERSION,
        encryption: protection.map(|(info, _)| info),
        sites: exported,
    })
}

/// Unpack an export into sites and their decrypted passwords.
///
/// # Arguments
///
/// * `file` - The export, from [`ExportFile::read`]
/// * `passphrase` - Passphrase used when exporting; needed if the file has passwords
///
/// # Returns
///
/// Each site (with `password_saved` false; it becomes true once stored in the keyring)
/// and its password, if one was exported
///
/// # Errors
///
/// Returns [`ExportError::PassphraseRequired`] or [`ExportError::WrongPassphrase`] when
/// passwords cannot be decrypted, or [`ExportError::BadSettings`] for a damaged file
pub fn import_sites(
    file: &ExportFile,
    passphrase: Option<&SecretString>,
) -> Result<Vec<(Site, Option<SecretString>)>, ExportError> {
    let cipher = if file.has_passwords() {
        let passphrase = passphrase.ok_or(ExportError::PassphraseRequired)?;
        let info = file
            .encryption
            .as_ref()
            .ok_or_else(|| ExportError::BadSettings(String::from("missing")))?;
        if info.kdf != KDF_NAME || info.cipher != CIPHER_NAME {
            return Err(ExportError::BadSettings(format!(
                "{} / {}",
                info.kdf, info.cipher
            )));
        }
        let salt = BASE64
            .decode(&info.salt)
            .map_err(|error| ExportError::BadSettings(error.to_string()))?;
        let cost = KdfCost {
            memory_kib: info.memory_kib,
            iterations: info.iterations,
            parallelism: info.parallelism,
        };
        Some(cipher_for(passphrase, &salt, cost)?)
    } else {
        None
    };

    file.sites
        .iter()
        .map(|exported| {
            let password = match (&cipher, &exported.password) {
                (Some(cipher), Some(sealed)) => Some(decrypt(cipher, &exported.id, sealed)?),
                _ => None,
            };
            let site = Site {
                id: exported.id.clone(),
                protocol: exported.protocol,
                host: exported.host.clone(),
                port: exported.port,
                user: exported.user.clone(),
                password_saved: false,
            };
            Ok((site, password))
        })
        .collect()
}

/// Derive the encryption key from a passphrase and build the cipher.
///
/// # Arguments
///
/// * `passphrase` - User's passphrase
/// * `salt` - Random salt
/// * `cost` - Argon2id cost settings
///
/// # Errors
///
/// Returns [`ExportError::BadSettings`] if the cost settings or salt are invalid
fn cipher_for(
    passphrase: &SecretString,
    salt: &[u8],
    cost: KdfCost,
) -> Result<XChaCha20Poly1305, ExportError> {
    let params = Params::new(cost.memory_kib, cost.iterations, cost.parallelism, Some(32))
        .map_err(|error| ExportError::BadSettings(error.to_string()))?;
    // `Zeroizing` wipes the key from memory when it goes out of scope.
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.expose_secret().as_bytes(), salt, key.as_mut())
        .map_err(|error| ExportError::BadSettings(error.to_string()))?;
    Ok(XChaCha20Poly1305::new(Key::from_slice(key.as_ref())))
}

/// Encrypt one password, bound to its site id.
///
/// # Arguments
///
/// * `cipher` - Cipher from [`cipher_for`]
/// * `site_id` - Site the password belongs to (authenticated, not encrypted)
/// * `password` - The password
///
/// # Returns
///
/// Base64 of the nonce followed by the ciphertext
fn encrypt(
    cipher: &XChaCha20Poly1305,
    site_id: &str,
    password: &SecretString,
) -> Result<String, ExportError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce)?;
    let payload = Payload {
        msg: password.expose_secret().as_bytes(),
        aad: site_id.as_bytes(),
    };
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), payload)
        .map_err(|_| ExportError::BadSettings(String::from("encryption failed")))?;
    let mut sealed = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(sealed))
}

/// Decrypt one password produced by [`encrypt`].
///
/// # Arguments
///
/// * `cipher` - Cipher from [`cipher_for`]
/// * `site_id` - Site the password must belong to
/// * `sealed` - Base64 nonce and ciphertext
///
/// # Errors
///
/// Returns [`ExportError::WrongPassphrase`] if authentication fails
fn decrypt(
    cipher: &XChaCha20Poly1305,
    site_id: &str,
    sealed: &str,
) -> Result<SecretString, ExportError> {
    let sealed = Zeroizing::new(
        BASE64
            .decode(sealed)
            .map_err(|_| ExportError::WrongPassphrase)?,
    );
    if sealed.len() <= NONCE_LEN {
        return Err(ExportError::WrongPassphrase);
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
    let payload = Payload {
        msg: ciphertext,
        aad: site_id.as_bytes(),
    };
    let plain = Zeroizing::new(
        cipher
            .decrypt(XNonce::from_slice(nonce), payload)
            .map_err(|_| ExportError::WrongPassphrase)?,
    );
    let text = std::str::from_utf8(&plain).map_err(|_| ExportError::WrongPassphrase)?;
    Ok(SecretString::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sites::new_site_id;

    /// Cheap costs so tests run fast; real exports use [`DEFAULT_COST`].
    const TEST_COST: KdfCost = KdfCost {
        memory_kib: 64,
        iterations: 1,
        parallelism: 1,
    };

    fn site(host: &str) -> Site {
        Site {
            id: new_site_id(),
            protocol: Protocol::Sftp,
            host: host.to_owned(),
            port: 22,
            user: String::from("me"),
            password_saved: true,
        }
    }

    fn secret(text: &str) -> SecretString {
        SecretString::from(text)
    }

    #[test]
    fn round_trip_restores_sites_and_passwords() {
        // Arrange
        let sites = [site("a.example"), site("b.example")];
        let passwords = [(sites[0].id.clone(), secret("hunter2"))];

        // Act
        let file = export_sites(&sites, &passwords, Some(&secret("passphrase")), TEST_COST)
            .expect("export");
        let imported = import_sites(&file, Some(&secret("passphrase"))).expect("import");

        // Assert
        assert!(file.has_passwords());
        let text = serde_json::to_string(&file).expect("serialize");
        assert!(!text.contains("hunter2"), "password must be encrypted");
        assert_eq!(imported[0].0.host, "a.example");
        assert_eq!(
            imported[0].1.as_ref().map(|p| p.expose_secret().to_owned()),
            Some(String::from("hunter2"))
        );
        assert!(imported[1].1.is_none());
    }

    #[test]
    fn wrong_or_missing_passphrase_is_rejected() {
        let sites = [site("a.example")];
        let passwords = [(sites[0].id.clone(), secret("hunter2"))];
        let file = export_sites(&sites, &passwords, Some(&secret("right one")), TEST_COST)
            .expect("export");

        assert!(matches!(
            import_sites(&file, Some(&secret("wrong one"))),
            Err(ExportError::WrongPassphrase)
        ));
        assert!(matches!(
            import_sites(&file, None),
            Err(ExportError::PassphraseRequired)
        ));
    }

    #[test]
    fn password_moved_to_another_site_is_detected() {
        // Arrange
        let sites = [site("a.example"), site("b.example")];
        let passwords = [(sites[0].id.clone(), secret("hunter2"))];
        let mut file = export_sites(&sites, &passwords, Some(&secret("pass phrase")), TEST_COST)
            .expect("export");

        // Act: swap the encrypted password onto the other site.
        file.sites[1].password = file.sites[0].password.take();

        // Assert
        assert!(matches!(
            import_sites(&file, Some(&secret("pass phrase"))),
            Err(ExportError::WrongPassphrase)
        ));
    }

    #[test]
    fn export_without_passphrase_has_no_passwords() {
        let sites = [site("a.example")];
        let passwords = [(sites[0].id.clone(), secret("hunter2"))];

        let file = export_sites(&sites, &passwords, None, TEST_COST).expect("export");

        assert!(!file.has_passwords());
        assert!(file.encryption.is_none());
        let imported = import_sites(&file, None).expect("import");
        assert_eq!(imported.len(), 1);
    }

    #[test]
    fn read_rejects_files_that_are_not_exports() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("other.json");
        fs::write(&path, r#"{"format":"something","version":1,"sites":[]}"#).expect("write file");

        assert!(matches!(
            ExportFile::read(&path),
            Err(ExportError::NotAnExport)
        ));
    }

    #[test]
    fn write_then_read_round_trips() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("export.json");
        let file = export_sites(&[site("a.example")], &[], None, TEST_COST).expect("export");

        file.write(&path).expect("write");

        assert_eq!(ExportFile::read(&path).expect("read"), file);
    }
}

//! Saved connection sites: the server details shown in the Login dialog.
//!
//! Passwords are deliberately not part of a site and are never written to this file;
//! they live in the desktop keyring (see `secrets`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{self, ConfigError};

/// File transfer protocol of a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// SSH File Transfer Protocol.
    #[default]
    Sftp,
    /// Secure copy over SSH.
    Scp,
    /// Plain File Transfer Protocol.
    Ftp,
}

impl Protocol {
    /// All protocols, in the order the Login dialog lists them.
    pub const ALL: [Self; 3] = [Self::Sftp, Self::Scp, Self::Ftp];

    /// Name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sftp => "SFTP",
            Self::Scp => "SCP",
            Self::Ftp => "FTP",
        }
    }

    /// Port the protocol's servers listen on unless configured otherwise.
    pub fn default_port(self) -> u16 {
        match self {
            Self::Sftp | Self::Scp => 22,
            Self::Ftp => 21,
        }
    }

    /// Position of this protocol in [`Protocol::ALL`].
    pub fn index(self) -> u32 {
        match self {
            Self::Sftp => 0,
            Self::Scp => 1,
            Self::Ftp => 2,
        }
    }

    /// Inverse of [`Protocol::index`].
    ///
    /// # Arguments
    ///
    /// * `index` - Position in [`Protocol::ALL`]
    ///
    /// # Returns
    ///
    /// The protocol, or `None` for an out-of-range index
    pub fn from_index(index: u32) -> Option<Self> {
        usize::try_from(index)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
    }
}

/// Connection details of one saved site. The password itself lives in the desktop
/// keyring (see `secrets`), never in this struct or on disk in plain form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    /// Stable random identifier; keys the site's keyring entry and export records.
    /// Older files without one get an id when loaded.
    #[serde(default)]
    pub id: String,
    /// Protocol used to connect.
    pub protocol: Protocol,
    /// Server host name or IP address.
    pub host: String,
    /// Server port.
    pub port: u16,
    /// Login user name (may be empty).
    pub user: String,
    /// Whether a password for this site is stored in the keyring.
    #[serde(default)]
    pub password_saved: bool,
}

/// Create a new random site id: 32 hex digits (128 bits).
pub fn new_site_id() -> String {
    let mut bytes = [0u8; 16];
    if let Err(error) = getrandom::fill(&mut bytes) {
        // The OS random source practically never fails; fall back to the clock so the
        // app keeps working, at the cost of a guessable (but still unique) id.
        tracing::warn!("random source unavailable ({error}); using a time-based site id");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        bytes = nanos.to_le_bytes();
    }
    bytes
        .iter()
        .fold(String::with_capacity(32), |mut id, byte| {
            // Writing into a `String` cannot fail.
            let _ = write!(id, "{byte:02x}");
            id
        })
}

impl Site {
    /// Name shown in the site list, like `user@host` or `user@host:2222`.
    pub fn display_name(&self) -> String {
        let mut name = if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        };
        if self.port != self.protocol.default_port() {
            name.push_str(&format!(":{}", self.port));
        }
        name
    }
}

/// All saved sites, as stored in `sites.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteBook {
    /// Sites in the order they are listed.
    pub sites: Vec<Site>,
}

impl SiteBook {
    /// Default location: `$XDG_CONFIG_HOME/guiscp/sites.json`.
    pub fn default_path() -> PathBuf {
        config::config_dir().join("sites.json")
    }

    /// Read saved sites from `path`; a missing file gives an empty book.
    ///
    /// # Arguments
    ///
    /// * `path` - JSON file to read
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if the file exists but cannot be read or parsed
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let mut book: Self = config::load_json(path)?;
        for site in book.sites.iter_mut().filter(|site| site.id.is_empty()) {
            site.id = new_site_id();
        }
        Ok(book)
    }

    /// Add imported sites: a site whose id is already present replaces that entry,
    /// any other is appended.
    ///
    /// # Arguments
    ///
    /// * `imported` - Sites read from an export file
    ///
    /// # Returns
    ///
    /// `(added, replaced)` counts
    pub fn merge(&mut self, imported: Vec<Site>) -> (usize, usize) {
        let (mut added, mut replaced) = (0, 0);
        for site in imported {
            match self
                .sites
                .iter_mut()
                .find(|existing| existing.id == site.id)
            {
                Some(existing) => {
                    *existing = site;
                    replaced += 1;
                }
                None => {
                    self.sites.push(site);
                    added += 1;
                }
            }
        }
        (added, replaced)
    }

    /// Write the sites to `path`.
    ///
    /// # Arguments
    ///
    /// * `path` - JSON file to write
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] if the directory or file cannot be written
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        config::save_json(self, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(user: &str, port: u16) -> Site {
        Site {
            id: new_site_id(),
            protocol: Protocol::Sftp,
            host: String::from("example.com"),
            port,
            user: user.to_owned(),
            password_saved: false,
        }
    }

    #[test]
    fn new_site_ids_are_unique_hex() {
        let (first, second) = (new_site_id(), new_site_id());
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn load_gives_ids_to_sites_saved_without_one() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("sites.json");
        std::fs::write(
            &path,
            r#"{"sites":[{"protocol":"sftp","host":"h","port":22,"user":"u"}]}"#,
        )
        .expect("write legacy file");

        let book = SiteBook::load(&path).expect("load legacy file");

        assert_eq!(book.sites[0].id.len(), 32);
        assert!(!book.sites[0].password_saved);
    }

    #[test]
    fn merge_replaces_same_id_and_appends_new() {
        // Arrange
        let mut book = SiteBook {
            sites: vec![site("me", 22)],
        };
        let mut updated = book.sites[0].clone();
        updated.host = String::from("new.example.com");

        // Act
        let counts = book.merge(vec![updated.clone(), site("you", 22)]);

        // Assert
        assert_eq!(counts, (1, 1));
        assert_eq!(book.sites[0], updated);
        assert_eq!(book.sites.len(), 2);
    }

    #[test]
    fn display_name_shows_user_and_non_default_port() {
        assert_eq!(site("me", 22).display_name(), "me@example.com");
        assert_eq!(site("", 22).display_name(), "example.com");
        assert_eq!(site("me", 2222).display_name(), "me@example.com:2222");
    }

    #[test]
    fn protocol_index_round_trips_and_default_ports() {
        for protocol in Protocol::ALL {
            assert_eq!(Protocol::from_index(protocol.index()), Some(protocol));
        }
        assert_eq!(Protocol::from_index(9), None);
        assert_eq!(Protocol::Scp.default_port(), 22);
        assert_eq!(Protocol::Ftp.default_port(), 21);
    }

    #[test]
    fn site_book_round_trips_and_missing_file_is_empty() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("sites.json");
        let book = SiteBook {
            sites: vec![site("me", 22), site("", 2222)],
        };

        // Act
        let missing = SiteBook::load(&path).expect("load missing file");
        book.save(&path).expect("save sites");
        let loaded = SiteBook::load(&path).expect("load sites");

        // Assert
        assert_eq!(missing, SiteBook::default());
        assert_eq!(loaded, book);
        let text = std::fs::read_to_string(&path).expect("read file");
        assert!(
            !text.contains("\"password\""),
            "passwords must never be saved"
        );
    }
}

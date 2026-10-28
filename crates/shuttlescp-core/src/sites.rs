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
    /// FTP settings (used only by FTP sites).
    #[serde(default)]
    pub ftp: FtpOptions,
    /// How to log in with keys (used only by SFTP and SCP sites). Files from before keys
    /// were supported have none: no key file, the agent used.
    #[serde(default)]
    pub ssh: SshAuth,
    /// Files copied at the same time to or from this server; more go faster when the
    /// server limits each connection's speed. FTP opens a connection for each. See
    /// [`Site::files_at_once`].
    #[serde(default = "default_files_at_once")]
    pub files_at_once: u8,
    /// The alias the user gave the site; empty shows [`Site::display_name`] instead (see
    /// [`Site::label`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alias: String,
    /// The user's notes about the site.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Folder the site is listed in, as a path like `Work/Clients`; empty is the top of
    /// the list.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

/// Files copied at the same time for a site that does not say (files from before the
/// setting existed).
pub const DEFAULT_FILES_AT_ONCE: u8 = 4;
/// Most files at once a site may be set to: many servers refuse more connections from
/// one client.
pub const MAX_FILES_AT_ONCE: u8 = 10;

pub(crate) fn default_files_at_once() -> u8 {
    DEFAULT_FILES_AT_ONCE
}

/// Logging in to an SSH server (SFTP and SCP) with keys rather than, or before, a
/// password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SshAuth {
    /// A private key file to log in with (OpenSSH or PuTTY `.ppk`), tried first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_file: Option<PathBuf>,
    /// Also try the keys held by the desktop's SSH agent (`SSH_AUTH_SOCK`), as `ssh`
    /// does.
    pub use_agent: bool,
    /// Whether the key file's passphrase is stored in the keyring.
    pub passphrase_saved: bool,
}

// Written out by hand: the agent is used unless turned off, so a derived default
// (`false`) would be wrong.
impl Default for SshAuth {
    fn default() -> Self {
        Self {
            key_file: None,
            use_agent: true,
            passphrase_saved: false,
        }
    }
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

/// How an FTP connection is encrypted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FtpEncryption {
    /// Switch to TLS (AUTH TLS) when the server offers it, otherwise carry on
    /// unencrypted. Works with every server, encrypted whenever possible.
    #[default]
    ExplicitIfAvailable,
    /// FTPS with explicit TLS, required: connects in plain and switches to TLS before
    /// logging in, refusing servers that cannot. The standard way, on port 21.
    ExplicitTls,
    /// FTPS with implicit TLS: TLS from the first byte, usually on port 990. Older, but
    /// still used.
    ImplicitTls,
    /// Plain FTP: everything, the password included, travels unencrypted.
    None,
}

impl FtpEncryption {
    /// All choices, in the order the Login dialog lists them.
    pub const ALL: [Self; 4] = [
        Self::ExplicitIfAvailable,
        Self::ExplicitTls,
        Self::ImplicitTls,
        Self::None,
    ];

    /// Name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Self::ExplicitIfAvailable => "Use explicit FTP over TLS if available",
            Self::ExplicitTls => "Require explicit FTP over TLS",
            Self::ImplicitTls => "Require implicit FTP over TLS",
            Self::None => "Only use plain FTP (insecure)",
        }
    }

    /// The usual port: 990 for implicit TLS, 21 otherwise.
    pub fn default_port(self) -> u16 {
        match self {
            Self::ImplicitTls => 990,
            Self::ExplicitIfAvailable | Self::ExplicitTls | Self::None => 21,
        }
    }

    /// Position in [`FtpEncryption::ALL`].
    pub fn index(self) -> u32 {
        match self {
            Self::ExplicitIfAvailable => 0,
            Self::ExplicitTls => 1,
            Self::ImplicitTls => 2,
            Self::None => 3,
        }
    }

    /// The choice at a position in [`FtpEncryption::ALL`].
    pub fn from_index(index: u32) -> Option<Self> {
        usize::try_from(index)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
    }
}

/// Settings of FTP sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FtpOptions {
    /// Plain, explicit TLS or implicit TLS.
    pub encryption: FtpEncryption,
    /// Passive mode: the client opens the data connections (works through firewalls
    /// and NAT). Off means active mode: the server connects back to the client.
    pub passive: bool,
}

// Written by hand: passive mode is the usual default, which `derive` would make `false`.
impl Default for FtpOptions {
    fn default() -> Self {
        Self {
            encryption: FtpEncryption::default(),
            passive: true,
        }
    }
}

impl Site {
    /// Files to copy at the same time to or from this server, kept within 1 and
    /// [`MAX_FILES_AT_ONCE`] whatever the settings file says.
    pub fn files_at_once(&self) -> usize {
        usize::from(self.files_at_once.clamp(1, MAX_FILES_AT_ONCE))
    }

    /// The usual port for this site's protocol (and, for FTP, its encryption).
    pub fn default_port(&self) -> u16 {
        match self.protocol {
            Protocol::Ftp => self.ftp.encryption.default_port(),
            Protocol::Sftp | Protocol::Scp => self.protocol.default_port(),
        }
    }

    /// Name shown in the site list, like `user@host` or `user@host:2222`.
    pub fn display_name(&self) -> String {
        address_label(&self.user, &self.host, self.port, self.default_port())
    }

    /// Name the site is shown by: its alias, else
    /// [`Site::display_name`].
    pub fn label(&self) -> String {
        match self.alias.trim() {
            "" => self.display_name(),
            name => name.to_owned(),
        }
    }
}

/// A server address as the site list shows it: `user@host`, with `:port` when not the
/// usual one.
///
/// # Arguments
///
/// * `user` - Login user name (may be empty)
/// * `host` - Server host name
/// * `port` - Server port
/// * `default_port` - The protocol's usual port
pub fn address_label(user: &str, host: &str, port: u16, default_port: u16) -> String {
    let mut name = if user.is_empty() {
        host.to_owned()
    } else {
        format!("{user}@{host}")
    };
    if port != default_port {
        name.push_str(&format!(":{port}"));
    }
    name
}

/// Tidy a typed folder path: spaces around each part and empty parts are dropped, so
/// `" Work / /Clients/"` becomes `Work/Clients` and a blank one the top level (`""`).
pub fn normalize_folder(path: &str) -> String {
    path.split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// The folder holding `path` (`""` for a top-level folder).
pub fn parent_folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// The last part of a folder path: `Clients` of `Work/Clients`.
pub fn folder_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// `name` inside folder `parent` (`""` is the top level).
pub fn join_folder(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

/// Whether `path` is `folder` or somewhere inside it.
pub fn is_within(path: &str, folder: &str) -> bool {
    path == folder
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Why a folder could not be moved or renamed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FolderError {
    /// The new place is inside the folder itself.
    #[error("a folder cannot go inside itself")]
    IntoItself,
    /// A folder of that name is already there.
    #[error("there is already a folder \"{0}\"")]
    Exists(String),
}

/// All saved sites, as stored in `sites.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteBook {
    /// Sites in the order they are listed.
    pub sites: Vec<Site>,
    /// Folders made by the user, kept even while they hold no sites. Folders that
    /// sites are in exist whether listed here or not.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<String>,
}

impl SiteBook {
    /// Every folder: those made by the user, those sites are in, and the folders above
    /// them, sorted.
    pub fn all_folders(&self) -> Vec<String> {
        let mut all = std::collections::BTreeSet::new();
        for path in self
            .folders
            .iter()
            .chain(self.sites.iter().map(|site| &site.folder))
        {
            let mut path = path.as_str();
            while !path.is_empty() {
                all.insert(path.to_owned());
                path = parent_folder(path);
            }
        }
        all.into_iter().collect()
    }

    /// The folders directly inside `parent` (`""` is the top level), sorted by name
    /// ignoring case.
    pub fn child_folders(&self, parent: &str) -> Vec<String> {
        let mut children: Vec<String> = self
            .all_folders()
            .into_iter()
            .filter(|path| parent_folder(path) == parent)
            .collect();
        children.sort_by_key(|path| folder_name(path).to_lowercase());
        children
    }

    /// Make an empty folder (nothing happens if it exists).
    ///
    /// # Arguments
    ///
    /// * `path` - The folder, like `Work/Clients`
    pub fn add_folder(&mut self, path: &str) {
        let path = normalize_folder(path);
        if !path.is_empty() && !self.all_folders().contains(&path) {
            self.folders.push(path);
        }
    }

    /// Move or rename a folder, with the sites and folders in it.
    ///
    /// # Arguments
    ///
    /// * `from` - The folder now
    /// * `to` - Its new path
    ///
    /// # Errors
    ///
    /// [`FolderError::IntoItself`] when `to` is inside `from`, [`FolderError::Exists`]
    /// when a folder `to` is already there
    pub fn move_folder(&mut self, from: &str, to: &str) -> Result<(), FolderError> {
        if from == to {
            return Ok(());
        }
        if is_within(to, from) {
            return Err(FolderError::IntoItself);
        }
        if self.all_folders().iter().any(|path| path == to) {
            return Err(FolderError::Exists(to.to_owned()));
        }
        self.replace_folder(from, to);
        Ok(())
    }

    /// Delete a folder; the sites and folders in it move up to its parent.
    ///
    /// # Arguments
    ///
    /// * `path` - The folder
    pub fn delete_folder(&mut self, path: &str) {
        self.replace_folder(path, parent_folder(path));
    }

    /// Put `to` in place of `from` at the start of every folder path that is in `from`.
    fn replace_folder(&mut self, from: &str, to: &str) {
        let replace = |path: &mut String| {
            if is_within(path, from) {
                *path = normalize_folder(&format!("{to}{}", &path[from.len()..]));
            }
        };
        for site in &mut self.sites {
            replace(&mut site.folder);
        }
        self.folders.iter_mut().for_each(replace);
        let mut seen = std::collections::HashSet::new();
        self.folders
            .retain(|path| !path.is_empty() && seen.insert(path.clone()));
    }

    /// Default location: `$XDG_CONFIG_HOME/shuttlescp/sites.json`.
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
        for site in &mut book.sites {
            if site.id.is_empty() {
                site.id = new_site_id();
            }
            // Hand-edited files may have untidy folder paths.
            site.folder = normalize_folder(&site.folder);
        }
        book.folders = std::mem::take(&mut book.folders)
            .iter()
            .map(|path| normalize_folder(path))
            .filter(|path| !path.is_empty())
            .collect();
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
            ftp: crate::sites::FtpOptions::default(),
            ssh: SshAuth::default(),
            files_at_once: DEFAULT_FILES_AT_ONCE,
            alias: String::new(),
            description: String::new(),
            folder: String::new(),
        }
    }

    fn in_folder(folder: &str) -> Site {
        Site {
            folder: folder.to_owned(),
            ..site("me", 22)
        }
    }

    #[test]
    fn label_prefers_the_given_name() {
        let mut named = site("me", 22);
        assert_eq!(named.label(), "me@example.com");
        named.alias = String::from("  Web server ");
        assert_eq!(named.label(), "Web server");
    }

    #[test]
    fn folder_paths_are_tidied_and_split() {
        assert_eq!(normalize_folder(" Work / /Clients/"), "Work/Clients");
        assert_eq!(normalize_folder("  "), "");
        assert_eq!(parent_folder("Work/Clients"), "Work");
        assert_eq!(parent_folder("Work"), "");
        assert_eq!(folder_name("Work/Clients"), "Clients");
        assert_eq!(join_folder("", "Work"), "Work");
        assert_eq!(join_folder("Work", "Clients"), "Work/Clients");
        assert!(is_within("Work/Clients", "Work"));
        assert!(is_within("Work", "Work"));
        assert!(!is_within("Workshop", "Work"));
    }

    #[test]
    fn folders_include_empty_ones_and_those_above_sites() {
        let mut book = SiteBook {
            sites: vec![in_folder("Work/Clients/Acme"), in_folder("")],
            folders: Vec::new(),
        };
        book.add_folder("home lab");
        book.add_folder("Work");

        assert_eq!(
            book.all_folders(),
            ["Work", "Work/Clients", "Work/Clients/Acme", "home lab"]
        );
        assert_eq!(book.child_folders(""), ["home lab", "Work"]);
        assert_eq!(book.child_folders("Work"), ["Work/Clients"]);
        // "Work" already exists through a site: not stored twice.
        assert_eq!(book.folders, ["home lab"]);
    }

    #[test]
    fn moving_a_folder_takes_its_sites_and_folders() {
        let mut book = SiteBook {
            sites: vec![in_folder("Work/Clients"), in_folder("Workshop")],
            folders: vec![String::from("Work/Clients/Empty")],
        };

        book.move_folder("Work", "Old/Work").expect("move");

        assert_eq!(book.sites[0].folder, "Old/Work/Clients");
        assert_eq!(book.sites[1].folder, "Workshop", "a lookalike name stays");
        assert_eq!(book.folders, ["Old/Work/Clients/Empty"]);
        assert_eq!(
            book.move_folder("Old", "Old/Work/Old"),
            Err(FolderError::IntoItself)
        );
        assert_eq!(
            book.move_folder("Old/Work", "Workshop"),
            Err(FolderError::Exists(String::from("Workshop")))
        );
    }

    #[test]
    fn deleting_a_folder_moves_its_contents_up() {
        let mut book = SiteBook {
            sites: vec![in_folder("Work/Clients"), in_folder("Work")],
            folders: vec![String::from("Work/Clients/Empty")],
        };

        book.delete_folder("Work/Clients");

        assert_eq!(book.sites[0].folder, "Work");
        assert_eq!(book.folders, ["Work/Empty"]);
        book.delete_folder("Work");
        assert_eq!(book.sites[0].folder, "");
        assert_eq!(book.all_folders(), ["Empty"]);
    }

    #[test]
    fn load_tidies_folder_paths() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("sites.json");
        std::fs::write(
            &path,
            r#"{"sites":[{"protocol":"sftp","host":"h","port":22,"user":"u","folder":"/Work/ "}],
                "folders":[" ", "a//b"]}"#,
        )
        .expect("write file");

        let book = SiteBook::load(&path).expect("load");

        assert_eq!(book.sites[0].folder, "Work");
        assert_eq!(book.folders, ["a/b"]);
    }

    #[test]
    fn ftp_encryption_reads_saved_names_and_lists_the_safe_default_first() {
        let read =
            |name: &str| serde_json::from_str::<FtpEncryption>(&format!("\"{name}\"")).expect(name);
        assert_eq!(
            read("explicit_if_available"),
            FtpEncryption::ExplicitIfAvailable
        );
        assert_eq!(read("explicit_tls"), FtpEncryption::ExplicitTls);
        assert_eq!(read("implicit_tls"), FtpEncryption::ImplicitTls);
        assert_eq!(read("none"), FtpEncryption::None);
        assert_eq!(FtpEncryption::ALL[0], FtpEncryption::default());
        for encryption in FtpEncryption::ALL {
            assert_eq!(
                FtpEncryption::from_index(encryption.index()),
                Some(encryption)
            );
        }
    }

    #[test]
    fn files_at_once_defaults_for_older_sites_and_stays_in_range() {
        // A site saved before the setting existed.
        let old: Site = serde_json::from_str(
            r#"{"protocol":"ftp","host":"example.com","port":21,"user":"me"}"#,
        )
        .expect("read an older site");
        assert_eq!(old.files_at_once, DEFAULT_FILES_AT_ONCE);
        assert_eq!(old.files_at_once(), usize::from(DEFAULT_FILES_AT_ONCE));

        // Out of range in a hand-edited file: kept within 1 and the maximum.
        let mut edited = site("me", 21);
        edited.files_at_once = 0;
        assert_eq!(edited.files_at_once(), 1);
        edited.files_at_once = 99;
        assert_eq!(edited.files_at_once(), usize::from(MAX_FILES_AT_ONCE));
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
            folders: Vec::new(),
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
            folders: Vec::new(),
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

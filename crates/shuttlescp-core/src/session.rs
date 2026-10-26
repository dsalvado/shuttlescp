//! The workflows behind the Login dialog: logging in, keeping a site's saved password in
//! step with the "Save password" choice, and exporting / importing sites with their
//! passwords. Front ends start these and show the results; the logic lives here.

use std::path::PathBuf;

use secrecy::SecretString;

use crate::remote::{self, ConnectError, Credentials, HostKey, Remote};
use crate::secrets;
use crate::site_export::{self, ExportFile};
use crate::sites::Site;

/// What saving a site should do with its password in the keyring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringAction {
    /// Store the password that was typed.
    Store,
    /// Remove the saved password.
    Remove,
    /// Leave the keyring as it is.
    Keep,
}

impl KeyringAction {
    /// Decide what to do from the "Save password" box and the site's current state.
    ///
    /// # Arguments
    ///
    /// * `save_password` - Whether "Save password" is ticked
    /// * `typed` - Whether a password was typed
    /// * `was_saved` - Whether the site already had a saved password
    ///
    /// # Returns
    ///
    /// The action; with the box ticked but nothing typed, an existing password is kept
    pub fn decide(save_password: bool, typed: bool, was_saved: bool) -> Self {
        match (save_password, typed, was_saved) {
            (true, true, _) => Self::Store,
            (false, _, true) => Self::Remove,
            (true, false, _) | (false, _, false) => Self::Keep,
        }
    }
}

/// Label of a site's keyring entry, as shown by keyring tools.
///
/// # Arguments
///
/// * `site` - The site
pub fn keyring_label(site: &Site) -> String {
    format!("ShuttleSCP: {}", site.display_name())
}

/// The keyring entry holding a site's key file passphrase, kept apart from its
/// password.
///
/// # Arguments
///
/// * `site_id` - The site's id
pub fn passphrase_entry(site_id: &str) -> String {
    format!("{site_id}#key-passphrase")
}

/// Name a site's key passphrase gets in the keyring, as keyring tools show it.
///
/// # Arguments
///
/// * `site` - The site
pub fn passphrase_label(site: &Site) -> String {
    format!("ShuttleSCP: key passphrase for {}", site.display_name())
}

/// Log in to `site`: with its key file and the SSH agent, and the typed password and
/// key passphrase, or else the ones saved in the keyring.
///
/// # Arguments
///
/// * `site` - Server details
/// * `typed` - Password typed by the user, if any
/// * `typed_passphrase` - Key file passphrase typed by the user, if any
/// * `approved_key` - A host key the user has just agreed to trust
///
/// # Errors
///
/// Returns a [`ConnectError`] describing the step that failed
pub async fn login(
    site: &Site,
    typed: Option<SecretString>,
    typed_passphrase: Option<SecretString>,
    approved_key: Option<HostKey>,
) -> Result<Remote, ConnectError> {
    let password = match typed {
        Some(password) => Some(password),
        None if site.password_saved => {
            saved_secret(&site.id, &keyring_label(site), "password").await
        }
        None => None,
    };
    let key_passphrase = match typed_passphrase {
        Some(passphrase) => Some(passphrase),
        None if site.ssh.passphrase_saved && site.ssh.key_file.is_some() => {
            saved_secret(
                &passphrase_entry(&site.id),
                &passphrase_label(site),
                "key passphrase",
            )
            .await
        }
        None => None,
    };
    let credentials = Credentials {
        password,
        key_passphrase,
    };
    remote::connect(site, credentials, approved_key).await
}

/// A secret saved in the keyring, or `None` (logged) when it cannot be read.
///
/// # Arguments
///
/// * `entry` - The keyring entry's id
/// * `label` - Its name in the keyring
/// * `what` - What it is, for the log
async fn saved_secret(entry: &str, label: &str, what: &str) -> Option<SecretString> {
    secrets::load_password(entry, label)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!("cannot read the saved {what}: {error}");
            None
        })
}

/// Result of a finished export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// File written.
    pub path: PathBuf,
    /// Number of sites written.
    pub sites: usize,
    /// How many of them include their password.
    pub with_passwords: usize,
    /// Saved passwords that could not be read from the keyring (left out).
    pub unreadable: usize,
}

/// Export `sites` to `path`. With a passphrase, their saved passwords are read from the
/// keyring and included, encrypted (see `site_export`); without one they are left out.
///
/// # Arguments
///
/// * `sites` - Sites to export
/// * `path` - File to write
/// * `passphrase` - Passphrase protecting the passwords, or `None` to leave them out
///
/// # Errors
///
/// Returns a readable message if the export cannot be built or written
pub async fn export_to_file(
    sites: Vec<Site>,
    path: PathBuf,
    passphrase: Option<SecretString>,
) -> Result<ExportReport, String> {
    let mut passwords = Vec::new();
    let mut unreadable = 0;
    if passphrase.is_some() {
        for site in sites.iter().filter(|site| site.password_saved) {
            match secrets::load_password(&site.id, &keyring_label(site)).await {
                Ok(Some(password)) => passwords.push((site.id.clone(), password)),
                Ok(None) | Err(_) => unreadable += 1,
            }
        }
    }
    let with_passwords = passwords.len();
    // Key derivation is deliberately slow, so it runs on a worker thread.
    let written = tokio::task::spawn_blocking(move || {
        site_export::export_sites(
            &sites,
            &passwords,
            passphrase.as_ref(),
            site_export::DEFAULT_COST,
        )
        .and_then(|file| file.write(&path).map(|()| (file.sites.len(), path)))
    })
    .await;
    match written {
        Ok(Ok((sites, path))) => Ok(ExportReport {
            path,
            sites,
            with_passwords,
            unreadable,
        }),
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

/// Result of a finished import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// The imported sites; `password_saved` is set where the keyring accepted the
    /// password. Merge them into the site book with `SiteBook::merge`.
    pub sites: Vec<Site>,
    /// Passwords that could not be stored in the keyring.
    pub failed_passwords: usize,
}

/// Decrypt an export's passwords and store them in the keyring.
///
/// # Arguments
///
/// * `file` - The export, from `ExportFile::read`
/// * `passphrase` - Passphrase the export was made with (needed if it has passwords)
///
/// # Errors
///
/// Returns a readable message for a wrong passphrase or a damaged file
pub async fn import_from_file(
    file: ExportFile,
    passphrase: Option<SecretString>,
) -> Result<ImportReport, String> {
    // Key derivation is deliberately slow, so it runs on a worker thread.
    let entries =
        tokio::task::spawn_blocking(move || site_export::import_sites(&file, passphrase.as_ref()))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
    let mut sites = Vec::with_capacity(entries.len());
    let mut failed_passwords = 0;
    for (mut site, password) in entries {
        if let Some(password) = password {
            match secrets::store_password(&site.id, &keyring_label(&site), &password).await {
                Ok(()) => site.password_saved = true,
                Err(error) => {
                    tracing::warn!("could not store an imported password: {error}");
                    failed_passwords += 1;
                }
            }
        }
        sites.push(site);
    }
    Ok(ImportReport {
        sites,
        failed_passwords,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_action_follows_the_save_password_box() {
        // Ticked: store what was typed, otherwise keep what is there.
        assert_eq!(
            KeyringAction::decide(true, true, false),
            KeyringAction::Store
        );
        assert_eq!(
            KeyringAction::decide(true, true, true),
            KeyringAction::Store
        );
        assert_eq!(
            KeyringAction::decide(true, false, true),
            KeyringAction::Keep
        );
        // Cleared: remove a saved password; nothing to do if there is none.
        assert_eq!(
            KeyringAction::decide(false, true, true),
            KeyringAction::Remove
        );
        assert_eq!(
            KeyringAction::decide(false, false, false),
            KeyringAction::Keep
        );
    }

    #[test]
    fn keyring_label_names_the_site() {
        let site = Site {
            id: String::from("x"),
            protocol: crate::sites::Protocol::Sftp,
            host: String::from("host"),
            port: 22,
            user: String::from("me"),
            password_saved: false,
            ftp: crate::sites::FtpOptions::default(),
            ssh: crate::sites::SshAuth::default(),
            files_at_once: crate::sites::DEFAULT_FILES_AT_ONCE,
        };
        assert_eq!(keyring_label(&site), "ShuttleSCP: me@host");
    }
}

//! Site passwords in the system's password store.
//!
//! On Linux this is the desktop keyring (the Secret Service, e.g. GNOME Keyring), which
//! encrypts its contents with the user's login password, so passwords are never stored
//! by this app in a file of its own. Each entry is tagged with the app name and the site
//! id, which also makes it identifiable in tools such as Seahorse.
//!
//! Other platforms do not have a backend yet (Windows Credential Manager and the macOS
//! Keychain are the natural ones): there every call reports [`KeyringError::Unsupported`]
//! and the app simply works without saved passwords.

use secrecy::SecretString;
use thiserror::Error;

/// Attribute value identifying this app's entries in the keyring. Fixed (not taken from
/// the crate name) because existing entries are found by it.
const APPLICATION: &str = "shuttlescp";
/// The attribute value used while the app was called GuiSCP. Passwords saved then are
/// found by it and copied under [`APPLICATION`] the first time they are read.
const LEGACY_APPLICATION: &str = "guiscp";

/// Error talking to the password store.
#[derive(Debug, Error)]
pub enum KeyringError {
    /// The password store rejected the request or is not running.
    #[error("keyring error: {0}")]
    Service(String),
    /// A stored secret was not valid UTF-8 text.
    #[error("stored password is not valid text")]
    NotText,
    /// This platform has no password store backend yet.
    #[error("saving passwords is not supported on this platform yet")]
    Unsupported,
}

/// Keyring lookup attributes for a site's password.
///
/// # Arguments
///
/// * `site_id` - The site's id
fn attributes(site_id: &str) -> [(&'static str, &str); 2] {
    [("application", APPLICATION), ("site-id", site_id)]
}

/// Keyring lookup attributes a site's password had under the app's earlier name.
fn legacy_attributes(site_id: &str) -> [(&'static str, &str); 2] {
    [("application", LEGACY_APPLICATION), ("site-id", site_id)]
}

/// Store (or replace) the password of a site.
///
/// # Arguments
///
/// * `site_id` - The site's id
/// * `label` - Human readable name shown by keyring tools, e.g. "ShuttleSCP: me@host"
/// * `password` - The password
///
/// # Errors
///
/// Returns [`KeyringError`] if the keyring cannot be reached or written
pub async fn store_password(
    site_id: &str,
    label: &str,
    password: &SecretString,
) -> Result<(), KeyringError> {
    backend::store(&attributes(site_id), label, password).await
}

/// Fetch the password of a site. A password saved while the app was called GuiSCP is
/// found too, and copied under the current name (the old entry stays, for older
/// versions of the app).
///
/// # Arguments
///
/// * `site_id` - The site's id
/// * `label` - Name to show for the entry if it is copied, e.g. "ShuttleSCP: me@host"
///
/// # Returns
///
/// The password, or `None` if none is stored
///
/// # Errors
///
/// Returns [`KeyringError`] if the keyring cannot be reached or the entry read
pub async fn load_password(
    site_id: &str,
    label: &str,
) -> Result<Option<SecretString>, KeyringError> {
    if let Some(password) = backend::load(&attributes(site_id)).await? {
        return Ok(Some(password));
    }
    let Some(password) = backend::load(&legacy_attributes(site_id)).await? else {
        return Ok(None);
    };
    match backend::store(&attributes(site_id), label, &password).await {
        Ok(()) => tracing::info!("copied a saved password from GuiSCP: {label}"),
        // The password still works this time; copying is retried next time.
        Err(error) => tracing::warn!("cannot copy a saved password from GuiSCP: {error}"),
    }
    Ok(Some(password))
}

/// Remove the password of a site (nothing happens if none is stored).
///
/// # Arguments
///
/// * `site_id` - The site's id
///
/// # Errors
///
/// Returns [`KeyringError`] if the keyring cannot be reached or changed
pub async fn delete_password(site_id: &str) -> Result<(), KeyringError> {
    backend::delete(&attributes(site_id)).await?;
    // Also any copy left from when the app was called GuiSCP.
    backend::delete(&legacy_attributes(site_id)).await
}

/// Linux: the Secret Service through `oo7`.
#[cfg(target_os = "linux")]
mod backend {
    use secrecy::{ExposeSecret, SecretString};

    use super::KeyringError;

    // Lets `?` convert library errors.
    impl From<oo7::Error> for KeyringError {
        fn from(error: oo7::Error) -> Self {
            Self::Service(error.to_string())
        }
    }

    /// Connect to the keyring, asking the desktop to unlock it if needed.
    async fn open() -> Result<oo7::Keyring, KeyringError> {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        Ok(keyring)
    }

    pub(super) async fn store(
        attributes: &[(&str, &str); 2],
        label: &str,
        password: &SecretString,
    ) -> Result<(), KeyringError> {
        open()
            .await?
            .create_item(label, attributes, password.expose_secret(), true)
            .await?;
        Ok(())
    }

    pub(super) async fn load(
        attributes: &[(&str, &str); 2],
    ) -> Result<Option<SecretString>, KeyringError> {
        let keyring = open().await?;
        let items = keyring.search_items(attributes).await?;
        let Some(item) = items.first() else {
            return Ok(None);
        };
        item.unlock().await?;
        let secret = item.secret().await?;
        let text = std::str::from_utf8(secret.as_bytes()).map_err(|_| KeyringError::NotText)?;
        Ok(Some(SecretString::from(text)))
    }

    pub(super) async fn delete(attributes: &[(&str, &str); 2]) -> Result<(), KeyringError> {
        open().await?.delete(attributes).await?;
        Ok(())
    }
}

/// Other platforms: no password store yet.
#[cfg(not(target_os = "linux"))]
mod backend {
    use secrecy::SecretString;

    use super::KeyringError;

    pub(super) async fn store(
        _attributes: &[(&str, &str); 2],
        _label: &str,
        _password: &SecretString,
    ) -> Result<(), KeyringError> {
        Err(KeyringError::Unsupported)
    }

    pub(super) async fn load(
        _attributes: &[(&str, &str); 2],
    ) -> Result<Option<SecretString>, KeyringError> {
        Err(KeyringError::Unsupported)
    }

    pub(super) async fn delete(_attributes: &[(&str, &str); 2]) -> Result<(), KeyringError> {
        Err(KeyringError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_tag_the_app_and_site() {
        assert_eq!(
            attributes("abc"),
            [("application", "shuttlescp"), ("site-id", "abc")]
        );
    }
}

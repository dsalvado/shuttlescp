//! Site passwords in the desktop keyring (the Secret Service, e.g. GNOME Keyring).
//!
//! The keyring encrypts its contents with the user's login password, so passwords are
//! never stored by this app in a file of its own. Each entry is tagged with the app
//! name and the site id, which also makes it identifiable in tools such as Seahorse.

use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;

/// Attribute value identifying this app's entries in the keyring.
const APPLICATION: &str = env!("CARGO_PKG_NAME");

/// Error talking to the keyring.
#[derive(Debug, Error)]
pub enum KeyringError {
    /// The Secret Service rejected the request or is not running. Boxed because the
    /// library's error is large, which would bloat every `Result` carrying it.
    #[error("keyring error: {0}")]
    Service(Box<oo7::Error>),
    /// A stored secret was not valid UTF-8 text.
    #[error("stored password is not valid text")]
    NotText,
}

// Lets `?` convert library errors, boxing them on the way.
impl From<oo7::Error> for KeyringError {
    fn from(error: oo7::Error) -> Self {
        Self::Service(Box::new(error))
    }
}

/// Keyring lookup attributes for a site's password.
///
/// # Arguments
///
/// * `site_id` - The site's id
fn attributes(site_id: &str) -> [(&'static str, &str); 2] {
    [("application", APPLICATION), ("site-id", site_id)]
}

/// Connect to the keyring, asking the desktop to unlock it if needed.
async fn open() -> Result<oo7::Keyring, KeyringError> {
    let keyring = oo7::Keyring::new().await?;
    keyring.unlock().await?;
    Ok(keyring)
}

/// Store (or replace) the password of a site.
///
/// # Arguments
///
/// * `site_id` - The site's id
/// * `label` - Human readable name shown by keyring tools, e.g. "GuiSCP: me@host"
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
    open()
        .await?
        .create_item(label, &attributes(site_id), password.expose_secret(), true)
        .await?;
    Ok(())
}

/// Fetch the password of a site.
///
/// # Arguments
///
/// * `site_id` - The site's id
///
/// # Returns
///
/// The password, or `None` if none is stored
///
/// # Errors
///
/// Returns [`KeyringError`] if the keyring cannot be reached or the entry read
pub async fn load_password(site_id: &str) -> Result<Option<SecretString>, KeyringError> {
    let keyring = open().await?;
    let items = keyring.search_items(&attributes(site_id)).await?;
    let Some(item) = items.first() else {
        return Ok(None);
    };
    item.unlock().await?;
    let secret = item.secret().await?;
    let text = std::str::from_utf8(secret.as_bytes()).map_err(|_| KeyringError::NotText)?;
    Ok(Some(SecretString::from(text)))
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
    open().await?.delete(&attributes(site_id)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_tag_the_app_and_site() {
        assert_eq!(
            attributes("abc"),
            [("application", "guiscp"), ("site-id", "abc")]
        );
    }
}

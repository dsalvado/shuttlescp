//! Site passwords in the system's password store.
//!
//! On Linux this is the desktop keyring (the Secret Service, e.g. GNOME Keyring), which
//! encrypts its contents with the user's login password, so passwords are never stored
//! by this app in a file of its own. Each entry is tagged with the app name and the site
//! id, which also makes it identifiable in tools such as Seahorse.
//!
//! On macOS it is the user's login Keychain, which the system unlocks at login, and on
//! Windows the Credential Manager, which Windows protects with the user's login.
//!
//! Other platforms do not have a backend: there every call reports
//! [`KeyringError::Unsupported`] and the app simply works without saved passwords.

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

/// macOS: the login Keychain through `security-framework`.
///
/// A Keychain generic password is keyed by a service and an account: the service is the
/// `application` attribute and the account the `site-id`, so entries show in Keychain
/// Access as "shuttlescp" with the site id as account. The Keychain calls block (and may
/// ask the user to allow access), so they run on tokio's blocking threads.
#[cfg(target_os = "macos")]
mod backend {
    use secrecy::{ExposeSecret, SecretString};
    use security_framework::passwords::{
        PasswordOptions, delete_generic_password, generic_password, set_generic_password_options,
    };

    use super::KeyringError;

    /// `errSecItemNotFound`: no entry matches the service and account.
    const ITEM_NOT_FOUND: i32 = -25300;

    // Lets `?` convert library errors.
    impl From<security_framework::base::Error> for KeyringError {
        fn from(error: security_framework::base::Error) -> Self {
            Self::Service(error.to_string())
        }
    }

    /// Run a blocking Keychain call off the async worker threads.
    async fn blocking<T: Send + 'static>(
        call: impl FnOnce() -> Result<T, KeyringError> + Send + 'static,
    ) -> Result<T, KeyringError> {
        tokio::task::spawn_blocking(call)
            .await
            .map_err(|error| KeyringError::Service(error.to_string()))?
    }

    /// The Keychain service and account for keyring attributes.
    fn service_account(attributes: &[(&str, &str); 2]) -> (String, String) {
        (attributes[0].1.to_owned(), attributes[1].1.to_owned())
    }

    pub(super) async fn store(
        attributes: &[(&str, &str); 2],
        label: &str,
        password: &SecretString,
    ) -> Result<(), KeyringError> {
        let (service, account) = service_account(attributes);
        let label = label.to_owned();
        let password = password.clone();
        blocking(move || {
            let mut options = PasswordOptions::new_generic_password(&service, &account);
            options.set_label(&label);
            set_generic_password_options(password.expose_secret().as_bytes(), options)?;
            Ok(())
        })
        .await
    }

    pub(super) async fn load(
        attributes: &[(&str, &str); 2],
    ) -> Result<Option<SecretString>, KeyringError> {
        let (service, account) = service_account(attributes);
        blocking(move || {
            match generic_password(PasswordOptions::new_generic_password(&service, &account)) {
                Ok(secret) => {
                    let text = String::from_utf8(secret).map_err(|_| KeyringError::NotText)?;
                    Ok(Some(SecretString::from(text)))
                }
                Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
        .await
    }

    pub(super) async fn delete(attributes: &[(&str, &str); 2]) -> Result<(), KeyringError> {
        let (service, account) = service_account(attributes);
        blocking(move || match delete_generic_password(&service, &account) {
            Err(error) if error.code() != ITEM_NOT_FOUND => Err(error.into()),
            _ => Ok(()),
        })
        .await
    }
}

/// Windows: Credential Manager through `keyring-core` and its Windows store, which keep
/// the (unsafe) Windows calls on their side.
///
/// Each password is a generic credential named `<application>:<site-id>` (e.g.
/// "shuttlescp:3f2a..."), with the label as its comment, as Credential Manager shows
/// under Windows Credentials. Credentials are kept on this computer only ("Local"),
/// never roamed with a domain profile. The calls block, so they run on tokio's
/// blocking threads.
#[cfg(windows)]
mod backend {
    use std::collections::HashMap;

    use keyring_core::api::CredentialStoreApi;
    use secrecy::{ExposeSecret, SecretString};

    use super::KeyringError;

    // Lets `?` convert library errors.
    impl From<keyring_core::Error> for KeyringError {
        fn from(error: keyring_core::Error) -> Self {
            match error {
                keyring_core::Error::BadEncoding(_) => Self::NotText,
                error => Self::Service(error.to_string()),
            }
        }
    }

    /// Run a blocking Credential Manager call off the async worker threads.
    async fn blocking<T: Send + 'static>(
        call: impl FnOnce() -> Result<T, KeyringError> + Send + 'static,
    ) -> Result<T, KeyringError> {
        tokio::task::spawn_blocking(call)
            .await
            .map_err(|error| KeyringError::Service(error.to_string()))?
    }

    /// The credential for keyring attributes (it need not exist yet).
    fn entry(attributes: &[(&str, &str); 2]) -> Result<keyring_core::Entry, KeyringError> {
        let target = format!("{}:{}", attributes[0].1, attributes[1].1);
        let modifiers = HashMap::from([("target", target.as_str()), ("persistence", "Local")]);
        let store = windows_native_keyring_store::Store::new()?;
        Ok(store.build(attributes[0].1, attributes[1].1, Some(&modifiers))?)
    }

    pub(super) async fn store(
        attributes: &[(&str, &str); 2],
        label: &str,
        password: &SecretString,
    ) -> Result<(), KeyringError> {
        let entry = entry(attributes)?;
        let label = label.to_owned();
        let password = password.clone();
        blocking(move || {
            entry.set_password(password.expose_secret())?;
            entry.update_attributes(&HashMap::from([("comment", label.as_str())]))?;
            Ok(())
        })
        .await
    }

    pub(super) async fn load(
        attributes: &[(&str, &str); 2],
    ) -> Result<Option<SecretString>, KeyringError> {
        let entry = entry(attributes)?;
        blocking(move || match entry.get_password() {
            Ok(password) => Ok(Some(SecretString::from(password))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        })
        .await
    }

    pub(super) async fn delete(attributes: &[(&str, &str); 2]) -> Result<(), KeyringError> {
        let entry = entry(attributes)?;
        blocking(move || match entry.delete_credential() {
            Err(keyring_core::Error::NoEntry) | Ok(()) => Ok(()),
            Err(error) => Err(error.into()),
        })
        .await
    }
}

/// Other platforms: no password store yet.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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

    /// Writes to the real login Keychain (macOS) or Credential Manager (Windows), so only
    /// run on request: `cargo test -p shuttlescp-core keychain_round_trip -- --ignored`
    #[cfg(any(target_os = "macos", windows))]
    #[tokio::test]
    #[ignore = "uses the user's real password store"]
    async fn keychain_round_trip() {
        use secrecy::ExposeSecret;

        let site = "shuttlescp-test-keychain-round-trip";
        let label = "ShuttleSCP: keychain test";
        delete_password(site).await.unwrap();
        assert!(load_password(site, label).await.unwrap().is_none());

        store_password(site, label, &SecretString::from("first"))
            .await
            .unwrap();
        store_password(site, label, &SecretString::from("second"))
            .await
            .unwrap();
        let loaded = load_password(site, label).await.unwrap().unwrap();
        assert_eq!(loaded.expose_secret(), "second");

        delete_password(site).await.unwrap();
        assert!(load_password(site, label).await.unwrap().is_none());
    }
}

//! Finding a newer release and installing it safely.
//!
//! Every GitHub release carries a small manifest, `latest.json`, naming its version and
//! the AppImage with its size and SHA-256, and `latest.json.sig`, an Ed25519 signature of
//! the manifest's exact bytes. The release workflow makes the signature with the release
//! signing key, whose private half exists only as a GitHub secret (and the maintainer's
//! backup); the public half is built into the app ([`RELEASE_PUBLIC_KEY`]).
//!
//! An update is only installed when:
//! 1. the manifest's signature checks out against that public key, so a manifest made by
//!    anyone else (a tampered download, a compromised account or mirror) is refused;
//! 2. its version is newer than the running one, so an old release cannot be replayed;
//! 3. the downloaded AppImage has exactly the size and SHA-256 the signed manifest gives.
//!
//! It then replaces the running AppImage with one rename in the same folder, so the file
//! is always either the old version or the new one, never half-written. The running app
//! keeps using the old file until it restarts.
//!
//! Downloads use HTTPS (rustls); the checks above do not rely on it.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Public half of the release signing key: a raw Ed25519 key, base64. Empty in builds
/// made before a key existed, which can then only report that updates are unavailable.
pub const RELEASE_PUBLIC_KEY: &str = "6ZeIUFLRLUUKMrp+p+/PkbY44Yn7jAJwNzGbpkDZGaw=";

/// Where the latest release's manifest is downloaded from.
pub const MANIFEST_URL: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/releases/latest/download/latest.json"
);
/// Where the latest release's manifest signature is downloaded from.
pub const SIGNATURE_URL: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/releases/latest/download/latest.json.sig"
);
/// Release downloads must come from here (the signature already vouches for the
/// address; this is a second line of defence).
const DOWNLOAD_PREFIX: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/download/");

/// Largest manifest or signature accepted, in bytes.
const SMALL_FILE_LIMIT: u64 = 64 * 1024;
/// Give up on a request that makes no progress for this long.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Bytes read per step of a download.
const CHUNK: usize = 256 * 1024;

/// What a release offers, as described by its signed manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Release version, e.g. "1.0.0".
    pub version: String,
    /// When it was published (ISO 8601).
    pub published: String,
    /// The release page, with its notes.
    pub notes_url: String,
    /// The Linux AppImage.
    pub appimage: Asset,
    /// The macOS disk image, from 0.10 on. Manifests before it have none, and apps
    /// before it ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<Asset>,
    /// The Windows installer, from 0.11 on. Earlier manifests have none, and apps before
    /// it ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Asset>,
}

/// A file of a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// File name, e.g. "ShuttleSCP-1.0.0-x86_64.AppImage".
    pub name: String,
    /// Where to download it.
    pub url: String,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256 of its contents, lower-case hex.
    pub sha256: String,
}

/// Look for a release newer than `current`.
///
/// Blocking: call it from a worker thread.
///
/// # Returns
///
/// The newer release's manifest, or `None` when `current` is the latest
///
/// # Errors
///
/// Returns a message if the release cannot be fetched, or its manifest is not
/// correctly signed or cannot be read
pub fn check(current: &str) -> Result<Option<Manifest>, String> {
    let key = release_key()?;
    let agent = agent();
    let manifest = fetch_small(&agent, MANIFEST_URL)?;
    let signature = fetch_small(&agent, SIGNATURE_URL)?;
    let manifest = verify_manifest(&manifest, &signature, &key)?;
    tracing::info!("latest release is {}", manifest.version);
    Ok(is_newer(&manifest.version, current)?.then_some(manifest))
}

/// Check a manifest's signature and read it.
///
/// # Arguments
///
/// * `manifest` - The manifest file's bytes
/// * `signature` - The raw 64-byte Ed25519 signature of those bytes
/// * `public_key` - The raw 32-byte Ed25519 public key
///
/// # Errors
///
/// Returns a message if the signature does not match or the manifest is malformed
pub fn verify_manifest(
    manifest: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> Result<Manifest, String> {
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(manifest, signature)
        .map_err(|_| String::from("the update is not signed by the ShuttleSCP release key"))?;
    let manifest: Manifest = serde_json::from_slice(manifest)
        .map_err(|error| format!("the update description cannot be read: {error}"))?;
    for asset in std::iter::once(&manifest.appimage)
        .chain(&manifest.macos)
        .chain(&manifest.windows)
    {
        if !asset.url.starts_with(DOWNLOAD_PREFIX) {
            return Err(format!(
                "the update would be downloaded from an unexpected place: {}",
                asset.url
            ));
        }
    }
    Ok(manifest)
}

/// Whether version `candidate` is newer than `current` (both "major.minor.patch",
/// optionally with a leading "v").
///
/// # Errors
///
/// Returns a message if either is not such a version
pub fn is_newer(candidate: &str, current: &str) -> Result<bool, String> {
    Ok(parse_version(candidate)? > parse_version(current)?)
}

/// Download a release's AppImage into `dir`, checking its size and SHA-256 against the
/// signed manifest, and make it executable.
///
/// Blocking: call it from a worker thread.
///
/// # Arguments
///
/// * `asset` - The AppImage, from a verified manifest
/// * `dir` - Folder to download into (the one holding the running AppImage, so the
///   finished file can replace it with a rename)
/// * `progress` - Called with (bytes so far, total) as the download proceeds
/// * `cancel` - Set it to stop the download
///
/// # Returns
///
/// The downloaded file, ready for [`install`]
///
/// # Errors
///
/// Returns a message if the download fails, is cancelled, or does not match the
/// manifest; the partial file is removed
pub fn download(
    asset: &Asset,
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let target = dir.join(format!(".{}.download", asset.name));
    let result = (|| {
        let mut response = agent()
            .get(&asset.url)
            .call()
            .map_err(|error| format!("cannot download {}: {error}", asset.name))?;
        let mut file = File::create(&target)
            .map_err(|error| format!("cannot create {}: {error}", target.display()))?;
        copy_verified(
            &mut response.body_mut().as_reader(),
            &mut file,
            asset,
            &mut progress,
            cancel,
        )?;
        file.sync_all()
            .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
        make_executable(&target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&target);
    }
    result.map(|()| target)
}

/// Replace `target` (the running AppImage) with the downloaded file, in one rename.
///
/// # Errors
///
/// Returns a message if the file cannot be replaced (e.g. its folder is read-only)
pub fn install(downloaded: &Path, target: &Path) -> Result<(), String> {
    std::fs::rename(downloaded, target).map_err(|error| {
        let _ = std::fs::remove_file(downloaded);
        format!("cannot replace {}: {error}", target.display())
    })?;
    tracing::info!("installed the update as {}", target.display());
    Ok(())
}

/// The AppImage file this program runs from, if it was started from one.
pub fn running_appimage() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

/// Copy a download to `out`, checking it against `asset` as it goes.
///
/// # Errors
///
/// Returns a message if reading or writing fails, the download is cancelled, or its
/// size or SHA-256 differs from the manifest's
fn copy_verified(
    input: &mut impl Read,
    out: &mut impl Write,
    asset: &Asset,
    progress: &mut impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; CHUNK];
    let mut done: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(String::from("the download was cancelled"));
        }
        let read = input
            .read(&mut buffer)
            .map_err(|error| format!("the download failed: {error}"))?;
        if read == 0 {
            break;
        }
        done += read as u64;
        if done > asset.size {
            return Err(String::from("the download is larger than the release says"));
        }
        hasher.update(&buffer[..read]);
        out.write_all(&buffer[..read])
            .map_err(|error| format!("cannot save the download: {error}"))?;
        progress(done, asset.size);
    }
    if done != asset.size {
        return Err(format!(
            "the download is incomplete ({done} of {} bytes)",
            asset.size
        ));
    }
    let digest = hex(&hasher.finalize());
    if !digest.eq_ignore_ascii_case(&asset.sha256) {
        return Err(String::from(
            "the download does not match the signed release (SHA-256 differs)",
        ));
    }
    Ok(())
}

/// The built-in release key, decoded.
fn release_key() -> Result<Vec<u8>, String> {
    use base64::Engine;
    if RELEASE_PUBLIC_KEY.is_empty() {
        return Err(String::from("this build cannot check for updates"));
    }
    base64::engine::general_purpose::STANDARD
        .decode(RELEASE_PUBLIC_KEY)
        .map_err(|error| format!("the built-in release key is damaged: {error}"))
}

/// An HTTPS client with time limits.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(TIMEOUT))
        .timeout_recv_response(Some(TIMEOUT))
        .timeout_recv_body(Some(Duration::from_secs(60 * 60)))
        .user_agent(concat!("ShuttleSCP/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Download a small file (a manifest or signature) completely.
fn fetch_small(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    agent
        .get(url)
        .call()
        .map_err(|error| format!("cannot reach the releases page: {error}"))?
        .body_mut()
        .with_config()
        .limit(SMALL_FILE_LIMIT)
        .read_to_vec()
        .map_err(|error| format!("cannot read the release description: {error}"))
}

/// "1.2.3" or "v1.2.3" as numbers.
fn parse_version(version: &str) -> Result<(u64, u64, u64), String> {
    let invalid = || format!("\"{version}\" is not a version number");
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let mut next = || -> Result<u64, String> {
        parts
            .next()
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())
    };
    let parsed = (next()?, next()?, next()?);
    if parts.next().is_some() {
        return Err(invalid());
    }
    Ok(parsed)
}

/// Make a downloaded AppImage runnable (rwxr-xr-x).
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot make {} runnable: {error}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Lower-case hex of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    use super::*;

    /// A throwaway signing key, as the release workflow's would be.
    fn key_pair() -> Ed25519KeyPair {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("generate");
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("parse")
    }

    fn manifest_json(url: &str) -> Vec<u8> {
        serde_json::to_vec(&Manifest {
            version: String::from("1.2.0"),
            published: String::from("2026-11-01T10:00:00Z"),
            notes_url: format!("{}/releases/tag/v1.2.0", env!("CARGO_PKG_REPOSITORY")),
            appimage: Asset {
                name: String::from("ShuttleSCP-1.2.0-x86_64.AppImage"),
                url: url.to_owned(),
                size: 11,
                sha256: hex(&Sha256::digest(b"hello world")),
            },
            macos: None,
            windows: None,
        })
        .expect("serialize")
    }

    fn release_url() -> String {
        format!("{DOWNLOAD_PREFIX}v1.2.0/ShuttleSCP-1.2.0-x86_64.AppImage")
    }

    #[test]
    fn a_correctly_signed_manifest_is_accepted() {
        let key = key_pair();
        let manifest = manifest_json(&release_url());
        let signature = key.sign(&manifest);

        let read = verify_manifest(&manifest, signature.as_ref(), key.public_key().as_ref())
            .expect("valid");

        assert_eq!(read.version, "1.2.0");
    }

    /// A manifest signed by packaging/make-release-manifest.sh (OpenSSL), as the
    /// release workflow signs them, with a throwaway key.
    #[test]
    fn a_manifest_signed_by_the_release_script_is_accepted() {
        use base64::Engine;
        let manifest = include_bytes!("../test-data/update/latest.json");
        let signature = include_bytes!("../test-data/update/latest.json.sig");
        let key = base64::engine::general_purpose::STANDARD
            .decode(include_str!("../test-data/update/test-public-key.txt").trim())
            .expect("base64");

        let read = verify_manifest(manifest, signature, &key).expect("valid");

        assert_eq!(read.version, "0.9.0");
        assert_eq!(read.appimage.name, "ShuttleSCP-0.9.0-x86_64.AppImage");
        let mut changed = manifest.to_vec();
        changed[20] ^= 1;
        assert!(verify_manifest(&changed, signature, &key).is_err());
    }

    #[test]
    fn a_changed_manifest_or_another_key_is_refused() {
        let key = key_pair();
        let manifest = manifest_json(&release_url());
        let signature = key.sign(&manifest);

        let mut tampered = manifest.clone();
        let last = tampered.len() - 2;
        tampered[last] ^= 1;
        assert!(verify_manifest(&tampered, signature.as_ref(), key.public_key().as_ref()).is_err());

        let other = key_pair();
        assert!(
            verify_manifest(&manifest, signature.as_ref(), other.public_key().as_ref()).is_err()
        );
    }

    #[test]
    fn a_signed_manifest_pointing_elsewhere_is_refused() {
        let key = key_pair();
        let manifest = manifest_json("https://example.com/ShuttleSCP.AppImage");
        let signature = key.sign(&manifest);

        let result = verify_manifest(&manifest, signature.as_ref(), key.public_key().as_ref());

        assert!(result.is_err_and(|error| error.contains("unexpected place")));
    }

    #[test]
    fn only_newer_versions_count() {
        assert_eq!(is_newer("1.0.0", "0.9.0"), Ok(true));
        assert_eq!(is_newer("v0.10.0", "0.9.5"), Ok(true));
        assert_eq!(is_newer("0.9.0", "0.9.0"), Ok(false));
        assert_eq!(is_newer("0.8.9", "0.9.0"), Ok(false));
        assert!(is_newer("1.0", "0.9.0").is_err());
        assert!(is_newer("1.0.0-beta", "0.9.0").is_err());
    }

    fn asset(size: u64, content_hash: &[u8]) -> Asset {
        Asset {
            name: String::from("ShuttleSCP.AppImage"),
            url: release_url(),
            size,
            sha256: hex(&Sha256::digest(content_hash)),
        }
    }

    #[test]
    fn a_download_matching_the_manifest_is_kept() {
        let mut out = Vec::new();
        let mut seen = 0;

        let result = copy_verified(
            &mut &b"hello world"[..],
            &mut out,
            &asset(11, b"hello world"),
            &mut |done, _| seen = done,
            &AtomicBool::new(false),
        );

        assert_eq!(result, Ok(()));
        assert_eq!(out, b"hello world");
        assert_eq!(seen, 11);
    }

    #[test]
    fn a_download_that_differs_from_the_manifest_is_refused() {
        let never = AtomicBool::new(false);
        let mut sink = Vec::new();
        let copy = |content: &[u8], expected: &Asset, sink: &mut Vec<u8>| {
            copy_verified(&mut &content[..], sink, expected, &mut |_, _| {}, &never)
        };

        assert!(copy(b"hello world", &asset(11, b"hello there"), &mut sink).is_err());
        assert!(copy(b"hello", &asset(11, b"hello world"), &mut sink).is_err());
        assert!(copy(b"hello world!!", &asset(11, b"hello world"), &mut sink).is_err());
    }

    #[test]
    fn a_cancelled_download_stops() {
        let result = copy_verified(
            &mut &b"hello world"[..],
            &mut Vec::new(),
            &asset(11, b"hello world"),
            &mut |_, _| {},
            &AtomicBool::new(true),
        );

        assert_eq!(result, Err(String::from("the download was cancelled")));
    }

    #[test]
    fn install_replaces_the_file_in_one_step() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (old, new) = (temp.path().join("app"), temp.path().join(".app.download"));
        std::fs::write(&old, b"old").expect("write");
        std::fs::write(&new, b"new").expect("write");

        install(&new, &old).expect("install");

        assert_eq!(std::fs::read(&old).expect("read"), b"new");
        assert!(!new.exists());
    }

    #[test]
    fn a_manifest_without_a_mac_download_still_reads() {
        // Arrange: the manifest of 0.9.x, before the macOS download existed.
        let key = key_pair();
        let manifest = manifest_json(&release_url());

        // Act
        let read = verify_manifest(
            &manifest,
            key.sign(&manifest).as_ref(),
            key.public_key().as_ref(),
        )
        .expect("accepted");

        // Assert
        assert_eq!(read.macos, None);
        assert_eq!(read.windows, None);
    }

    #[test]
    fn a_windows_download_from_elsewhere_is_refused() {
        // Arrange
        let key = key_pair();
        let mut value: serde_json::Value =
            serde_json::from_slice(&manifest_json(&release_url())).expect("json");
        value["windows"] = serde_json::json!({
            "name": "ShuttleSCP-1.2.0-windows-x64-setup.exe",
            "url": "https://example.com/setup.exe",
            "size": 1,
            "sha256": "00",
        });
        let manifest = serde_json::to_vec(&value).expect("serialize");

        // Act
        let result = verify_manifest(
            &manifest,
            key.sign(&manifest).as_ref(),
            key.public_key().as_ref(),
        );

        // Assert
        assert!(result.is_err_and(|error| error.contains("unexpected place")));
    }

    #[test]
    fn a_mac_download_from_elsewhere_is_refused() {
        // Arrange
        let key = key_pair();
        let mut value: serde_json::Value =
            serde_json::from_slice(&manifest_json(&release_url())).expect("json");
        value["macos"] = serde_json::json!({
            "name": "ShuttleSCP-1.2.0-macos-arm64.dmg",
            "url": "https://example.com/ShuttleSCP.dmg",
            "size": 1,
            "sha256": "00",
        });
        let manifest = serde_json::to_vec(&value).expect("serialize");

        // Act
        let result = verify_manifest(
            &manifest,
            key.sign(&manifest).as_ref(),
            key.public_key().as_ref(),
        );

        // Assert
        assert!(result.is_err_and(|error| error.contains("unexpected place")));
    }
}

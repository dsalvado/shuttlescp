//! Single quick operations on one side: creating a folder and renaming.

use std::path::{Path, PathBuf};

use super::endpoint::Endpoint;
use crate::remote::Remote;

/// Check that `name` can be a file or folder name.
///
/// # Errors
///
/// Returns a message saying what is wrong with it
pub fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err(String::from("enter a name"))
    } else if name == "." || name == ".." {
        Err(format!("\"{name}\" cannot be used as a name"))
    } else if name.contains('/') {
        Err(String::from("a name cannot contain \"/\""))
    } else if name.contains('\0') {
        Err(String::from("a name cannot contain a null character"))
    } else {
        Ok(())
    }
}

/// Create a new folder called `name` in `parent`.
///
/// # Arguments
///
/// * `remote` - The server, or `None` for this computer
/// * `parent` - Folder to create it in
/// * `name` - Its name
///
/// # Returns
///
/// The new folder's path
///
/// # Errors
///
/// Returns a message if the name is invalid, already taken, or the folder cannot be
/// created
pub async fn create_folder(
    remote: Option<&Remote>,
    parent: &Path,
    name: &str,
) -> Result<PathBuf, String> {
    check_name(name)?;
    let endpoint = Endpoint::of(remote);
    let path = parent.join(name);
    if endpoint.exists(&path).await {
        return Err(format!("{name} already exists"));
    }
    endpoint.create_dir(&path).await?;
    tracing::info!("created folder {}", path.display());
    Ok(path)
}

/// Rename the file or folder at `path` to `new_name`, in the same folder.
///
/// # Arguments
///
/// * `remote` - The server, or `None` for this computer
/// * `path` - What to rename
/// * `new_name` - Its new name
///
/// # Returns
///
/// The new path
///
/// # Errors
///
/// Returns a message if the name is invalid, already taken, or renaming fails. Nothing
/// is ever replaced.
pub async fn rename(
    remote: Option<&Remote>,
    path: &Path,
    new_name: &str,
) -> Result<PathBuf, String> {
    check_name(new_name)?;
    let endpoint = Endpoint::of(remote);
    let target = path.with_file_name(new_name);
    if target == path {
        return Ok(target);
    }
    if endpoint.exists(&target).await {
        return Err(format!("{new_name} already exists"));
    }
    endpoint.rename(path, &target).await?;
    tracing::info!("renamed {} to {}", path.display(), target.display());
    Ok(target)
}

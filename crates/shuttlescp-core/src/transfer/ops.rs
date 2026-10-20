//! Single quick operations on one side: creating a folder and renaming; and the check
//! that a copy within one side makes sense.

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

/// Check that copying or moving `sources` into `dest_dir` on their own side makes
/// sense: they are not already there, and no folder goes inside itself.
///
/// # Arguments
///
/// * `sources` - Files and folders to copy or move
/// * `dest_dir` - Folder on the same side to put them in
///
/// # Errors
///
/// Returns a message saying why the copy cannot be done
pub fn check_same_side(sources: &[PathBuf], dest_dir: &Path) -> Result<(), String> {
    for source in sources {
        let name = source
            .file_name()
            .map_or_else(|| source.to_string_lossy(), |name| name.to_string_lossy());
        if dest_dir.starts_with(source) {
            return Err(format!("{name} cannot go inside itself"));
        }
        if source.parent() == Some(dest_dir) {
            return Err(format!("{name} is already in {}", dest_dir.display()));
        }
    }
    Ok(())
}

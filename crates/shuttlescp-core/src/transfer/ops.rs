//! Single quick operations on one side: creating a folder and renaming; the checks that
//! a name is usable and that a copy within one side makes sense; and names that Windows
//! can use.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};

use super::Side;
use super::endpoint::Endpoint;
use crate::path::SidePath;
use crate::remote::Remote;

/// Characters Windows does not allow in a file or folder name.
const WINDOWS_FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
/// Names Windows keeps for devices, whatever extension follows ("CON.txt" too).
const WINDOWS_RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
/// Device names Windows keeps with a digit 1 to 9 after them ("COM1", "LPT9").
const WINDOWS_RESERVED_NUMBERED: [&str; 2] = ["COM", "LPT"];

/// Check that `name` can be a file or folder name on `side`. On Windows, local names
/// also follow Windows' rules (see [`windows_name_problem`]).
///
/// # Errors
///
/// Returns a message saying what is wrong with it
pub fn check_name(name: &str, side: Side) -> Result<(), String> {
    if name.trim().is_empty() {
        Err(String::from("enter a name"))
    } else if name == "." || name == ".." {
        Err(format!("\"{name}\" cannot be used as a name"))
    } else if name.contains('/') {
        Err(String::from("a name cannot contain \"/\""))
    } else if name.contains('\0') {
        Err(String::from("a name cannot contain a null character"))
    } else if cfg!(windows)
        && side == Side::Local
        && let Some(problem) = windows_name_problem(name)
    {
        Err(problem)
    } else {
        Ok(())
    }
}

/// What makes `name` unusable on Windows, if anything: the characters
/// `\ / : * ? " < > |`, control characters, a space or dot at the end, or a device
/// name such as CON or COM1.
pub fn windows_name_problem(name: &str) -> Option<String> {
    if let Some(forbidden) = name.chars().find(|c| WINDOWS_FORBIDDEN.contains(c)) {
        return Some(format!("a name cannot contain \"{forbidden}\""));
    }
    if name.chars().any(char::is_control) {
        return Some(String::from("a name cannot contain control characters"));
    }
    if name.ends_with([' ', '.']) {
        return Some(String::from("a name cannot end with a space or a dot"));
    }
    // Windows reads "con.txt" and "CON .txt" as the device CON too.
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let upper = stem.to_ascii_uppercase();
    let numbered = WINDOWS_RESERVED_NUMBERED.iter().any(|device| {
        upper
            .strip_prefix(device)
            .is_some_and(|digit| matches!(digit.as_bytes(), [b'1'..=b'9']))
    });
    (WINDOWS_RESERVED.contains(&upper.as_str()) || numbered)
        .then(|| format!("\"{stem}\" is a name Windows keeps for a device"))
}

/// A name Windows can use, for a file or folder called `name` elsewhere (e.g. on a
/// Linux server). As WinSCP does, each character Windows forbids becomes `%` and its
/// hex code ("notes:v2.txt" becomes "notes%3Av2.txt"); a space or dot at the end, and
/// the last letter of a device name ("con.txt" becomes "co%6E.txt"), are written the
/// same way. A name Windows accepts comes back unchanged.
pub fn windows_safe_name(name: &str) -> Cow<'_, str> {
    if windows_name_problem(name).is_none() {
        return Cow::Borrowed(name);
    }
    let escape = |c: char| format!("%{:02X}", u32::from(c));
    let mut safe: String = name
        .chars()
        .map(|c| {
            if WINDOWS_FORBIDDEN.contains(&c) || c.is_control() {
                escape(c)
            } else {
                c.to_string()
            }
        })
        .collect();
    if let Some(last) = safe.chars().last().filter(|c| matches!(c, ' ' | '.')) {
        safe.pop();
        safe.push_str(&escape(last));
    }
    if windows_name_problem(&safe).is_some() {
        // Only a device name is left: its stem ends at the first dot, spaces aside.
        let stem_end = safe.find('.').unwrap_or(safe.len());
        let stem = safe[..stem_end].trim_end();
        if let Some(last) = stem.chars().last() {
            let cut = stem.len();
            safe.replace_range(cut - last.len_utf8()..cut, &escape(last));
        }
    }
    Cow::Owned(safe)
}

/// The name [`windows_safe_name`] was given, for a name it wrote: "notes%3Av2.txt"
/// becomes "notes:v2.txt" again. As WinSCP does, uploads from Windows use it, so a file
/// downloaded and sent back keeps its name on the server. A name `windows_safe_name`
/// would not have written (e.g. "100%41.txt", or "report.pdf") comes back unchanged.
pub fn windows_restored_name(name: &str) -> Cow<'_, str> {
    if !name.contains('%') {
        return Cow::Borrowed(name);
    }
    // A code as `windows_safe_name` writes it: two hex digits, upper case.
    let decode = |hex: &str| {
        hex.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
            .then(|| u8::from_str_radix(hex, 16).ok())
            .flatten()
            .map(char::from)
    };
    // Each code for a character Windows forbids, and a space or dot at the end.
    let mut restored = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(at) = rest.find('%') {
        restored.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        match after.get(..2).and_then(decode) {
            Some(c)
                if WINDOWS_FORBIDDEN.contains(&c)
                    || c.is_control()
                    || (after.len() == 2 && matches!(c, ' ' | '.')) =>
            {
                restored.push(c);
                rest = &after[2..];
            }
            _ => {
                restored.push('%');
                rest = after;
            }
        }
    }
    restored.push_str(rest);
    // The last letter of a device name's stem ("co%6E.txt" for "con.txt").
    let stem_end = restored.find('.').unwrap_or(restored.len());
    let stem_len = restored[..stem_end].trim_end().len();
    if let Some(code) = stem_len.checked_sub(3)
        && restored.is_char_boundary(code)
        && restored[code..].starts_with('%')
        && let Some(c) = decode(&restored[code + 1..stem_len])
    {
        let device = format!("{}{c}{}", &restored[..code], &restored[stem_len..]);
        if windows_safe_name(&device) == name {
            return Cow::Owned(device);
        }
    }
    // Only a name that turns back into `name` is the one it was made from.
    if restored != name && windows_safe_name(&restored) == name {
        Cow::Owned(restored)
    } else {
        Cow::Borrowed(name)
    }
}

/// The name a copy of `name` from `source_side` gets in `dest_dir`. On Windows, a copy
/// to a folder on this computer gets a name Windows can use (see [`windows_safe_name`]),
/// and an upload gets back a name made that way (see [`windows_restored_name`]). Any
/// other copy keeps its name.
pub(super) fn dest_name<'a>(
    name: &'a OsStr,
    source_side: Side,
    dest_dir: &SidePath,
) -> Cow<'a, OsStr> {
    if !cfg!(windows) {
        return Cow::Borrowed(name);
    }
    let Some(text) = name.to_str() else {
        return Cow::Borrowed(name);
    };
    let renamed = match (source_side, dest_dir.side()) {
        (_, Side::Local) => windows_safe_name(text),
        (Side::Local, Side::Remote) => windows_restored_name(text),
        (Side::Remote, Side::Remote) => Cow::Borrowed(text),
    };
    match renamed {
        Cow::Owned(renamed) => Cow::Owned(OsString::from(renamed)),
        Cow::Borrowed(_) => Cow::Borrowed(name),
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
    parent: &SidePath,
    name: &str,
) -> Result<SidePath, String> {
    check_name(name, parent.side())?;
    let endpoint = Endpoint::of(remote);
    let path = parent.join(name);
    if endpoint.exists(&path).await {
        return Err(format!("{name} already exists"));
    }
    endpoint.create_dir(&path).await?;
    tracing::info!("created folder {}", path);
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
    path: &SidePath,
    new_name: &str,
) -> Result<SidePath, String> {
    check_name(new_name, path.side())?;
    let endpoint = Endpoint::of(remote);
    let target = path.with_file_name(new_name);
    if &target == path {
        return Ok(target);
    }
    if endpoint.exists(&target).await {
        return Err(format!("{new_name} already exists"));
    }
    endpoint.rename(path, &target).await?;
    tracing::info!("renamed {} to {}", path, target);
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
pub fn check_same_side(sources: &[SidePath], dest_dir: &SidePath) -> Result<(), String> {
    for source in sources {
        let name = source.file_name().map_or_else(
            || source.to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        if dest_dir.starts_with(source) {
            return Err(format!("{name} cannot go inside itself"));
        }
        if source.parent().as_ref() == Some(dest_dir) {
            return Err(format!("{name} is already in {}", dest_dir));
        }
    }
    Ok(())
}

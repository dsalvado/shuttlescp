//! Changing the permissions and owner of files and folders, as the Permissions dialog
//! asks: for one item or several at once, here or on a server, and through sudo when
//! the user's own rights are not enough.
//!
//! Several items can start out different. The dialog then only says which bits to turn
//! on and which to turn off; a bit it leaves alone keeps each item's own value. That is
//! a [`PermissionEdit`], applied to each item with [`apply`].

use crate::path::SidePath;
use crate::remote::{Change, CommandError, Remote, RunAs};

/// What to change, the same for every item chosen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionEdit {
    /// Permission bits to turn on.
    pub set: u32,
    /// Permission bits to turn off.
    pub clear: u32,
    /// New owner (name or number); `None` keeps each item's own.
    pub owner: Option<String>,
    /// New group (name or number); `None` keeps each item's own.
    pub group: Option<String>,
}

impl PermissionEdit {
    /// An item's permissions after this edit.
    ///
    /// # Arguments
    ///
    /// * `old` - Its permissions now
    pub fn new_mode(&self, old: u32) -> u32 {
        ((old & !self.clear) | self.set) & 0o7777
    }

    /// An item's permissions after this edit, if they can be known: from its
    /// permissions now, or when unknown, from an edit that decides every rwx bit.
    ///
    /// # Arguments
    ///
    /// * `old` - Its permissions now, if known
    pub fn mode_after(&self, old: Option<u32>) -> Option<u32> {
        match old {
            Some(old) => Some(self.new_mode(old)),
            None if self.covers_all_rwx() => Some(self.new_mode(0)),
            None => None,
        }
    }

    /// Whether this edit says what every rwx bit becomes, so it can be applied even
    /// where the permissions now are unknown.
    fn covers_all_rwx(&self) -> bool {
        (self.set | self.clear) & 0o777 == 0o777
    }

    /// Whether it changes owner or group.
    fn changes_owner(&self) -> bool {
        self.owner.is_some() || self.group.is_some()
    }
}

/// Which items inside the chosen folders a permissions job changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// Files and folders.
    #[default]
    All,
    /// Files only.
    Files,
    /// Folders only.
    Folders,
}

/// A permissions job ([`crate::transfer::JobKind::Permissions`]): the edit, applied to
/// the chosen items and everything inside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionsJob {
    /// What to change.
    pub edit: PermissionEdit,
    /// Which items to change.
    pub scope: Scope,
    /// Turn execute on only for folders, and for files that already have it for
    /// someone (chmod's `X`): opening folders, without making every file runnable.
    pub execute_only_on_folders: bool,
    /// Through sudo, with this password (or none); `None` as the user.
    pub sudo: Option<SudoPassword>,
}

impl PermissionsJob {
    /// The edit for one item of the job, or `None` if the scope leaves it out.
    ///
    /// # Arguments
    ///
    /// * `is_dir` - It is a folder
    /// * `mode` - Its permissions now, if known
    pub fn edit_for(&self, is_dir: bool, mode: Option<u32>) -> Option<PermissionEdit> {
        let wanted = match self.scope {
            Scope::All => true,
            Scope::Files => !is_dir,
            Scope::Folders => is_dir,
        };
        if !wanted {
            return None;
        }
        let mut edit = self.edit.clone();
        let runnable = mode.is_some_and(|mode| mode & 0o111 != 0);
        if self.execute_only_on_folders && !is_dir && !runnable {
            edit.set &= !0o111;
        }
        Some(edit)
    }

    /// Who makes the changes.
    pub fn run_as(&self) -> RunAs {
        self.sudo.as_ref().map_or(RunAs::User, SudoPassword::run_as)
    }
}

/// The sudo password a job uses, if it needs one. Kept only in memory: jobs with it are
/// never saved, and it never takes part in comparing or printing jobs.
#[derive(Clone, Default)]
pub struct SudoPassword(Option<std::sync::Arc<secrecy::SecretString>>);

impl SudoPassword {
    /// Sudo with this password, or without one (`None`: the account needs none).
    pub fn new(password: Option<secrecy::SecretString>) -> Self {
        Self(password.map(std::sync::Arc::new))
    }

    /// Running through sudo with this password.
    pub fn run_as(&self) -> RunAs {
        RunAs::Sudo {
            password: self.0.as_deref().cloned(),
        }
    }
}

// Two jobs are the same job whatever the password: a secret is not compared.
impl PartialEq for SudoPassword {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for SudoPassword {}

// Written by hand, so a password can never be printed (in logs, for one).
impl std::fmt::Debug for SudoPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SudoPassword(..)")
    }
}

/// Why an item was not changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyError {
    /// What to show: "cannot change the permissions of /x: Operation not permitted".
    pub message: String,
    /// Refused as the user, on a server where sudo could be tried.
    pub sudo_may_help: bool,
    /// What sudo answered, when it was tried (to ask for a password, or again).
    pub sudo: Option<CommandError>,
}

/// Apply `edit` to one item.
///
/// As the user ([`RunAs::User`]): permissions with the protocol's own request (it works
/// on every SFTP and most FTP accounts), owners by number the same way, owners by name
/// with `chown` (needs a shell). Through sudo: everything with commands, as root.
///
/// # Arguments
///
/// * `remote` - The server, or `None` for this computer
/// * `path` - The item
/// * `mode` - Its permissions now, if known
/// * `edit` - What to change
/// * `run_as` - Who makes the change
///
/// # Errors
///
/// Returns what went wrong, and whether sudo might do better
pub async fn apply(
    remote: Option<&Remote>,
    path: &SidePath,
    mode: Option<u32>,
    edit: &PermissionEdit,
    run_as: &RunAs,
) -> Result<(), ApplyError> {
    let new_mode = match mode {
        Some(old) => Some(edit.new_mode(old)).filter(|&new| new != old & 0o7777),
        None if edit.covers_all_rwx() => Some(edit.new_mode(0)),
        None if edit.set | edit.clear == 0 => None,
        // Some bits chosen, the rest unknown: guessing them could take rights away.
        None => {
            return Err(plain(crate::i18n::cannot(
                "set-permissions",
                path,
                crate::tr!("error-mode-unknown"),
            )));
        }
    };
    match (remote, path) {
        (Some(remote), SidePath::Remote(remote_path)) => {
            on_server(remote, remote_path, new_mode, edit, run_as).await
        }
        (None, SidePath::Local(local_path)) => here(local_path, new_mode, edit),
        _ => Err(plain(crate::i18n::cannot(
            "set-permissions",
            path,
            "wrong side",
        ))),
    }
}

/// [`apply`] on a server.
async fn on_server(
    remote: &Remote,
    path: &crate::path::RemotePath,
    new_mode: Option<u32>,
    edit: &PermissionEdit,
    run_as: &RunAs,
) -> Result<(), ApplyError> {
    // Sudo can only help over SSH (FTP runs no commands).
    let sudo_may_help = remote.can_set_owner() && !run_as.is_sudo();
    let refused = |message: String| ApplyError {
        message,
        sudo_may_help,
        sudo: None,
    };
    let by_command = |action: &'static str, error: CommandError| ApplyError {
        message: crate::i18n::cannot(action, path, &error),
        sudo_may_help: sudo_may_help && matches!(error, CommandError::Failed(_)),
        sudo: run_as.is_sudo().then_some(error),
    };
    if let Some(mode) = new_mode {
        if run_as.is_sudo() {
            remote
                .change_by_command(path, &Change::Mode(mode), run_as)
                .await
                .map_err(|error| by_command("set-permissions", error))?;
        } else {
            remote.set_mode(path, mode).await.map_err(refused)?;
        }
    }
    if edit.changes_owner() {
        let numbers = (number(edit.owner.as_deref()), number(edit.group.as_deref()));
        match numbers {
            // Numbers need no shell: SFTP's own request takes them.
            (Ok(uid), Ok(gid)) if !run_as.is_sudo() => {
                remote.set_owner(path, uid, gid).await.map_err(refused)?;
            }
            _ => {
                let change = Change::Owner {
                    owner: edit.owner.clone(),
                    group: edit.group.clone(),
                };
                remote
                    .change_by_command(path, &change, run_as)
                    .await
                    .map_err(|error| by_command("set-owner", error))?;
            }
        }
    }
    Ok(())
}

/// [`apply`] on this computer (Linux, macOS). No sudo here: a user who needs root for
/// their own files can run the app as root.
#[cfg(unix)]
fn here(
    path: &std::path::Path,
    new_mode: Option<u32>,
    edit: &PermissionEdit,
) -> Result<(), ApplyError> {
    if let Some(mode) = new_mode {
        crate::local_fs::set_mode(path, mode).map_err(plain)?;
    }
    if edit.changes_owner() {
        let unknown = |name: &str| {
            plain(crate::i18n::cannot(
                "set-owner",
                path.display(),
                crate::tr!("error-no-such-user", name = name.to_owned()),
            ))
        };
        let uid = match edit.owner.as_deref() {
            Some(name) => Some(crate::local_fs::user_id(name).ok_or_else(|| unknown(name))?),
            None => None,
        };
        let gid = match edit.group.as_deref() {
            Some(name) => Some(crate::local_fs::group_id(name).ok_or_else(|| unknown(name))?),
            None => None,
        };
        crate::local_fs::set_owner(path, uid, gid).map_err(plain)?;
    }
    Ok(())
}

/// Windows has no Unix permissions; the menu does not offer this there.
#[cfg(not(unix))]
fn here(
    path: &std::path::Path,
    _new_mode: Option<u32>,
    _edit: &PermissionEdit,
) -> Result<(), ApplyError> {
    Err(plain(crate::i18n::cannot(
        "set-permissions",
        path.display(),
        "not on Windows",
    )))
}

/// A failure sudo cannot help with.
fn plain(message: String) -> ApplyError {
    ApplyError {
        message,
        sudo_may_help: false,
        sudo: None,
    }
}

/// `Ok(Some(number))` for a numeric owner or group, `Ok(None)` for none, `Err` for a
/// name.
fn number(text: Option<&str>) -> Result<Option<u32>, ()> {
    match text {
        None => Ok(None),
        Some(text) => text.parse().map(Some).map_err(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(set: u32, clear: u32) -> PermissionEdit {
        PermissionEdit {
            set,
            clear,
            ..PermissionEdit::default()
        }
    }

    #[test]
    fn bits_left_alone_keep_each_items_value() {
        // "Group may write" on two files that differ in their other bits.
        let group_write = edit(0o020, 0);
        assert_eq!(group_write.new_mode(0o644), 0o664);
        assert_eq!(group_write.new_mode(0o700), 0o720);
        // "Others may not read", keeping the setgid bit.
        assert_eq!(edit(0, 0o004).new_mode(0o2755), 0o2751);
        // The file-type bits never come through.
        assert_eq!(edit(0, 0).new_mode(0o100_644), 0o644);
    }

    #[test]
    fn jobs_pick_their_items_and_keep_files_from_becoming_runnable() {
        let job = |scope, execute_only_on_folders| PermissionsJob {
            edit: edit(0o755, 0o022),
            scope,
            execute_only_on_folders,
            sudo: None,
        };
        assert!(
            job(Scope::Files, false)
                .edit_for(true, Some(0o755))
                .is_none()
        );
        assert!(
            job(Scope::Folders, false)
                .edit_for(false, Some(0o644))
                .is_none()
        );
        // Execute only on folders: a plain file gets rw, a folder or a script gets x.
        let x = job(Scope::All, true);
        assert_eq!(
            x.edit_for(false, Some(0o600)).unwrap().new_mode(0o600),
            0o644
        );
        assert_eq!(
            x.edit_for(true, Some(0o700)).unwrap().new_mode(0o700),
            0o755
        );
        assert_eq!(
            x.edit_for(false, Some(0o700)).unwrap().new_mode(0o700),
            0o755
        );
        // Without it, everything gets x.
        let plain = job(Scope::All, false);
        assert_eq!(
            plain.edit_for(false, Some(0o600)).unwrap().new_mode(0o600),
            0o755
        );
    }

    #[test]
    fn the_sudo_password_is_never_printed() {
        let password = SudoPassword::new(Some(secrecy::SecretString::from("hunter2")));
        assert!(!format!("{password:?}").contains("hunter2"));
    }

    #[test]
    fn unknown_permissions_need_every_bit_chosen() {
        assert!(edit(0o644, 0o133).covers_all_rwx());
        assert!(!edit(0o020, 0).covers_all_rwx());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn changes_local_files_and_refuses_to_guess() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("temp dir");
        let file = temp.path().join("a.txt");
        std::fs::write(&file, b"").expect("write");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let path = SidePath::Local(file.clone());

        apply(None, &path, Some(0o644), &edit(0o020, 0o004), &RunAs::User)
            .await
            .expect("apply");
        let mode = std::fs::metadata(&file)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o660);

        let unknown = apply(None, &path, None, &edit(0o001, 0), &RunAs::User).await;
        assert!(unknown.is_err());
    }
}

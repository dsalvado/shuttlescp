//! Changing permissions and owners with shell commands over SSH (`chmod`, `chown`,
//! `chgrp`), as the logged-in user or as root through `sudo`.
//!
//! The protocols' own requests (SFTP setstat, FTP `SITE CHMOD`) always act as the
//! logged-in user, and SFTP only takes owner numbers. A command takes names, and can run
//! through sudo when the user's own rights are not enough. It needs an account that may
//! run commands (not SFTP-only ones).
//!
//! This file holds the parts that need no server: the command text, and reading what
//! sudo answered. The SSH side is `Sftp::run_command`.

use std::fmt;

use secrecy::SecretString;

/// What to change on a file or folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// New permission bits (only the lowest 12 are used).
    Mode(u32),
    /// New owner and/or group, by name (or number); `None` keeps that one.
    Owner {
        owner: Option<String>,
        group: Option<String>,
    },
}

/// Who runs the command.
#[derive(Debug)]
pub enum RunAs {
    /// The logged-in user.
    User,
    /// Root, through sudo. Without a password, sudo is asked not to prompt (`-n`): that
    /// works where the account needs none, and otherwise says one is needed.
    Sudo { password: Option<SecretString> },
}

impl RunAs {
    /// Whether this goes through sudo.
    pub fn is_sudo(&self) -> bool {
        matches!(self, Self::Sudo { .. })
    }
}

/// Why a command did not make its change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The account may not run commands (SFTP-only accounts, FTP sites).
    NoShell,
    /// Sudo wants a password and none was given.
    NeedsPassword,
    /// Sudo refused the password given.
    WrongPassword,
    /// The account is not allowed to use sudo.
    NotAllowed,
    /// The server has no sudo.
    NoSudo,
    /// The server's sudo only runs from a terminal (`requiretty`).
    NeedsTerminal,
    /// No answer in time.
    NoAnswer,
    /// The command itself failed; its first line of error output.
    Failed(String),
}

// Written by hand: the message is in the interface language.
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::NoShell => crate::tr!("command-no-shell"),
            Self::NeedsPassword => crate::tr!("command-sudo-password"),
            Self::WrongPassword => crate::tr!("command-sudo-wrong-password"),
            Self::NotAllowed => crate::tr!("command-sudo-not-allowed"),
            Self::NoSudo => crate::tr!("command-no-sudo"),
            Self::NeedsTerminal => crate::tr!("command-sudo-terminal"),
            Self::NoAnswer => crate::tr!("command-no-answer"),
            Self::Failed(reason) => reason.clone(),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for CommandError {}

/// Printed after the change succeeds. Success is judged by this, not by the exit
/// status alone: an SFTP-only account may "run" any command by starting its SFTP server,
/// which ends with status 0 having done nothing.
pub(crate) const DONE_MARKER: &str = "SHUTTLESCP-DONE";

/// The shell command that makes `change` to `path`, or `None` when there is nothing
/// to change.
///
/// `LC_ALL=C` keeps sudo's and chmod's messages in English, so [`outcome`] can read
/// them. Every name and path is quoted, so no file name can run anything. With a
/// password, sudo reads it from the command's input (`-S`), never from the command
/// line, which other users of the server could see in `ps`; `-p ''` stops it printing
/// a prompt.
pub(crate) fn command_line(change: &Change, path: &str, run_as: &RunAs) -> Option<String> {
    let path = shell_quote(path);
    let action = match change {
        Change::Mode(mode) => format!("chmod {:04o} -- {path}", mode & 0o7777),
        Change::Owner { owner, group } => match (owner, group) {
            (Some(owner), Some(group)) => {
                format!(
                    "chown {} -- {path}",
                    shell_quote(&format!("{owner}:{group}"))
                )
            }
            (Some(owner), None) => format!("chown {} -- {path}", shell_quote(owner)),
            (None, Some(group)) => format!("chgrp {} -- {path}", shell_quote(group)),
            (None, None) => return None,
        },
    };
    let sudo = match run_as {
        RunAs::User => "",
        RunAs::Sudo { password: None } => "sudo -n -- ",
        RunAs::Sudo { password: Some(_) } => "sudo -S -p '' -- ",
    };
    Some(format!("LC_ALL=C {sudo}{action} && echo {DONE_MARKER}"))
}

/// The command that only checks that sudo works (with that password, or without one),
/// changing nothing: run before a long job, so it does not fail item after item.
pub(crate) fn sudo_check_line(run_as: &RunAs) -> String {
    let sudo = match run_as {
        RunAs::Sudo { password: Some(_) } => "sudo -S -p '' -- ",
        RunAs::User | RunAs::Sudo { password: None } => "sudo -n -- ",
    };
    format!("LC_ALL=C {sudo}true && echo {DONE_MARKER}")
}

/// What a command's output says happened.
///
/// # Arguments
///
/// * `output` - What it printed
/// * `errors` - What it printed as errors
/// * `status` - Its exit status, if the server sent one
/// * `sudo` - Whether it went through sudo
pub(crate) fn outcome(
    output: &str,
    errors: &str,
    status: Option<u32>,
    sudo: bool,
) -> Result<(), CommandError> {
    if output.contains(DONE_MARKER) {
        return Ok(());
    }
    let lower = errors.to_ascii_lowercase();
    if sudo {
        // Checked from the most specific to the least.
        if lower.contains("password is required")
            || lower.contains("terminal is required to read the password")
        {
            return Err(CommandError::NeedsPassword);
        }
        if lower.contains("incorrect password")
            || lower.contains("try again")
            || lower.contains("no password was provided")
        {
            return Err(CommandError::WrongPassword);
        }
        if lower.contains("not in the sudoers")
            || lower.contains("is not allowed to")
            || lower.contains("may not run sudo")
        {
            return Err(CommandError::NotAllowed);
        }
        if lower.contains("must have a tty") {
            return Err(CommandError::NeedsTerminal);
        }
        if status == Some(127) && lower.contains("sudo") {
            return Err(CommandError::NoSudo);
        }
    }
    match errors.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some(line) => Err(CommandError::Failed(line.to_owned())),
        // Nothing said and not done: the account ran no command at all.
        None => Err(CommandError::NoShell),
    }
}

/// `text` as one word for the server's shell: in single quotes, inside which nothing
/// (`$`, `;`, backquotes, spaces) has a meaning, each `'` written as `'\''` (end the
/// quotes, an escaped quote, start them again). Works in sh, bash, zsh and fish.
pub(crate) fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sudo(password: Option<&str>) -> RunAs {
        RunAs::Sudo {
            password: password.map(SecretString::from),
        }
    }

    #[test]
    fn commands_quote_paths_and_names() {
        let line = command_line(&Change::Mode(0o100_755), "/srv/it's here", &RunAs::User);
        assert_eq!(
            line.as_deref(),
            Some(r"LC_ALL=C chmod 0755 -- '/srv/it'\''s here' && echo SHUTTLESCP-DONE")
        );
        let owner = |owner: Option<&str>, group: Option<&str>| Change::Owner {
            owner: owner.map(str::to_owned),
            group: group.map(str::to_owned),
        };
        let both = command_line(&owner(Some("alex"), Some("staff")), "/f", &RunAs::User);
        assert!(both.unwrap().contains("chown 'alex:staff' -- '/f'"));
        let group = command_line(&owner(None, Some("www-data")), "/f", &RunAs::User);
        assert!(group.unwrap().contains("chgrp 'www-data' -- '/f'"));
        // A name can't break out of its quotes either.
        let sneaky = command_line(&owner(Some("x'; rm -rf ~; '"), None), "/f", &RunAs::User);
        assert!(
            sneaky
                .unwrap()
                .contains(r"chown 'x'\''; rm -rf ~; '\''' -- '/f'")
        );
        assert_eq!(command_line(&owner(None, None), "/f", &RunAs::User), None);
    }

    #[test]
    fn sudo_reads_the_password_from_input_never_the_command_line() {
        let asked = command_line(&Change::Mode(0o644), "/f", &sudo(None)).unwrap();
        assert!(asked.starts_with("LC_ALL=C sudo -n -- chmod"), "{asked}");
        let given = command_line(&Change::Mode(0o644), "/f", &sudo(Some("hunter2"))).unwrap();
        assert!(
            given.starts_with("LC_ALL=C sudo -S -p '' -- chmod"),
            "{given}"
        );
        assert!(!given.contains("hunter2"));
    }

    #[test]
    fn reads_what_sudo_answered() {
        let read = |errors: &str, status| outcome("", errors, Some(status), true);
        assert_eq!(
            read("sudo: a password is required", 1),
            Err(CommandError::NeedsPassword)
        );
        assert_eq!(
            read("Sorry, try again.\nsudo: 1 incorrect password attempt", 1),
            Err(CommandError::WrongPassword)
        );
        assert_eq!(
            read(
                "alex is not in the sudoers file.  This incident will be reported.",
                1
            ),
            Err(CommandError::NotAllowed)
        );
        assert_eq!(
            read("sudo: sorry, you must have a tty to run sudo", 1),
            Err(CommandError::NeedsTerminal)
        );
        assert_eq!(
            read("sh: 1: sudo: not found", 127),
            Err(CommandError::NoSudo)
        );
    }

    #[test]
    fn success_needs_the_marker() {
        assert_eq!(outcome("SHUTTLESCP-DONE\n", "", Some(0), false), Ok(()));
        // An SFTP-only account: status 0, nothing done.
        assert_eq!(outcome("", "", Some(0), false), Err(CommandError::NoShell));
        let denied = "chmod: changing permissions of '/etc/x': Operation not permitted";
        assert_eq!(
            outcome("", denied, Some(1), false),
            Err(CommandError::Failed(denied.to_owned()))
        );
    }
}

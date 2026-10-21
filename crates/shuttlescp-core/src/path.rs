//! Paths on either side of a connection.
//!
//! Local paths are the system's own ([`PathBuf`]): on Windows they use `\` and drive
//! letters. Server paths always use `/`, whatever system the app runs on, so they get
//! their own type, [`RemotePath`]: a [`PathBuf`] would put `\` between the parts on
//! Windows, which servers read as part of a name. [`SidePath`] is either kind, for code
//! that handles both sides alike (transfers, the file panes).

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::transfer::Side;

/// A path on the server: parts separated by `/`, on every system.
///
/// A trailing `/` is dropped (except for the root itself), so `/srv/` and `/srv` are the
/// same path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct RemotePath(String);

impl RemotePath {
    /// A path from its text, as the server reports or the user types it.
    pub fn new(text: impl Into<String>) -> Self {
        let mut text = text.into();
        while text.len() > 1 && text.ends_with('/') {
            text.pop();
        }
        Self(text)
    }

    /// The top folder, `/`.
    pub fn root() -> Self {
        Self(String::from("/"))
    }

    /// The path as the text the protocols expect.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path of `name` inside this folder.
    pub fn join(&self, name: &str) -> Self {
        if self.0.is_empty() {
            Self::new(name)
        } else if self.0.ends_with('/') {
            Self::new(format!("{}{name}", self.0))
        } else {
            Self::new(format!("{}/{name}", self.0))
        }
    }

    /// The folder this path is in; `None` for the root (or a bare name).
    pub fn parent(&self) -> Option<Self> {
        let cut = self.0.rfind('/')?;
        if self.0 == "/" {
            None
        } else if cut == 0 {
            Some(Self::root())
        } else {
            Some(Self::new(&self.0[..cut]))
        }
    }

    /// The last part of the path; `None` for the root.
    pub fn file_name(&self) -> Option<&str> {
        let name = self.0.rsplit('/').next()?;
        (!name.is_empty() && name != "..").then_some(name)
    }

    /// This path with its last part replaced by `name`.
    pub fn with_file_name(&self, name: &str) -> Self {
        match self.parent() {
            Some(parent) => parent.join(name),
            None => Self::new(name),
        }
    }

    /// Whether this path is `base` or inside it. Whole parts are compared, so
    /// `/srv/data2` is not inside `/srv/data`.
    pub fn starts_with(&self, base: &Self) -> bool {
        match self.0.strip_prefix(&base.0) {
            Some(rest) => rest.is_empty() || rest.starts_with('/') || base.0.ends_with('/'),
            None => false,
        }
    }
}

impl fmt::Display for RemotePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for RemotePath {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for RemotePath {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<RemotePath> for String {
    fn from(path: RemotePath) -> Self {
        path.0
    }
}

/// A path on this computer or on the server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SidePath {
    /// On this computer.
    Local(PathBuf),
    /// On the server.
    Remote(RemotePath),
}

impl SidePath {
    /// Which side the path is on.
    pub fn side(&self) -> Side {
        match self {
            Self::Local(_) => Side::Local,
            Self::Remote(_) => Side::Remote,
        }
    }

    /// The local path, if it is one.
    pub fn as_local(&self) -> Option<&Path> {
        match self {
            Self::Local(path) => Some(path),
            Self::Remote(_) => None,
        }
    }

    /// The server path, if it is one.
    pub fn as_remote(&self) -> Option<&RemotePath> {
        match self {
            Self::Local(_) => None,
            Self::Remote(path) => Some(path),
        }
    }

    /// The path of `name` inside this folder. A name that is not valid UTF-8 cannot be
    /// sent to a server as it is; on the server its odd bytes become `�`.
    pub fn join(&self, name: impl AsRef<OsStr>) -> Self {
        let name = name.as_ref();
        match self {
            Self::Local(path) => Self::Local(path.join(name)),
            Self::Remote(path) => Self::Remote(path.join(&name.to_string_lossy())),
        }
    }

    /// The folder this path is in; `None` at the top.
    pub fn parent(&self) -> Option<Self> {
        match self {
            Self::Local(path) => path
                .parent()
                .map(|parent| Self::Local(parent.to_path_buf())),
            Self::Remote(path) => path.parent().map(Self::Remote),
        }
    }

    /// The last part of the path; `None` at the top.
    pub fn file_name(&self) -> Option<&OsStr> {
        match self {
            Self::Local(path) => path.file_name(),
            Self::Remote(path) => path.file_name().map(OsStr::new),
        }
    }

    /// This path with its last part replaced by `name`.
    pub fn with_file_name(&self, name: impl AsRef<OsStr>) -> Self {
        let name = name.as_ref();
        match self {
            Self::Local(path) => Self::Local(path.with_file_name(name)),
            Self::Remote(path) => Self::Remote(path.with_file_name(&name.to_string_lossy())),
        }
    }

    /// Whether this path is `base` or inside it, on the same side.
    pub fn starts_with(&self, base: &Self) -> bool {
        match (self, base) {
            (Self::Local(path), Self::Local(base)) => path.starts_with(base),
            (Self::Remote(path), Self::Remote(base)) => path.starts_with(base),
            _ => false,
        }
    }
}

impl fmt::Display for SidePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(path) => path.display().fmt(f),
            Self::Remote(path) => path.fmt(f),
        }
    }
}

impl From<PathBuf> for SidePath {
    fn from(path: PathBuf) -> Self {
        Self::Local(path)
    }
}

impl From<RemotePath> for SidePath {
    fn from(path: RemotePath) -> Self {
        Self::Remote(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(text: &str) -> RemotePath {
        RemotePath::new(text)
    }

    #[test]
    fn remote_paths_join_with_a_slash_on_every_system() {
        assert_eq!(remote("/home/me").join("docs").as_str(), "/home/me/docs");
        assert_eq!(remote("/").join("srv").as_str(), "/srv");
        assert_eq!(remote("/home/me/").join("docs").as_str(), "/home/me/docs");
        assert_eq!(
            SidePath::Remote(remote("/srv")).join("a.txt").to_string(),
            "/srv/a.txt"
        );
    }

    #[test]
    fn remote_parent_and_name() {
        assert_eq!(remote("/home/me/docs").parent(), Some(remote("/home/me")));
        assert_eq!(remote("/home").parent(), Some(RemotePath::root()));
        assert_eq!(RemotePath::root().parent(), None);
        assert_eq!(remote("/home/me/a.txt").file_name(), Some("a.txt"));
        assert_eq!(RemotePath::root().file_name(), None);
        // Windows servers (OpenSSH) show drives as /C:.
        assert_eq!(remote("/C:").parent(), Some(RemotePath::root()));
        assert_eq!(remote("/C:").file_name(), Some("C:"));
    }

    #[test]
    fn a_backslash_is_part_of_a_remote_name() {
        let path = remote("/srv/odd\\name");

        assert_eq!(path.file_name(), Some("odd\\name"));
        assert_eq!(path.parent(), Some(remote("/srv")));
    }

    #[test]
    fn remote_with_file_name_renames_in_place() {
        assert_eq!(
            remote("/srv/a.txt").with_file_name("b.txt"),
            remote("/srv/b.txt")
        );
    }

    #[test]
    fn remote_starts_with_compares_whole_parts() {
        assert!(remote("/srv/data/a").starts_with(&remote("/srv/data")));
        assert!(remote("/srv/data").starts_with(&remote("/srv/data")));
        assert!(remote("/srv").starts_with(&RemotePath::root()));
        assert!(!remote("/srv/data2").starts_with(&remote("/srv/data")));
        assert!(!remote("/srv").starts_with(&remote("/srv/data")));
    }

    #[test]
    fn trailing_slashes_do_not_make_another_path() {
        assert_eq!(remote("/srv/"), remote("/srv"));
        assert_eq!(remote("//"), RemotePath::root());
    }

    #[test]
    fn sides_never_contain_each_other() {
        let local = SidePath::Local(PathBuf::from("/srv"));
        let server = SidePath::Remote(remote("/srv"));

        assert!(!server.starts_with(&local));
        assert!(!local.starts_with(&server));
        assert_eq!(server.side(), Side::Remote);
    }

    #[test]
    fn remote_paths_save_as_plain_text() {
        let json = serde_json::to_string(&remote("/srv/a")).expect("serialize");

        assert_eq!(json, r#""/srv/a""#);
        assert_eq!(
            serde_json::from_str::<RemotePath>(&json).expect("deserialize"),
            remote("/srv/a")
        );
    }
}

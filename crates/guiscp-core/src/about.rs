//! What the app is and how it was built, for the status bar and the About dialog.
//!
//! The build details (git commit, build time, ...) are recorded by each front end's
//! build script, since that is the program being built; this module only holds and
//! formats them, so every front end shows the same text.

/// The app's name as shown to users.
pub const APP_NAME: &str = "GuiSCP";

/// One-line description of the app.
pub const DESCRIPTION: &str = "Graphical SFTP client for copying files between this \
                               computer and remote servers, in the spirit of WinSCP.";

/// How the running program was built. Front ends fill this in from their build script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    /// Release version, e.g. "0.1.0".
    pub version: &'static str,
    /// Short git commit hash, or "unknown" when built outside a git checkout.
    pub commit: &'static str,
    /// The source had uncommitted changes when it was built.
    pub dirty: bool,
    /// When it was built, e.g. "2026-10-19 14:02 UTC".
    pub built: &'static str,
    /// Cargo profile: "debug" or "release".
    pub profile: &'static str,
    /// Platform it was built for, e.g. "x86_64-unknown-linux-gnu".
    pub target: &'static str,
    /// Compiler used, e.g. "rustc 1.90.0 (1159e78c4 2025-09-14)".
    pub rustc: &'static str,
}

impl BuildInfo {
    /// The commit, with "-dirty" appended when the source had uncommitted changes.
    ///
    /// # Returns
    ///
    /// E.g. "1b4b2e9" or "1b4b2e9-dirty"
    pub fn revision(&self) -> String {
        if self.dirty {
            format!("{}-dirty", self.commit)
        } else {
            self.commit.to_owned()
        }
    }

    /// Short text for a status bar: version, revision, build date, and the profile
    /// when it is not a release build.
    ///
    /// # Returns
    ///
    /// E.g. "v0.1.0 · 1b4b2e9 · 2026-10-19" or "v0.1.0 · 1b4b2e9-dirty · 2026-10-19 · debug"
    pub fn summary(&self) -> String {
        // The date is the first word of `built` ("2026-10-19 14:02 UTC").
        let date = self.built.split_whitespace().next().unwrap_or(self.built);
        let mut text = format!("v{} · {} · {date}", self.version, self.revision());
        if self.profile != "release" {
            text.push_str(" · ");
            text.push_str(self.profile);
        }
        text
    }

    /// Every build detail plus `extra` facts from the front end (toolkit version, ...)
    /// as "Label: value" lines, e.g. for an About dialog or a bug report.
    ///
    /// # Arguments
    ///
    /// * `extra` - Additional (label, value) lines, appended in order
    ///
    /// # Returns
    ///
    /// One line per detail
    pub fn details(&self, extra: &[(&str, String)]) -> String {
        let mut lines = vec![
            format!("{APP_NAME} {}", self.version),
            format!("Commit: {}", self.revision()),
            format!("Built: {}", self.built),
            format!("Profile: {}", self.profile),
            format!("Target: {}", self.target),
            format!("Compiler: {}", self.rustc),
            format!("Operating system: {}", os_description()),
        ];
        lines.extend(
            extra
                .iter()
                .map(|(label, value)| format!("{label}: {value}")),
        );
        lines.join("\n")
    }
}

/// Name of the running operating system, e.g. "Zorin OS 17.3" on Linux.
///
/// # Returns
///
/// The distribution's pretty name from `/etc/os-release` when there is one, otherwise
/// the OS family and architecture (e.g. "windows x86_64")
pub fn os_description() -> String {
    let fallback = || format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| os_release_pretty_name(&text))
        .map_or_else(fallback, |name| {
            format!("{name} ({})", std::env::consts::ARCH)
        })
}

/// Find `PRETTY_NAME` in the text of an `os-release` file.
///
/// # Arguments
///
/// * `text` - File contents: `KEY=value` lines, values optionally quoted
///
/// # Returns
///
/// The unquoted name, or `None` if the file has no (non-empty) `PRETTY_NAME`
fn os_release_pretty_name(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(dirty: bool, profile: &'static str) -> BuildInfo {
        BuildInfo {
            version: "0.1.0",
            commit: "1b4b2e9",
            dirty,
            built: "2026-10-19 14:02 UTC",
            profile,
            target: "x86_64-unknown-linux-gnu",
            rustc: "rustc 1.90.0",
        }
    }

    #[test]
    fn summary_of_a_clean_release_build() {
        assert_eq!(
            build(false, "release").summary(),
            "v0.1.0 · 1b4b2e9 · 2026-10-19"
        );
    }

    #[test]
    fn summary_marks_uncommitted_changes_and_debug_builds() {
        assert_eq!(
            build(true, "debug").summary(),
            "v0.1.0 · 1b4b2e9-dirty · 2026-10-19 · debug"
        );
    }

    #[test]
    fn details_lists_build_facts_then_extras() {
        let text = build(false, "release").details(&[("GTK", String::from("4.14.5"))]);

        let lines: Vec<&str> = text.lines().collect();

        assert_eq!(lines[0], "GuiSCP 0.1.0");
        assert!(lines.contains(&"Commit: 1b4b2e9"));
        assert!(lines.contains(&"Built: 2026-10-19 14:02 UTC"));
        assert_eq!(lines.last(), Some(&"GTK: 4.14.5"));
    }

    #[test]
    fn os_release_pretty_name_unquotes_and_skips_empty() {
        let text = "NAME=\"Zorin OS\"\nPRETTY_NAME=\"Zorin OS 17.3\"\nID=zorin\n";
        assert_eq!(
            os_release_pretty_name(text).as_deref(),
            Some("Zorin OS 17.3")
        );
        assert_eq!(os_release_pretty_name("PRETTY_NAME=\n"), None);
        assert_eq!(os_release_pretty_name("NAME=x\n"), None);
    }
}

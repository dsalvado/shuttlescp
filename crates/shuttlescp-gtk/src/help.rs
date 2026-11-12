//! The user guide: HTML pages shipped with the app (`docs/help` in the repository),
//! opened in the browser from Help › User Guide.

use std::path::{Path, PathBuf};

/// The guide's front page, if it can be found: in a package, beside the program's own
/// files (AppImage and Windows `bin/../share`, macOS `MacOS/../Resources/share`); in a
/// development build, in the repository it was built from.
pub fn guide_index() -> Option<PathBuf> {
    let program = std::env::current_exe().ok()?;
    let dir = program.parent()?;
    let packaged = [
        "../share/shuttlescp/help",
        "../Resources/share/shuttlescp/help",
    ]
    .into_iter()
    .map(|place| dir.join(place));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/help");
    packaged
        .chain(std::iter::once(repository))
        .map(|folder| folder.join("index.html"))
        .find(|index| index.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_development_build_finds_the_guide_in_the_repository() {
        let index = guide_index().expect("the guide is found");
        assert!(
            index.ends_with("docs/help/index.html"),
            "{}",
            index.display()
        );
    }
}

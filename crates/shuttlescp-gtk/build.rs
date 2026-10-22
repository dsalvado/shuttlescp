//! Records how the app is being built, as compile-time environment variables read by
//! `src/build_info.rs`: the git commit (and whether the source had uncommitted changes),
//! the build time, the Cargo profile, the target platform and the compiler version.
//!
//! Anything that cannot be found (no git, no git checkout) becomes "unknown" rather than
//! failing the build.
//!
//! The build time is when this script runs, so builds of one commit on different
//! machines differ. Release / CI builds should use the commit's time instead, so every
//! platform's build of a commit shows the same date (see BUILDING.md):
//!
//! ```sh
//! SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) cargo build --release
//! ```

use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Utc};

/// Placeholder for a fact that could not be found.
const UNKNOWN: &str = "unknown";

fn main() {
    let commit = git(&["rev-parse", "--short=7", "HEAD"]);
    // Changes to tracked files' contents only: stray untracked files, and permission
    // bits that look changed on Windows or shared drives, do not make a build "dirty".
    let dirty = git(&[
        "-c",
        "core.fileMode=false",
        "status",
        "--porcelain",
        "--untracked-files=no",
    ])
    .is_some_and(|changes| !changes.is_empty());
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| String::from("rustc"));
    let rustc_version = run(&rustc, &["--version"]);

    set(
        "SHUTTLESCP_GIT_COMMIT",
        commit.as_deref().unwrap_or(UNKNOWN),
    );
    set("SHUTTLESCP_GIT_DIRTY", if dirty { "true" } else { "false" });
    set("SHUTTLESCP_BUILT", &build_time());
    set("SHUTTLESCP_PROFILE", &env_or_unknown("PROFILE"));
    set("SHUTTLESCP_TARGET", &env_or_unknown("TARGET"));
    set(
        "SHUTTLESCP_RUSTC",
        rustc_version.as_deref().unwrap_or(UNKNOWN),
    );

    rerun_when_source_or_git_changes();
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        windows_resources();
    }
}

/// Windows: build the program icon (packaging/windows/shuttlescp.ico, see make-icon.sh)
/// and the version details Explorer shows under Properties into the .exe.
#[cfg(windows)]
fn windows_resources() {
    let icon = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/windows/shuttlescp.ico")
        .canonicalize()
        .expect("packaging/windows/shuttlescp.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = env!("CARGO_PKG_VERSION");
    // Resource files want the version as four numbers too: 0.10.0 is 0,10,0,0.
    let numbers = format!("{},0", version.replace('.', ","));
    // The resource compiler reads "\" in quoted text as an escape, so "/" it is.
    let icon = icon.display().to_string().replace('\\', "/");
    let icon = icon.strip_prefix("//?/").unwrap_or(&icon);
    let resources = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "{authors}"
      VALUE "FileDescription", "ShuttleSCP"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "shuttlescp"
      VALUE "LegalCopyright", "{authors}, {license}"
      VALUE "OriginalFilename", "shuttlescp.exe"
      VALUE "ProductName", "ShuttleSCP"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        authors = env!("CARGO_PKG_AUTHORS"),
        license = env!("CARGO_PKG_LICENSE"),
    );
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let rc = Path::new(&out).join("shuttlescp.rc");
    std::fs::write(&rc, resources).expect("write shuttlescp.rc");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .expect("compile the Windows resources");
}

/// When the build happens, in UTC. Honours `SOURCE_DATE_EPOCH` (the standard way to
/// make builds reproducible) when it is set.
fn build_time() -> String {
    let time = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|seconds| seconds.trim().parse::<i64>().ok())
        .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
        .unwrap_or_else(Utc::now);
    time.format("%Y-%m-%d %H:%M UTC").to_string()
}

/// Rerun this script, refreshing the commit, "dirty" flag and build time, whenever the
/// source of either crate changes or git moves to another commit. Without this Cargo
/// would run it only once, and the recorded commit would go stale.
fn rerun_when_source_or_git_changes() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../shuttlescp-core/src");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        // HEAD changes on checkout, the branch refs on commit, the index on staging.
        for name in ["HEAD", "index", "refs/heads", "packed-refs"] {
            let path = Path::new(&git_dir).join(name);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}

/// Pass `value` to the crate being built as the compile-time variable `name`.
fn set(name: &str, value: &str) {
    println!("cargo:rustc-env={name}={value}");
}

/// A Cargo-provided build script variable, or "unknown".
fn env_or_unknown(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| String::from(UNKNOWN))
}

/// Run git with `args` in this crate's folder.
fn git(args: &[&str]) -> Option<String> {
    run("git", args)
}

/// Run a program and return its trimmed output, or `None` if it cannot run or fails.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

# Building ShuttleSCP

## Development build

```sh
cargo run
```

## Build information

The build script (`crates/shuttlescp-gtk/build.rs`) records the following:

- the git commit, with `-dirty` added when tracked files have uncommitted changes;
- the build time;
- the Cargo profile;
- the target platform;
- the compiler version.

These show in the status bar and on the System page of Help > About. The version number
comes from `[workspace.package] version` in the top-level `Cargo.toml` and is shared by
every crate and platform. Keep it purely numeric (e.g. `0.2.0`), because Windows
installers and macOS app bundles require numeric versions.

## Release and CI builds

Build releases from a clean checkout so the commit has no `-dirty` marker. Set
`SOURCE_DATE_EPOCH` to the commit's time so the build is reproducible: every platform's
build of the same commit then shows the same date.

```sh
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) cargo build --release
```

On Windows (PowerShell):

```powershell
$env:SOURCE_DATE_EPOCH = git log -1 --format=%ct
cargo build --release
```

Git must be on the `PATH`. Without it, or when building from a source archive with no
`.git` folder, the commit shows as `unknown`. The build still succeeds.

`.gitattributes` keeps text files at LF line endings on every OS. Without it, a Windows
checkout could convert them to CRLF and every build would be marked `-dirty`.

## AppImage

```sh
packaging/linux/build-appimage.sh
```

This builds the release program and packs it, with GTK 4 and the libraries it needs, into
`dist/ShuttleSCP-<version>-x86_64.AppImage`. The first run downloads the packing tools
(linuxdeploy and its GTK plugin) into `target/appimage/tools`. An AppImage runs on
systems with the same or a newer C library than the one it was built on, so official
releases are built on Ubuntu 24.04 by the release workflow; see
[docs/RELEASING.md](docs/RELEASING.md).

## Windows

ShuttleSCP builds on Windows against GTK 4 from [MSYS2](https://www.msys2.org) (its
UCRT64 environment), with Rust's GNU toolchain, which links with MSYS2's libraries:

1. Install MSYS2, then in its **UCRT64** shell run `packaging/windows/install-build-deps.sh`
   (GTK 4, its icons, librsvg, gcc, binutils, pkgconf).
2. Install Rust ([rustup.rs](https://rustup.rs)) and the GNU toolchain:
   `rustup toolchain install stable-x86_64-pc-windows-gnu`.
3. Build and run, with MSYS2's `ucrt64\bin` on `PATH` (it is, in the UCRT64 shell):
   `cargo +stable-x86_64-pc-windows-gnu run --release`.

A release build is a Windows GUI program (no console window); a debug build keeps the
console, for development. The program's icon and version details are built into the
.exe from `packaging/windows/shuttlescp.ico` (made from the SVG by `make-icon.sh`).

To make the installer, in the UCRT64 shell:

```sh
packaging/windows/build-bundle.sh      # target/windows/ShuttleSCP: the program with GTK's files
powershell -File packaging/windows/check-bundle.ps1   # runs it without MSYS2, as a user's PC would
packaging/windows/build-installer.sh   # target/windows/ShuttleSCP-<version>-windows-x64-setup.exe
```

The installer is made by [Inno Setup](https://jrsoftware.org/isinfo.php) (version 6 or 7;
`packaging/windows/install-inno-setup.ps1` installs the version CI uses). It is not
code-signed, so Windows SmartScreen asks to confirm it the first time it runs.

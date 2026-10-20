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

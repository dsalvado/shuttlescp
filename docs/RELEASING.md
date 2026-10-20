# Releasing ShuttleSCP

How ShuttleSCP is checked, built, signed and published, and what to do for each
release. It is written for someone new to CI/CD, so it explains the why as well as the
how.

## The big picture

```
 you push code ──► CI workflow ──────────► ✓ or ✗ on the commit
                   (format, lint, test)

 you push a tag ─► Release workflow ─────► GitHub release:
   (v1.0.0)        test → build AppImage     ShuttleSCP-1.0.0-x86_64.AppImage
                   → sign manifest           latest.json + latest.json.sig
                                             SHA256SUMS
                                                │
         ┌──────────────────────────────────────┤
         ▼                                      ▼
 web page (GitHub Pages)              installed ShuttleSCP apps
 reads the latest release             check latest.json, verify the
 and offers the download              signature, download, install
```

- **CI (continuous integration)** means every change is checked automatically on a
  fresh machine, so mistakes are caught when they are made, not at release time.
- **CD (continuous delivery)** means releasing is one small, deliberate action (pushing a
  tag) and a machine does the rest the same way every time: no "works on my laptop"
  builds, no forgotten steps.

### Workflows, jobs, steps and runners

The automation lives in `.github/workflows/`, as YAML files GitHub reads:

| File | Runs when | Does |
|---|---|---|
| `ci.yml` | a push to `master`, or a pull request | formatting check, lint (clippy), tests |
| `release.yml` | a tag like `v1.0.0` is pushed | tests, AppImage, signed manifest, GitHub release |
| `pages.yml` | `site/` changes on `master` | publishes the web page |

Each workflow has **jobs**; each job runs on a **runner**, a fresh virtual machine GitHub
starts for that run and throws away afterwards, so nothing left over from an earlier run
can affect it. A job is a list of **steps**: shell commands (`run:`) or ready-made
**actions** (`uses:`, e.g. `actions/checkout` to fetch the code).

Watch them on the repository's **Actions** tab. Click a run, then a job, then a step to
read its output. A red ✗ there is the place to start when something fails.

### Secrets

The release signing key must never be in the repository. It is stored as a repository
**secret** (Settings › Secrets and variables › Actions): GitHub hands it only to the
workflow steps that name it, never to pull requests from other people's copies, and
masks it in logs. Nobody, not even you, can read a secret back from GitHub once it is
saved, which is why you also keep a backup.

## How updates stay safe

An app that downloads and runs new versions of itself is a tempting target: anyone who
could slip it a fake update would control every installed copy. ShuttleSCP therefore
trusts a signature, not the download location:

1. **A key pair.** `packaging/new-signing-key.sh` makes an Ed25519 key pair. The private
   half can make signatures; the public half can only check them. The public half is
   written into the app (`RELEASE_PUBLIC_KEY` in `crates/shuttlescp-core/src/update.rs`);
   the private half becomes the `UPDATE_SIGNING_KEY` secret.
2. **A signed manifest.** For each release, `packaging/make-release-manifest.sh` writes
   `latest.json` (version, AppImage name, download address, size and SHA-256) and signs
   it, giving `latest.json.sig`.
3. **The app checks before it trusts.** It downloads both files from the latest
   release and refuses the update unless the signature matches the built-in public key,
   the version is newer than its own (an old release cannot be replayed), and the
   downloaded AppImage has exactly the size and SHA-256 the signed manifest gives.
4. **A safe swap.** The new AppImage is downloaded next to the old one and replaces it
   in one rename, so the file is always either the old version or the new one.

So even someone who managed to upload files to the release, or tampered with the
download on its way, could not get an update installed without the private key.

## One-time setup

Done once, in this order:

1. **Make the signing key** (on your own computer):
   ```sh
   packaging/new-signing-key.sh
   ```
   It saves the private key in `~/.config/shuttlescp-release/update-signing-key.pem`
   (readable only by you) and prints the public key.
2. **Back up the private key** somewhere safe and offline, such as a password manager or
   an encrypted USB stick. If it is lost, installed copies can no longer update
   themselves; if it leaks, someone else could sign updates.
3. **Put the public key in the app**: paste it into `RELEASE_PUBLIC_KEY` in
   `crates/shuttlescp-core/src/update.rs` and commit.
4. **Store the private key as a secret**:
   ```sh
   gh secret set UPDATE_SIGNING_KEY < ~/.config/shuttlescp-release/update-signing-key.pem
   ```
5. **Turn on GitHub Pages** for the web page, publishing from GitHub Actions:
   ```sh
   gh api --method POST repos/dsalvado/shuttlescp/pages -f build_type=workflow
   ```
   (or Settings › Pages › Source: GitHub Actions).

## Making a release

1. **Pick the version.** ShuttleSCP uses [semantic versioning](https://semver.org):
   `major.minor.patch`. Raise *patch* for fixes only (0.9.0 → 0.9.1), *minor* for new
   features (0.9.1 → 0.10.0), *major* for changes that break existing use (→ 1.0.0).
2. **Set it** in the top-level `Cargo.toml` (`[workspace.package] version`), and add a
   `<release>` line to `packaging/linux/io.github.dsalvado.ShuttleSCP.metainfo.xml`.
   Run `cargo build` so `Cargo.lock` follows, then commit:
   ```sh
   git commit -am "Version 1.0.0"
   git push
   ```
   Wait for CI to pass (✓ on the commit).
3. **Tag the commit and push the tag.** A tag is a name for one commit; pushing one that
   starts with `v` starts the release workflow:
   ```sh
   git tag -a v1.0.0 -m "ShuttleSCP 1.0.0"
   git push origin v1.0.0
   ```
4. **Watch it** on the Actions tab (about 10 to 15 minutes). When it finishes, the
   release appears under Releases, the web page offers it, and installed copies see it
   on their next check.

The release workflow stops, without publishing anything, if the tag does not match the
version in `Cargo.toml`, a test fails, the AppImage cannot be built, or the signing key
is missing or does not match the app's public key.

### If a release goes wrong

- **It failed before publishing:** fix the problem, then move the tag to the fixed
  commit and push it again:
  ```sh
  git tag -d v1.0.0 && git push origin :refs/tags/v1.0.0
  git tag -a v1.0.0 -m "ShuttleSCP 1.0.0" && git push origin v1.0.0
  ```
- **A published release is broken:** do not reuse its version number, since installed
  copies may already have it. Fix the problem and release the next patch version
  (1.0.1).

## Trying things on your own computer

- **Build the AppImage**: `packaging/linux/build-appimage.sh` makes
  `dist/ShuttleSCP-<version>-x86_64.AppImage`, exactly as the release workflow does.
- **Check workflow files** before pushing them with
  [actionlint](https://github.com/rhysd/actionlint): `actionlint .github/workflows/*.yml`.
- **Run the checks CI runs**:
  ```sh
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo test --workspace --locked
  ```

# Security policy

ShuttleSCP handles server passwords and keys, and it updates itself, so security
problems matter. Thank you for reporting them responsibly.

## Supported versions

Only the latest release receives security fixes. ShuttleSCP is in public beta (0.x);
please update before reporting (Help › Check for Updates).

| Version | Supported |
|---|---|
| Latest 0.x release | Yes |
| Older releases | No |

## Reporting a vulnerability

**Please do not open a public issue for a security problem.** Report it privately
through GitHub instead:

1. Go to the repository's [Security tab](https://github.com/dsalvado/shuttlescp/security).
2. Choose **Report a vulnerability**.
3. Describe the problem, how to reproduce it, which version you used, and what an
   attacker could do with it.

Only the maintainer sees the report. ShuttleSCP is maintained by one person in their
own time, so please allow:

- **7 days** for a first reply,
- **30 days** for a fix or a plan, depending on severity.

Once a fix is released, the report is published as a GitHub security advisory, crediting
you unless you prefer otherwise.

## What is in scope

Of particular interest:

- **The updater**: anything that would make ShuttleSCP install an update not signed with
  the release key, install an older version, or run a file other than the verified one.
- **Credentials**: saved passwords leaving the desktop keyring, appearing in logs or
  files, or site exports decryptable without their passphrase.
- **Connections**: SSH host keys or TLS certificates not being verified, or a downgrade
  to plain FTP when encryption was required.
- **Files**: a server being able to write outside the folder you chose (path traversal).

Out of scope: problems that need an attacker who already controls your user account or
computer, and bugs in the libraries ShuttleSCP uses (please report those upstream, and
tell us if ShuttleSCP needs to update).

## How releases are signed

Each release's update manifest (`latest.json`) is signed with an Ed25519 key. Its public
half is built into the app (`RELEASE_PUBLIC_KEY` in
`crates/shuttlescp-core/src/update.rs`) and the private half is held only by the
maintainer and the release workflow. If the key were ever compromised, a new key would be
announced here and in the release notes, and installed copies would need to be updated by
hand once. See [docs/RELEASING.md](docs/RELEASING.md) for the details.

To check a download by hand, compare it with the release's `SHA256SUMS`:

```sh
sha256sum --check SHA256SUMS
```

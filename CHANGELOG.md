# Changelog

All notable changes to ShuttleSCP are recorded here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Each release's section
becomes its release notes on GitHub.

## [Unreleased]

## [0.9.1] - 2026-10-21

### Fixed

- The AppImage includes GTK's add-ons again: SVG images load (0.9.0 showed a fallback
  arrow on the ".." row), drawing uses the graphics card instead of falling back to
  software, and input methods (accents, Asian scripts) work.

### Changed

- Every change now builds the AppImage in CI too, so packaging problems show before a
  release.

## [0.9.0] - 2026-10-21

The first public release: a public beta. Feedback and bug reports are welcome on
[GitHub Issues](https://github.com/dsalvado/shuttlescp/issues).

### Added

- Connections over SFTP and SCP (SSH), and FTP with plain, explicit or implicit TLS
  (FTPS), in passive or active mode.
- SSH login with a private key file (OpenSSH, or PuTTY `.ppk` brought over from WinSCP)
  or the keys of the desktop's SSH agent, tried before the password as `ssh` does. A
  protected key asks for its passphrase, which can be saved in the keyring.
- Saved sites, with passwords kept in the desktop keyring (GNOME Keyring, KWallet), and
  export and import of sites with passwords encrypted under a passphrase.
- Two file panes, this computer and the server, sortable by name, size or date, with
  the desktop's file icons.
- Upload, download, move, rename, delete and new folder, from the right-click menu, the
  keyboard (Delete, F2) or drag and drop (between the panes, or onto a folder in the
  same pane). Dragging with the right mouse button selects a range of rows.
- Several connections at once, each in its own tab and colour, with its own transfer
  lists (Queue, Completed, Failed).
- Transfers that run side by side, show progress, speed and time left, ask before
  replacing a file, and continue where they stopped after an interruption.
- Files being written by a transfer are shown in red until they are complete.
- A log of every session and transfer, in the app and in a file, with selectable detail.
- Light and dark themes, following the desktop's GTK theme or one picked in View › Theme.
- A Linux AppImage that runs on 64-bit systems from 2024 onwards (Ubuntu 24.04,
  Fedora 40, Debian 13 and the like).
- Update checks (Help › Check for Updates, and daily at startup, which can be turned
  off). Updates are installed only when signed with the ShuttleSCP release key.

[Unreleased]: https://github.com/dsalvado/shuttlescp/compare/v0.9.1...HEAD
[0.9.1]: https://github.com/dsalvado/shuttlescp/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/dsalvado/shuttlescp/releases/tag/v0.9.0

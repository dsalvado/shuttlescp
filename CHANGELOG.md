# Changelog

All notable changes to ShuttleSCP are recorded here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Each release's section
becomes its release notes on GitHub.

## [Unreleased]

### Added

- Double-clicking a file in the local pane (or pressing Enter on it) opens it in the app
  your system uses for that type, as the file manager would.

## [0.11.0] - 2026-10-21

### Added

- **Windows version** (64-bit Windows 10 and 11), as an installer that needs no
  administrator rights, with a Start menu entry and an uninstaller. Passwords are saved
  in the Credential Manager, the file lists show Explorer's icons, SSH keys can come
  from Windows' OpenSSH agent or PuTTY's Pageant, and it updates itself through the
  next release's installer (checked against the release key). The installer is not
  code-signed, so Windows SmartScreen asks to confirm it the first time.
- On Windows, names Windows does not allow are refused when creating or renaming
  (`\ / : * ? " < > |`, a space or dot at the end, CON, NUL, COM1...), and server files
  with such names are downloaded with the character written as `%XX` (`notes:v2.txt`
  becomes `notes%3Av2.txt`), as WinSCP does.

### Changed

- The open menu's title in the menu bar is shown with a soft highlight.
- Messages from GTK and the libraries under it (warnings about icons or fonts) go to the
  log instead of the terminal.

### Fixed

- Deleting files in the local pane did nothing while no server was connected.
- Choosing System default (or a light theme) after a dark or high-contrast theme kept
  the dark or high-contrast look where the desktop does not set one (Windows).

## [0.10.0] - 2026-10-21

### Added

- **macOS version** (Apple silicon, macOS 14 or newer), as a disk image with a
  self-contained app. It follows the Mac: menus in the menu bar at the top of the screen,
  window buttons on the left, the system's light or dark appearance, Mac-sized text,
  Mac-style icons (from the WhiteSur icon theme) and the file icons Finder shows.
  Passwords are saved in the Keychain. It checks for new versions and offers the new
  disk image; it is not signed with an Apple Developer ID, so macOS asks to confirm it
  the first time it is opened.
- The server's pane has a drive list too: its mounted disks and network shares (Linux),
  volumes (macOS) or drive letters (Windows).

### Fixed

- A password whose saving to the keyring failed is kept in the field for logging in,
  instead of being lost.
- With GTK's default theme (no Adwaita-style theme installed), the selected session and
  transfer tabs were see-through and some highlights were missing.

## [0.9.3] - 2026-10-21

### Changed

- The View › Theme menu is wider with the names centred, and each theme shows a small
  swatch of its window and accent colours.

## [0.9.2] - 2026-10-21

### Fixed

- The AppImage follows the theme picked in View › Theme (and the desktop's theme)
  again; 0.9.1 was always drawn in plain Adwaita.

### Changed

- Every built AppImage is checked automatically before release: GTK's add-ons are
  inside, and nothing forces the theme or the display backend.

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

[Unreleased]: https://github.com/dsalvado/shuttlescp/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/dsalvado/shuttlescp/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/dsalvado/shuttlescp/compare/v0.9.3...v0.10.0
[0.9.3]: https://github.com/dsalvado/shuttlescp/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/dsalvado/shuttlescp/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/dsalvado/shuttlescp/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/dsalvado/shuttlescp/releases/tag/v0.9.0

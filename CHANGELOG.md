# Changelog

All notable changes to ShuttleSCP are recorded here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Each release's section
becomes its release notes on GitHub.

## [Unreleased]

### Changed

- Transfer progress bars show which way files are going: they fill from the pane the
  files come from, with an arrowhead at the leading edge, and while data is moving their
  stripes slide toward the other pane (left to right from the left pane, right to left
  from the right one).
- Site-to-site transfer titles always name the left pane's server first, with the arrow
  showing the way: "Copy report.pdf: left → right" or "Copy report.pdf: left ← right",
  so a transfer reads the same however the list is ordered.

## [0.15.1] - 2026-10-23

### Fixed

- The Login dialog's Site to site lists now show sites added, renamed or deleted on the
  Direct tab without closing the dialog. Deleting a site could also leave a site-to-site
  pick pointing at the next site in the list.

## [0.15.0] - 2026-10-23

### Added

- **Site to site:** copy and move files between two servers, through this computer
  (like `scp -3`), in any combination of SFTP, SCP and FTP/FTPS. The Login dialog has a
  new Site to site tab: pick a saved site for each pane and log in. The session tab is
  titled "left ⇄ right", and between the panes a picture of this computer shows arrows
  for the way files are going and two disk-style lights (green receiving, amber
  sending) that blink while transfers run. Transfers are titled "Copy report.pdf:
  left → right", can be paused, stopped and resumed like any other, and are offered for
  resuming after a restart when the same two sites are connected again.
- Each file pane has a title bar naming what it shows: "This computer · <name>", or the
  server, with its host name once logged in (SFTP and SCP).
- The Site to site tab of the Login dialog shows the picked site's details under each
  list.

### Fixed

- Two transfers could write the same files at once: starting the same transfer again
  after an interrupted one left two resumable records, and resuming both ran them side
  by side over the same partial files. A transfer over files that another unfinished
  one (running or paused) is changing now does not start (it goes to Failed, to retry
  later), and a new transfer replaces older interrupted records over the same files.

### Changed

- Quitting with unfinished transfers, running or paused, asks what to do with them:
  keep them (and their partial files) to resume the next time you connect, or delete
  their partial files and quit. Paused transfers used to be dropped without a word.
- The right-click menu names where files go: "Upload to <server>", "Download to this
  computer", site to site "Copy to <server>", and "Move to ...".
- A transfer that fails stays in the Queue a few seconds, so its message can be read,
  before moving to Failed.

## [0.14.0] - 2026-10-22

### Changed

- The Linux AppImage now also runs on older systems: Ubuntu 20.04 and 22.04, Debian 12,
  Linux Mint 20 and 21 and the like, where it used to need a system from 2024 or later.
  It carries the C library and the few other libraries those systems have in older
  versions, and uses them only there; newer systems run it exactly as before. Every
  build is started on Ubuntu 22.04 and Debian 12 before it is released.

## [0.13.0] - 2026-10-22

### Added

- A single file or folder can be stopped while the rest of its transfer goes on: each
  unfinished item in the Queue has its own stop button. A stopped file's partial copy
  is removed (when moving, it stays where it was), and the transfer still counts as
  completed ("1 stopped").
- Transfers can be paused: the pause button on a running copy or move stops it but keeps
  what was copied (the partial files stay in place), and it waits in the Queue as
  "Paused" until resumed with its play button, continuing where it stopped. A paused
  transfer is also offered for resuming after ShuttleSCP restarts; its close button
  cancels it.
- Single files can be paused too: each file (or folder) being copied has its own pause
  button. A paused file gives its place to the next one, keeps its partial copy, and
  continues where it stopped when resumed with its play button; the rest of the
  transfer goes on meanwhile.

### Changed

- Selecting several files works as in a file manager: press the left button on an empty
  spot (anywhere but a file's icon or name, including the empty space below the rows)
  and drag a box over them. Ctrl (Cmd on macOS) adds to the selection, the list scrolls
  when the box reaches its edge, and a click on the empty space clears the selection.
  Dragging a file's icon or name still drags the selected files. This replaces
  selecting with a right-button drag.

### Fixed

- A ShuttleSCP built from source in debug mode (`cargo run`) no longer takes over the
  app launcher's ShuttleSCP entry, which then started that build instead of the
  installed AppImage.

## [0.12.0] - 2026-10-22

### Added

- Double-clicking a file in the local pane (or pressing Enter on it) opens it in the app
  your system uses for that type, as the file manager would.
- A newer release is shown in the status bar ("Update available: 1.0.0") until it is
  installed or the notice is closed, so it is not missed while another dialog is open.
  Clicking it offers the update again; once the update is in place it reads "Restart
  to finish updating".
- An update that finishes downloading while transfers run can wait for them: "When
  Transfers Finish" restarts ShuttleSCP into the new version by itself once the last
  transfer is done (the status bar says "Update waits for transfers" meanwhile).
  "Restart Now" stops them as before, and "Later" leaves the restart to the status bar.

### Fixed

- Building the macOS disk image tries again when macOS's disk-image tool fails for no
  reason of ours (it did once on GitHub's Mac machines), and shows its message.

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

[Unreleased]: https://github.com/dsalvado/shuttlescp/compare/v0.15.1...HEAD
[0.15.1]: https://github.com/dsalvado/shuttlescp/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/dsalvado/shuttlescp/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/dsalvado/shuttlescp/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/dsalvado/shuttlescp/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/dsalvado/shuttlescp/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/dsalvado/shuttlescp/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/dsalvado/shuttlescp/compare/v0.9.3...v0.10.0
[0.9.3]: https://github.com/dsalvado/shuttlescp/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/dsalvado/shuttlescp/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/dsalvado/shuttlescp/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/dsalvado/shuttlescp/releases/tag/v0.9.0

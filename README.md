# ShuttleSCP

[![CI](https://github.com/dsalvado/shuttlescp/actions/workflows/ci.yml/badge.svg)](https://github.com/dsalvado/shuttlescp/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/dsalvado/shuttlescp?label=release)](https://github.com/dsalvado/shuttlescp/releases/latest)
[![Licence: GPL v3 or later](https://img.shields.io/badge/licence-GPL--3.0--or--later-blue)](LICENSE)

A graphical SFTP and FTP client for Linux, inspired by WinSCP: your computer's
files on the left, the server's on the right, and copying between them is a drag, a
right-click or a key press away.

Written in Rust with [relm4](https://relm4.org) and GTK 4.

![ShuttleSCP with two connections open](docs/screenshot.png)

> **Status: public beta (0.9).** It is used day to day on Linux, and feedback is very
> welcome: please [open an issue](https://github.com/dsalvado/shuttlescp/issues). Windows
> and macOS front ends are planned (the transfer engine is kept free of GTK for that).
> See the [changelog](CHANGELOG.md) for what each release brought.

## Features

**Connections**

- SFTP and SCP over SSH, and FTP with plain, explicit or implicit TLS (FTPS), in
  passive or active mode.
- Saved sites, with passwords kept in the desktop keyring (GNOME Keyring, KWallet...),
  never in a plain file.
- Export and import of sites, with saved passwords encrypted under a passphrase you
  choose, to move them to another computer.
- Several connections at once, each in its own tab with its own colour. The tab, its
  panes and its transfer lists share that colour, so it is always clear which belongs
  to which.

**Files**

- Two file panes, sortable by name, size or date, with the desktop's file icons.
- Upload, download, move, rename, delete and create folders from the right-click menu
  or the keyboard (Delete, F2).
- Drag and drop between the panes, or onto a folder in the same pane, then choose Copy
  Here or Move Here. Drag with the right mouse button to select a range of rows.
- The local pane remembers the last folder on each disk, and the app reopens where you
  left off.

**Transfers**

- Several transfers run side by side. Each one shows its folders and files as a tree,
  with progress, speed and time left.
- A file that already exists is never replaced without asking: overwrite, skip or
  cancel, for one file or all of them.
- Interrupted transfers continue where they stopped, even after the app was closed or
  the connection dropped.
- Files being written show in red in the panes until they are complete. Finished
  transfers move to Completed or Failed, where failed ones can be retried.
- Quitting or closing a tab while transfers run asks first.

**Everything else**

- A compact interface that follows your GTK theme, light or dark.
- A log of every session and transfer, with selectable detail, shown in the app and
  kept in a file.

## Installing

Download the AppImage from the [ShuttleSCP web page](https://shuttlescp.dsalvado.ca/)
or the [latest release](https://github.com/dsalvado/shuttlescp/releases/latest). It is one
file holding ShuttleSCP and everything it needs, for 64-bit Linux from 2024 or later
(Ubuntu 24.04, Fedora 40, Debian 13 and the like):

```sh
chmod +x ShuttleSCP-*-x86_64.AppImage
./ShuttleSCP-*-x86_64.AppImage
```

The first run adds ShuttleSCP to your desktop's app launcher. It checks for new versions
(at most once a day; Help › Check for Updates to check now, or to turn it off) and
installs them only when they are signed with the ShuttleSCP release key.

You can also build it from source, as described below.

## Building

You need:

- **Rust** 1.88 or newer ([rustup.rs](https://rustup.rs)).
- **GTK 4** (4.14 or newer) development files, and a C compiler:

  | Distribution | Command |
  |---|---|
  | Debian, Ubuntu, Mint, Zorin | `sudo apt install build-essential libgtk-4-dev` |
  | Fedora | `sudo dnf install gcc gtk4-devel` |
  | Arch, Manjaro | `sudo pacman -S base-devel gtk4` |

- To save passwords: a desktop keyring that speaks the Secret Service protocol (GNOME
  Keyring or KWallet, which most desktops already run).

Then:

```sh
git clone https://github.com/dsalvado/shuttlescp.git
cd shuttlescp
cargo run --release
```

The program is `target/release/shuttlescp`. To build the AppImage yourself, run
`packaging/linux/build-appimage.sh`. See [BUILDING.md](BUILDING.md) for release builds
and the build information shown in the app, and [docs/RELEASING.md](docs/RELEASING.md)
for how releases are made, signed and published.

## Where it keeps things

| What | Where |
|---|---|
| Settings and saved sites | `~/.config/shuttlescp/` |
| Log and unfinished transfers | `~/.local/state/shuttlescp/` |
| Passwords | Your desktop keyring, under "ShuttleSCP" |

## Project layout

- `crates/shuttlescp-core`: everything that is not the user interface (sites,
  settings, the keyring, SSH, SFTP and FTP, the transfer engine, logging). It does not
  depend on GTK, so other front ends can reuse it.
- `crates/shuttlescp-gtk`: the Linux front end, built with relm4 and GTK 4.

Run the tests with `cargo test --workspace`.

## How this was built

ShuttleSCP was built with [Claude Code](https://claude.com/claude-code), Anthropic's AI
coding agent, and I want to be upfront about that. I moved my laptop from Windows 10 to
Linux and missed WinSCP, which does not run well under Wine, so I set out to build the
tool I wanted, in Rust, and to use the project to learn modern project engineering with
AI tools.

The split of work: Claude wrote the code, tests and documentation. I decided what to
build and how it should look and behave, reviewed every change, tested it against my own
servers, and made the calls on design, security and releases (for example signed
updates, keeping the core free of GTK for other platforms, and publishing under the GPL).
Commits made with Claude carry a `Co-Authored-By: Claude` line, so the history shows
exactly which they are.

I'm David Salvador; more about me at [dsalvado.ca](https://dsalvado.ca).

## Security

Please report security problems privately, as described in [SECURITY.md](SECURITY.md),
rather than in a public issue.

## Licence

ShuttleSCP is free software: you can redistribute it and/or modify it under the terms
of the GNU General Public License as published by the Free Software Foundation, either
version 3 of the License, or (at your option) any later version. See [LICENSE](LICENSE).

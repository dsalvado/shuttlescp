# ShuttleSCP's interface texts in English (the original, and the fallback for any
# text a translation lacks). Each translation is a folder named after its language
# tag (fr-FR, de-DE, ...) holding a file like this one, with the same message names.
#
# Fluent syntax: https://projectfluent.org/fluent/guide/

## The product's name, used inside messages as { -app-name }. Not translated.

-app-name = ShuttleSCP

## This language's own name, shown in the Preferences window's language list.

language-name = English

## Buttons used in many dialogs.

dialog-close = Close

## The Preferences window (preferences_dialog.rs).

preferences-title = Preferences
# The underscore marks the keyboard shortcut letter (Alt+L).
preferences-language = Interface _language:
preferences-restart = Restart { -app-name } to use the new language.

## The status bar (app.rs). Variables: $running and $limit are numbers of
## transfers, $count the transfers waiting.

status-transfers-unlimited = Transfers: unlimited
status-transfers-limited = Transfers: { $running } of { $limit }
status-transfers-waiting = { $count } waiting
dialog-cancel = Cancel

## The Quit dialog, when transfers are unfinished (quit_dialog.rs). $count is a number
## of transfers; quit-detail starts with one of the three messages after it.

quit-title = Quit
quit-question = Quit { -app-name }?
quit-detail = { $unfinished } Keep them to resume the next time you connect, or delete their partial files.
quit-running =
    { $count ->
        [one] 1 transfer is still running.
       *[other] { $count } transfers are still running.
    }
quit-paused =
    { $count ->
        [one] 1 transfer is paused.
       *[other] { $count } transfers are paused.
    }
# Always 2 or more transfers.
quit-unfinished = { $count } transfers are unfinished ({ $running } running, { $paused } paused).
quit-discard = Delete Partial Files and Quit
quit-keep = Keep for Later and Quit

## Exporting and importing sites (passphrase_dialog.rs).

passphrase-export-title = Export Sites
passphrase-import-title = Import Sites
passphrase-export-explain = Saved passwords are encrypted with a passphrase you choose. You will need it to import the file on another computer.
passphrase-import-explain = This file contains encrypted passwords. Enter the passphrase it was exported with.
passphrase-include = Include saved passwords
passphrase-new = Passphrase (at least 8 characters)
passphrase-field = Passphrase
passphrase-repeat = Repeat passphrase
passphrase-export = Export
passphrase-import = Import

## Asking for a private key's passphrase (key_passphrase_dialog.rs). $key is the key
## file's name.

key-passphrase-title = Key Passphrase
key-passphrase-question = The key { $key } is protected by a passphrase
key-passphrase-wrong = That passphrase does not open the key. Please try again.
key-passphrase-explain = Enter its passphrase to log in with it.
key-passphrase-save = Save passphrase in the keyring
key-passphrase-unlock = Unlock

## A file being copied already exists (conflict_dialog.rs). $name is the file's name,
## $folder the folder it is copied to.

conflict-title = File Already Exists
conflict-question = { $name } already exists in { $folder }
conflict-existing = Existing:
conflict-new = New:
conflict-for-all = Do the same for the other existing files
conflict-cancel = Cancel Transfer
conflict-skip = Skip
conflict-overwrite = Overwrite
# Added after the date of the more recent of the two files.
conflict-newer = {" "}(newer)
key-passphrase-save-tooltip = Keep it in the desktop keyring (encrypted with your login password), so the key opens without asking

## Updates (update_dialog.rs). $version, $current: version numbers like 0.20.0;
## $count: transfers running; $error: what went wrong (in English, from the
## program); $done, $total: sizes like "4.2 MiB".

update-restart-title = Restart
update-restart-question = Restart into { -app-name } { $version }?
update-restart-running =
    { $count ->
        [one] A transfer is still running.
       *[other] { $count } transfers are still running.
    }
# Starts with update-restart-running.
update-restart-detail = { $running } { -app-name } can restart by itself once they finish, or now: they stop, and can be resumed the next time you connect to the server.
update-later = Later
update-restart-now = Restart Now
update-when-finished = When Transfers Finish
update-check-title = Check for Updates
update-up-to-date = { -app-name } is up to date
update-latest = Version { $version } is the latest release.
update-check-failed = Cannot check for updates
update-check-error = { $error }.
update-available-title = Update Available
update-available = { -app-name } { $version } is available
# $date: when the new version was released, like 2026-11-01.
update-you-have = You have version { $current }. The new version was released on { $date }.
update-whats-new = What's new in { $version }
update-mac-steps = Download it, open the disk image and drag { -app-name } onto Applications, replacing this version.
update-download = Download
update-from-page = Download the new version from the { -app-name } web page.
update-open-page = Open Download Page
update-downloading = Downloading...
update-install = Install and Restart
update-downloading-progress = Downloading... { $done } of { $total }
update-failed = { $error }. You can download it from the { -app-name } web page instead.

## The menus (app.rs). The underscore marks each item's keyboard letter: pick
## letters that are unique within each menu.

menu-file = _File
menu-edit = _Edit
menu-view = _View
menu-tools = _Tools
menu-window = _Window
menu-help = _Help
menu-login = _Login…
menu-disconnect = _Disconnect
menu-preferences = _Preferences…
# The macOS name of Preferences.
menu-settings = _Settings…
menu-quit = _Quit
menu-theme = Theme
menu-log-level = _Log Level
menu-open-log = _Open Log File
menu-check-updates = Check for _Updates…
menu-auto-updates = Check for Updates at _Startup
menu-about = _About { -app-name }

## The macOS Edit menu (app.rs).

edit-undo = Undo
edit-redo = Redo
edit-cut = Cut
edit-copy = Copy
edit-paste = Paste
edit-delete = Delete
edit-select-all = Select All

## The main window (app.rs, session.rs). $name is a server or connection name, like
## me@example.com; $left and $right the two servers of a site-to-site session.

not-connected = Not connected
new-connection = New Connection
new-connection-tooltip = Connect to another server in a new tab
# The local pane's title; $name is this computer's name.
this-computer-title = This computer · { $name }
# As in "Copy to this computer".
this-computer = this computer
status-connected = Connected to { $name }
status-connected-pair = Connected to { $left } and { $right }
status-transfers-tooltip = Transfers running at once, over all connections. Click to limit them.
# $level: Error, Warn, Info, Debug or Trace (not translated).
status-log-level = Log level: { $level }
session-activity = Transfers running
session-close = Disconnect and close this session
close-session-title = Close Session
close-session-question = Close the session?
stop-transfers-detail =
    { $count ->
        [one] A transfer is still running. It will stop, and can be resumed the next time you connect to this server.
       *[other] { $count } transfers are still running. They will stop, and can be resumed the next time you connect to this server.
    }
delete-title = Delete
delete-detail = Folders are deleted with everything in them. This cannot be undone.

## The status bar's update notice (app.rs). $version, $current: version numbers.

update-notice-available = Update available: { $version }
update-notice-waiting = Update waits for transfers
update-notice-restart = Restart to finish updating
update-notice-available-tooltip = { -app-name } { $version } is available (you have { $current }). Click to see it.
update-notice-waiting-tooltip = { -app-name } restarts into { $version } once the running transfers finish. Click to restart now or later instead.
update-notice-restart-tooltip = { -app-name } { $version } is ready. Click to restart into it.
update-notice-hide = Hide this notice

## The transfer limit's panel, from the status bar (app.rs).

limit-title = Transfers running at once
limit-unlimited = Unlimited
limit-at-most = At most
limit-hint = Over all connections. More wait in the Queue and start as others finish.

## Site to site: the picture between the two panes (session.rs).

relay-tooltip = Files copied between the two servers pass through this computer
relay-to-right = Copying from the left server to the right one
relay-to-left = Copying from the right server to the left one
relay-receiving = Receiving from the server copies come from
relay-sending = Sending to the server copies go to

## The bottom panel's tabs (app.rs, session.rs). A tab shows "Queue (2)" while
## jobs run; $list is one of the list-…-tooltip names.

panel-log = Log
list-queue = Queue
list-completed = Completed
list-failed = Failed
list-queue-tooltip = Transfers
list-completed-tooltip = Completed transfers
list-failed-tooltip = Failed transfers
list-for-connection = { $list } for { $name }
list-not-connected = { $list } (not connected)
log-unavailable = Logging to a file is not available, so there is nothing to show.

## The theme list in the View menu (theme_list.rs) and the disk picker.

theme-system = System default
theme-swatch = Window and accent colours
disk-tooltip = Disk

## The About window (build_info.rs). The system details below it stay in English,
## for bug reports.

about-title = About { -app-name }
# $revision: the build's code revision, like 4b9928b.
about-version = Version { $version } ({ $revision })
about-description = A graphical SFTP, SCP and FTP client for Linux, macOS and Windows, inspired by WinSCP.
about-authors = By { $authors }
about-license = Free software: { $license }
about-system-details = System details
about-copy-details = Copy Details

## The file panes (file_pane.rs). $name is a file, folder or server name; $folder a
## folder's path; $saved when a saved listing was made, like 2026-11-01 10:02.

pane-gathering = Gathering information...
pane-gathering-tooltip = Gathering information
pane-saved-gathering = Saved list from { $saved } · gathering information...
pane-saved-stale = Saved list from { $saved }; could not check for changes: { $error }
pane-refresh-tooltip = Refresh
pane-items =
    { $count ->
        [one] 1 item
       *[other] { $count } items
    }
pane-created = Created folder { $name }
pane-renamed = Renamed to { $name }
pane-new-folder-title = New Folder
pane-new-folder-prompt = Create a folder in { $folder }:
pane-new-folder-create = Create
# The name first offered for a new folder.
pane-new-folder-name = New Folder
pane-rename-title = Rename
pane-rename-prompt = New name for { $name }:
pane-open-title = Open File
pane-open-failed = Cannot open { $name }
# $error: the system's reason, in its own language.
pane-open-error = { $error }.
pane-column-name = Name
pane-column-size = Size
pane-column-modified = Modified

## The file panes' right-click menu. The underscore marks the keyboard letter.
## $kind is upload, download or copy: the item copies files to the other pane,
## named $name (another server, or "this computer").

pane-transfer =
    { $kind ->
        [upload] _Upload
        [download] _Download
       *[copy] _Copy
    }
pane-transfer-to =
    { $kind ->
        [upload] _Upload to { $name }
        [download] _Download to { $name }
       *[copy] _Copy to { $name }
    }
pane-move = _Move
pane-move-to = _Move to { $name }
pane-menu-new-folder = _New Folder…
pane-menu-rename = _Rename…
pane-menu-delete = _Delete
pane-menu-refresh = Re_fresh

## The menu after dropping files with the right mouse button.

pane-drop-copy = _Copy Here
pane-drop-move = _Move Here
pane-drop-cancel = C_ancel

## The transfer lists (transfer_panel.rs). $done, $total: sizes like "4.2 MiB" or
## counts, as the message says; $count: a number of transfers, files or items.

transfers-resume = Resume
transfers-discard = Discard
transfers-discard-tooltip = Forget them; their partial files stay where they are
transfers-hide-finished = Hide finished files
transfers-hide-finished-tooltip = Leave files copied without problems out of the list, so the ones being copied stay at the top
transfers-queue-empty = Queue empty
transfers-completed-empty = No completed transfers
transfers-failed-empty = No failed transfers
transfers-clear = Clear
transfers-interrupted =
    { $count ->
        [one] 1 transfer was interrupted and can be resumed.
       *[other] { $count } transfers were interrupted and can be resumed.
    }
# $other: the title of the transfer using the same files.
transfers-blocked = Not started: "{ $other }" is using the same files. Retry once it has finished.

## A running copy's line, in parts joined by " · ": "55% · 14:20 left · 2.6 MiB/s ·
## 1204 of 2165 files · 2.6 GiB of 4.8 GiB". $time: like 14:20; $size: like 2.6 MiB.

progress-percent = { $percent }%
progress-left = { $time } left
progress-speed = { $size }/s
progress-files = { $done } of { $total } files
progress-bytes = { $done } of { $total }

## A transfer's line.

job-waiting-slot = Waiting for a free transfer slot
job-pausing = Pausing...
job-paused = Paused
job-paused-copied = Paused: { $done } of { $total } copied
job-preparing = Preparing...
job-deleting = { $done } of { $total } deleted
# Site to site: $subject is what is copied (a name, or "3 items"); $from and $to the
# two servers, the left pane's first with an arrow showing the way: → or ←.
job-relay-right =
    { $kind ->
        [move] Move { $subject }: { $from } → { $to }
       *[copy] Copy { $subject }: { $from } → { $to }
    }
job-relay-left =
    { $kind ->
        [move] Move { $subject }: { $to } ← { $from }
       *[copy] Copy { $subject }: { $to } ← { $from }
    }
# $title: the transfer's title, like "Upload report.pdf to /srv".
job-resumed = { $title } (resumed)
job-cancel = Cancel the whole transfer
job-cancel-paused = Cancel the paused transfer
job-pause = Pause: stop for now, keeping what is copied
job-resume = Resume: continue where it was paused
job-start-now = Start now
job-retry = Retry: continue where it stopped
job-remove = Remove from the list

## A finished transfer's line: the main part, then each count that is not zero after
## summary-separator: "3 done, 1 failed, 2 cancelled, 1 skipped".

summary-separator = {", "}
summary-failed = { $done } done, { $failed } failed
summary-cancelled-job = Cancelled: { $done } done
summary-deleted =
    { $count ->
        [one] 1 item deleted
       *[other] { $count } items deleted
    }
# $size: the total size, like 4.8 GiB.
summary-copied =
    { $count ->
        [one] 1 item · { $size }
       *[other] { $count } items · { $size }
    }
summary-cancelled = { $count } cancelled
summary-skipped = { $count } skipped
summary-stopped = { $count } stopped

## A file or folder's line inside a transfer.

item-deleted = Deleted
item-skipped = Skipped: the existing file was kept
item-paused = Paused
item-paused-slot = Paused: waiting for a free transfer slot
item-paused-part = Paused: { $done } of { $total }
item-cancelled = Cancelled
item-stopped = Stopped
item-stopped-delete = Stopped: not deleted
item-asking = Already exists: waiting for your answer
item-waiting = Waiting
item-stop-file = Stop this file; the rest goes on
item-stop-folder = Stop this folder; the rest goes on
item-pause-file = Pause this file; the rest goes on
item-pause-folder = Pause the files in this folder; the rest goes on
item-resume-file = Resume this file
item-resume-folder = Resume this folder

## The Login dialog (login_dialog.rs). $name is a site's name (its alias, or like
## me@example.com); $folder a site folder's path, like Work/Clients.

login-new-site = New Site
login-alias-optional = Optional
login-connecting = Connecting to { $name }...
login-deleted-site = (deleted site)
login-forgotten = Forgotten
login-deleting-partials = Deleting partial files...
login-save-failed = Cannot save sites: { $error }
login-folder-exists = There is already a folder "{ $folder }"
login-folder-move-failed = Cannot move "{ $folder }": { $error }

## The password field and the line under the form.

login-password-replace = Saved - type to replace
# Shown in the empty password field when a password is saved: eight dots, then.
login-password-dots = •••••••• (saved)
login-password-not-saved = Not saved
login-password-saved = Password saved in the keyring
login-password-none = No saved password
login-password-saving = Saving password to the keyring...

## A picked site's summary on the Site to site tab: how it logs in, in the order
## tried, like "key file id_ed25519, then SSH agent, then saved password".
## $encryption: how an FTP site is encrypted (in English, from the program).

login-ftp-encryption = FTP · { $encryption }
login-way-key = key file { $key }
login-way-agent = SSH agent
login-way-password = saved password
login-way-none = no saved password
login-way-then = {", then "}

## File choosers.

login-key-chooser-title = Private Key File
login-key-chooser-choose = _Choose
login-chooser-export = _Export
login-chooser-import = _Import
login-chooser-filter = Site exports (*.json)

## The Unfinished tab. $count: transfers; $subject: what a transfer copies (a name,
## or "3 items"); $place: this computer or a site's name.

login-unfinished-tab = Unfinished
login-unfinished-tab-count = Unfinished ({ $count })
login-unfinished-one-site = Tick transfers of one site (or one pair of sites) at a time.
login-unfinished-site-gone = Its site was deleted, so it cannot be resumed. Discard forgets it (delete any partial files by hand).
login-unfinished-folder = { $folder } ({ $place })
login-unfinished-what =
    { $kind ->
        [move] Move { $subject }
        [delete] Delete { $subject }
       *[copy] Copy { $subject }
    }
login-unfinished-saved-unknown = Unknown (saved by an older version)

## The Login dialog's window: the site list, the site form ("Session"), the
## three tabs and the buttons along the bottom. Labels ending in a colon name
## the field after them.

login-title = Login
login-new-folder-tooltip = New folder (in the picked folder). Drag sites and folders onto a folder to move them.
login-rename = Rename
login-edit = Edit
login-session = Session
login-alias = Alias:
login-alias-tooltip = A name for the site, shown in the site list and on the session's tab; left blank, the address is shown
login-protocol = File protocol:
login-encryption-tooltip = How the FTP connection is encrypted
login-passive = Passive mode
login-passive-tooltip = The client opens the data connections (works through firewalls and NAT). Untick for active mode.
login-host = Host name:
login-port = Port:
login-user = User name:
login-password = Password:
login-show-password = Show password
login-save-password = Save password
login-save-password-tooltip = Keep the password in the desktop keyring (encrypted with your login password)
login-key-file = Private key file:
login-key-file-placeholder = None (OpenSSH or PuTTY .ppk key)
login-browse = Browse...
login-browse-tooltip = Choose a private key file
login-use-agent = Use SSH agent
login-use-agent-tooltip = Also try the keys held by the desktop's SSH agent (as ssh does)
login-description = Description:
login-advanced = Advanced
login-files-at-once-tooltip = Files copied at the same time. More can be faster when the server limits each connection's speed; FTP opens a connection for each, and some servers allow only a few.
login-files-at-once = Files at once:
login-save = Save
login-tab-direct = Direct
login-site-to-site-intro = Copies between the two servers are transferred through this computer. Pick a saved site for each connection, then Login.
login-connection-1 = Connection 1
login-connection-1-tooltip = Shown in the left pane
login-detail-protocol = Protocol:
login-detail-address = Address:
login-detail-user = User:
login-detail-login = Login:
login-connection-2 = Connection 2
login-connection-2-tooltip = Shown in the right pane
login-tab-site-to-site = Site to site
login-unfinished-intro = Transfers that did not finish. Tick the ones to resume: { -app-name } logs in to their site with its saved password or keys and opens it where they were. Discard deletes their partial files instead.
login-detail-transfer = Transfer:
login-detail-from = From:
login-detail-to = To:
login-detail-saved = Saved:
login-discard-tooltip = Log in, delete the ticked transfers' partial files, and forget them
login-resume-tooltip = Log in and continue the ticked transfers where they stopped
login-show-at-startup = Show at startup
login-show-at-startup-tooltip = Open this dialog when { -app-name } starts
login-import = Import...
login-import-tooltip = Add sites from a { -app-name } export file
login-export = Export...
login-export-tooltip = Save all sites to a file for another computer
login-login-tooltip = Connect to the selected site
# Shown after the Files at once field; $count is the usual number.
login-files-at-once-usual = (usually { $count })
login-verify = Verify checksums:
login-verify-tooltip = Whether file checksums are verified after each transfer to or from this server. Server checksums only: only where the server can compute them, never by reading the file back (best for slow servers).
login-verify-same = Same as Preferences
login-verify-checksums = Server checksums only
login-verify-off = Off
preferences-verify = _Verify file checksums after completion
preferences-read-back = _Read back files up to:
preferences-read-back-note = When a server cannot compute checksums, the file is read back to verify it: a second transfer.
read-back-never = Never
read-back-any-size = Any size

## The Login dialog's prompts and results. $count: a number of sites, transfers or
## passwords as the message says; $host: a server's address; $error, $reason: what
## went wrong (in English, from the program).

login-unfinished-none = No unfinished transfers
login-unfinished-notice =
    { $count ->
        [one] 1 transfer did not finish: see the Unfinished tab
       *[other] { $count } transfers did not finish: see the Unfinished tab
    }
login-unfinished-tick-tooltip = Resume or discard this transfer
login-folder-open = Open folder
login-folder-close = Close folder
login-password-saved-tooltip = Password saved
login-rename-folder = Rename Folder
login-rename-folder-prompt = New name for the folder { $folder }:
login-new-folder-prompt = Name of the new folder:
login-new-folder-prompt-in = Name of the new folder in { $folder }:
login-delete-folder = Delete Folder
login-delete-folder-question = Delete the folder { $folder }?
login-delete-folder-to-top = The sites and folders in it move to the top of the list. No site is deleted.
login-delete-folder-to-parent = The sites and folders in it move to { $folder }. No site is deleted.
login-delete-site = Delete Site
login-delete-site-question = Delete { $name }?
login-delete-site-with-password = The site and its saved password (in the keyring) will be removed. This cannot be undone.
login-delete-site-detail = The site will be removed. This cannot be undone.
login-exporting = Exporting...
login-importing = Importing...
# $file: where the export was written.
login-exported =
    { $count ->
        [one] Exported 1 site ({ $passwords } with passwords) to { $file }
       *[other] Exported { $count } sites ({ $passwords } with passwords) to { $file }
    }
login-exported-unreadable = ; { $count } passwords could not be read
login-export-failed = Export failed: { $error }
login-imported =
    { $count ->
        [one] Imported 1 site ({ $added } new, { $replaced } updated, { $passwords } with passwords)
       *[other] Imported { $count } sites ({ $added } new, { $replaced } updated, { $passwords } with passwords)
    }
login-imported-unstored = ; { $count } passwords could not be stored
login-import-failed = Import failed: { $error }
login-not-discarded = Not discarded: { $reason }
# $deleted: how many partial files were deleted.
login-discarded =
    { $count ->
        [one] Discarded 1 transfer: { $deleted ->
            [one] 1 partial file deleted
           *[other] { $deleted } partial files deleted
        }
       *[other] Discarded { $count } transfers: { $deleted ->
            [one] 1 partial file deleted
           *[other] { $deleted } partial files deleted
        }
    }
login-password-removed = Saved password removed
login-keyring-error = Keyring: { $error }
login-password-kept = Password not saved ({ $error }); it stays in the field for this login
login-pair-needs-password = { $name } needs a password: save it on the Direct tab, or use SSH keys

## Checking a server's identity on the first connection. $fingerprint: the key's or
## certificate's fingerprint, shown alone on its line; keep the blank lines.

login-waiting-key = Waiting for you to check the server's key
login-waiting-certificate = Waiting for you to check the server's certificate
login-waiting-passphrase = Waiting for the key's passphrase
login-trust = Trust and Connect
login-trust-key = Unknown Server
login-trust-key-question = Trust the server { $host }?
login-trust-key-detail =
    This is the first connection to this server, so its identity cannot be checked yet. Its host key fingerprint is:

    { $fingerprint }

    If it matches the server's real key, trust it: it will be remembered in ~/.ssh/known_hosts.
login-trust-certificate = Untrusted Certificate
login-trust-certificate-question = Trust the certificate of { $host }?
login-trust-certificate-detail =
    The server's TLS certificate cannot be verified: { $reason }. FTP servers often use a self-signed certificate. Its fingerprint is:

    { $fingerprint }

    If it matches the server's real certificate, trust it: it will be remembered for this server.

## Sizes (format.rs), like "4.2 MiB": $size is the number, already written with
## size-decimal-mark; $unit picks the unit (binary multiples of 1024). The unit
## names are the usual ones in the language (French writes Kio, Mio, Gio...).

size-decimal-mark = .
size =
    { $unit ->
        [b] { $size } B
        [kib] { $size } KiB
        [mib] { $size } MiB
        [gib] { $size } GiB
        [tib] { $size } TiB
        [pib] { $size } PiB
       *[eib] { $size } EiB
    }

## Log levels (Tools › Log Level, and the status bar).

log-level-error = Error
log-level-warn = Warning
log-level-info = Info
log-level-debug = Debug
log-level-trace = Trace

## How an FTP site is encrypted (the Login dialog's list).

ftp-encryption-explicit-if-available = Use explicit FTP over TLS if available
ftp-encryption-explicit = Require explicit FTP over TLS
ftp-encryption-implicit = Require implicit FTP over TLS
ftp-encryption-none = Only use plain FTP (insecure)

## A transfer's title (transfer/mod.rs). $what is what it copies (a file or folder
## name, or job-items); $dest the folder it goes to. $action says which kind:
## upload and download between this computer and the server; move-to-server and
## move-to-computer move them across; copy/move-here and copy/move-on-server stay on
## one side; copy/move-other-server go between two servers (site to site).

job-title =
    { $action ->
        [upload] Upload { $what } to { $dest }
        [download] Download { $what } to { $dest }
        [move-to-server] Move { $what } to the server, { $dest }
        [move-to-computer] Move { $what } to this computer, { $dest }
        [copy-here] Copy { $what } to { $dest }
        [copy-on-server] Copy { $what } to { $dest } on the server
        [move-here] Move { $what } to { $dest }
        [move-on-server] Move { $what } to { $dest } on the server
        [copy-other-server] Copy { $what } to { $dest } on the other server
        [move-other-server] Move { $what } to { $dest } on the other server
        [delete-here] Delete { $what } from this computer
       *[delete-on-server] Delete { $what } from the server
    }
job-items =
    { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

## Errors from file operations (the core library). $path is a file or folder; $reason
## why it failed, as the system or server said it (often in English); $from and $to a
## renamed item's old and new path. The message starts lowercase: it often follows
## other text, as in "Failed: cannot read ...".

error-cannot =
    { $action ->
        [read] cannot read { $path }: { $reason }
        [open] cannot open { $path }: { $reason }
        [write] cannot write { $path }: { $reason }
        [create] cannot create { $path }: { $reason }
        [create-folder] cannot create folder { $path }: { $reason }
        [delete] cannot delete { $path }: { $reason }
        [delete-folder] cannot delete folder { $path }: { $reason }
        [set-time] cannot set the time of { $path }: { $reason }
        [finish] cannot finish { $path }: { $reason }
        [download] cannot download { $path }: { $reason }
        [replace] cannot replace { $path }: { $reason }
        [access] cannot access { $path }: { $reason }
       *[make-runnable] cannot make { $path } runnable: { $reason }
    }
error-cannot-bare =
    { $action ->
        [read] cannot read { $path }
        [open] cannot open { $path }
        [write] cannot write { $path }
        [create] cannot create { $path }
        [create-folder] cannot create folder { $path }
        [delete] cannot delete { $path }
        [delete-folder] cannot delete folder { $path }
        [set-time] cannot set the time of { $path }
        [finish] cannot finish { $path }
        [download] cannot download { $path }
        [replace] cannot replace { $path }
        [access] cannot access { $path }
       *[make-runnable] cannot make { $path } runnable
    }
error-cannot-rename = cannot rename { $from } to { $to }: { $reason }
error-cannot-rename-bare = cannot rename { $from } to { $to }
error-no-such-item = no such file or folder
# $path: a file whose date is before 1970 or after 2106.
error-sftp-date = { $path } has a date SFTP cannot store
checksum-unsupported = the server cannot compute checksums
checksum-no-answer = the server's answer held no checksum
verify-mismatch = the copy's checksum differs from the original's: the copy is not what was copied
verify-skipped = skipped by the user
item-verifying = Verifying…
item-verified = { $size } · verified
item-not-verified = { $size } · not verified
item-not-verified-large = { $size } · not verified (large file)
item-skip-check = Skip the check
item-verify-now = Verify now (reads the file back)

## Why connecting failed (remote/mod.rs). Shown after the site's name or on their
## own, so they start lowercase. $host: a server's address; $reason: what went wrong
## (often from the server, in English); $path: a key file.

connect-unsupported = { $protocol } connections are not supported yet
connect-timeout = { $host } did not respond (timed out)
connect-network = cannot connect to { $host }: { $reason }
connect-unknown-host-key = the server's host key is not known yet ({ $fingerprint })
# $line: the line of ~/.ssh/known_hosts holding the old key.
connect-host-key-changed = WARNING: the host key of { $host } has changed (known_hosts line { $line }). Someone could be intercepting the connection; refusing to connect.
connect-untrusted-certificate = the server's certificate is not trusted ({ $reason })
connect-password-required = enter the password
connect-keys-rejected = the server did not accept the SSH key; enter the password, or check the key
connect-key-passphrase-required = the key file { $path } needs its passphrase
connect-key-unreadable = cannot use the key file { $path }: { $reason }
connect-authentication-failed = login failed: wrong user name, key or password
connect-sftp = the server's file transfer (SFTP) service failed: { $reason }
connect-ftp = FTP: { $reason }
connect-host-certificates = host certificates are not supported
connect-ftp-home = cannot read the home folder: { $reason }
# $reply: the server's answer; $choice: the name of the encryption choice to pick
# instead (ftp-encryption-explicit-if-available).
connect-ftp-no-tls = the server does not offer TLS encryption ({ $reply }); choose "{ $choice }" to connect without requiring it
connect-tls-setup = TLS setup failed: { $reason }
connect-no-authorities = this system has no trusted certificate authorities
# Why a server's TLS certificate is not trusted; shown in the dialog asking whether to
# trust it, after "cannot be verified:".
certificate-problem =
    { $reason ->
        [unknown-issuer] it is not signed by an authority this computer trusts
        [expired] it has expired
        [not-valid-yet] it is not valid yet
        [other-name] it was issued for a different server name
        [revoked] it has been revoked
       *[self-signed] it is self-signed
    }

## Name rules, shown under the name field (New Folder, Rename). $character: a
## character a name cannot contain; $name: the name typed.

name-empty = enter a name
name-reserved = "{ $name }" cannot be used as a name
name-forbidden-character = a name cannot contain "{ $character }"
name-null = a name cannot contain a null character
name-control-characters = a name cannot contain control characters
name-trailing-space-or-dot = a name cannot end with a space or a dot
name-windows-device = "{ $name }" is a name Windows keeps for a device

## Transfer and file errors (transfer/). $name: a file or folder name; $folder and
## $path: paths; $reason: what went wrong (often from the system or server).

error-already-exists = { $name } already exists
error-into-itself = { $name } cannot go inside itself
error-already-there = { $name } is already in { $folder }
error-parent-not-created = the folder it goes in could not be created
error-folder-there = a folder named { $name } is already there
error-items-inside-failed = some items inside it failed
error-items-inside-not-deleted = some items inside it were not deleted
# The file was copied, but deleting the original (a move) failed.
error-copied-but = copied, but { $reason }
error-not-connected = not connected to a server
error-not-connected-destination = not connected to the server copies go to
error-cannot-copy-root = { $path } cannot be copied or deleted
error-nested-too-deeply = { $path } is nested too deeply (a symbolic link loop?)

## Update errors (update.rs), shown in the update dialog. $reason: what went wrong
## (often from the network, in English).

update-error-unsigned = the update is not signed by the { -app-name } release key
update-error-unreadable = the update description cannot be read: { $reason }
update-error-unexpected-place = the update would be downloaded from an unexpected place: { $url }
update-error-cancelled = the download was cancelled
update-error-failed = the download failed: { $reason }
update-error-too-large = the download is larger than the release says
update-error-save = cannot save the download: { $reason }
# $done, $total: numbers of bytes.
update-error-incomplete = the download is incomplete ({ $done } of { $total } bytes)
update-error-mismatch = the download does not match the signed release (SHA-256 differs)
update-error-no-key = this build cannot check for updates
update-error-bad-key = the built-in release key is damaged: { $reason }
update-error-unreachable = cannot reach the releases page: { $reason }
update-error-description = cannot read the release description: { $reason }
update-error-version = "{ $version }" is not a version number

## The passphrase rules in the export and import dialog (shown under the fields).

passphrase-problem-empty = Enter the passphrase
# $count: the fewest characters allowed (8).
passphrase-problem-short = Use at least { $count } characters
passphrase-problem-mismatch = Passphrases do not match

## Site export and import errors (site_export.rs).

export-error-json = { $path } is not a valid site export: { $reason }
export-error-not-export = the file is not a { -app-name } site export
export-error-version = unsupported export version { $version }
export-error-passphrase-required = a passphrase is needed to import the saved passwords
export-error-wrong-passphrase = wrong passphrase, or the file has been altered
export-error-settings = invalid encryption settings in the file: { $reason }
export-error-random = cannot generate random data: { $reason }

## The desktop keyring (secrets.rs), settings and site folders.

keyring-error-service = keyring error: { $reason }
keyring-error-not-text = stored password is not valid text
keyring-error-unsupported = saving passwords is not supported on this platform yet
settings-error-invalid = invalid settings in { $path }: { $reason }
folder-error-into-itself = a folder cannot go inside itself
folder-error-exists = there is already a folder "{ $folder }"

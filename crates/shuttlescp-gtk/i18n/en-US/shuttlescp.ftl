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

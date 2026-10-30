ShuttleSCP privacy policy
=========================

ShuttleSCP collects no information about you or how you use it. It has no telemetry,
no usage statistics and no crash reporting.

What it connects to
-------------------

- The servers you add yourself, to log in and copy files. Nothing else is sent to them
  beyond what logging in and copying needs.
- GitHub, to look for new versions: once a day when ShuttleSCP starts, and when you
  choose Help > Check for Updates. It downloads two small files from the latest
  ShuttleSCP release (latest.json and its signature). GitHub sees your IP address and
  the ShuttleSCP version you run, as with any download; GitHub's privacy statement
  applies: https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement
  A new version is only downloaded when you agree to update.

To stop the daily check, untick Help > Check for Updates at Startup. ShuttleSCP then
contacts GitHub only when you choose Help > Check for Updates.

What stays on your computer
---------------------------

Your saved sites, settings, logs and saved folder listings are kept in your user
folders and are never sent anywhere. Saved passwords and key passphrases are kept in
your system's password store (GNOME Keyring or KWallet on Linux, the Keychain on macOS,
the Credential Manager on Windows). A site export file holds passwords only when you
choose, encrypted with a passphrase you pick.

The web page
------------

The ShuttleSCP web page (https://shuttlescp.dsalvado.ca) counts visits with Umami on
the author's own server, without cookies. The program itself does not.

Questions
---------

Open an issue at https://github.com/dsalvado/shuttlescp/issues, or report anything
sensitive privately as described in SECURITY.md.

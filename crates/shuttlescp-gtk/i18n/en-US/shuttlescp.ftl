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

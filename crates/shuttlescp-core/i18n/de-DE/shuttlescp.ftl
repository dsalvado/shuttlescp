# Die Texte der ShuttleSCP-Oberfläche auf Deutsch. Maschinelle Übersetzung (KI), noch
# nicht von einer deutschsprachigen Person geprüft: Korrekturen sind willkommen (siehe
# README). Dieselben Nachrichtennamen wie i18n/en-US/shuttlescp.ftl.
#
# Fluent-Syntax: https://projectfluent.org/fluent/guide/

-app-name = ShuttleSCP

language-name = Deutsch (maschinelle Übersetzung)

dialog-close = Schließen

preferences-title = Einstellungen
preferences-language = _Sprache der Oberfläche:
preferences-restart = Starten Sie { -app-name } neu, um die neue Sprache zu verwenden.

status-transfers-unlimited = Übertragungen: unbegrenzt
status-transfers-limited = Übertragungen: { $running } von { $limit }
status-transfers-waiting = { $count } wartend
dialog-cancel = Abbrechen

quit-title = Beenden
quit-question = { -app-name } beenden?
quit-detail = { $unfinished } Behalten Sie sie, um sie bei der nächsten Verbindung fortzusetzen, oder löschen Sie ihre Teildateien.
quit-running =
    { $count ->
        [one] { $count } Übertragung läuft noch.
       *[other] { $count } Übertragungen laufen noch.
    }
quit-paused =
    { $count ->
        [one] { $count } Übertragung ist angehalten.
       *[other] { $count } Übertragungen sind angehalten.
    }
quit-unfinished = { $count } Übertragungen sind nicht abgeschlossen ({ $running } laufend, { $paused } angehalten).
quit-discard = Teildateien löschen und beenden
quit-keep = Für später behalten und beenden

passphrase-export-title = Sites exportieren
passphrase-import-title = Sites importieren
passphrase-export-explain = Gespeicherte Passwörter werden mit einer Passphrase Ihrer Wahl verschlüsselt. Sie benötigen sie, um die Datei auf einem anderen Computer zu importieren.
passphrase-import-explain = Diese Datei enthält verschlüsselte Passwörter. Geben Sie die Passphrase ein, mit der sie exportiert wurde.
passphrase-include = Gespeicherte Passwörter einschließen
passphrase-new = Passphrase (mindestens 8 Zeichen)
passphrase-field = Passphrase
passphrase-repeat = Passphrase wiederholen
passphrase-export = Exportieren
passphrase-import = Importieren

key-passphrase-title = Passphrase des Schlüssels
key-passphrase-question = Der Schlüssel { $key } ist durch eine Passphrase geschützt
key-passphrase-wrong = Diese Passphrase öffnet den Schlüssel nicht. Bitte versuchen Sie es erneut.
key-passphrase-explain = Geben Sie seine Passphrase ein, um sich damit anzumelden.
key-passphrase-save = Passphrase im Schlüsselbund speichern
key-passphrase-unlock = Entsperren

conflict-title = Datei existiert bereits
conflict-question = { $name } existiert bereits in { $folder }
conflict-existing = Vorhanden:
conflict-new = Neu:
conflict-for-all = Für die anderen vorhandenen Dateien dasselbe tun
conflict-cancel = Übertragung abbrechen
conflict-skip = Überspringen
conflict-overwrite = Überschreiben
conflict-newer = {" "}(neuer)
key-passphrase-save-tooltip = Im Schlüsselbund des Desktops aufbewahren (mit Ihrem Anmeldepasswort verschlüsselt), damit sich der Schlüssel ohne Nachfrage öffnet

update-restart-title = Neu starten
update-restart-question = Mit { -app-name } { $version } neu starten?
update-restart-running =
    { $count ->
        [one] { $count } Übertragung läuft noch.
       *[other] { $count } Übertragungen laufen noch.
    }
update-restart-detail = { $running } { -app-name } kann sich selbst neu starten, sobald sie fertig sind, oder jetzt: Sie werden angehalten und können bei der nächsten Verbindung zum Server fortgesetzt werden.
update-later = Später
update-restart-now = Jetzt neu starten
update-when-finished = Wenn Übertragungen fertig sind
update-check-title = Nach Aktualisierungen suchen
update-up-to-date = { -app-name } ist auf dem neuesten Stand
update-latest = Version { $version } ist die neueste Version.
update-check-failed = Suche nach Aktualisierungen nicht möglich
update-check-error = { $error }.
update-available-title = Aktualisierung verfügbar
update-available = { -app-name } { $version } ist verfügbar
update-you-have = Sie haben Version { $current }. Die neue Version erschien am { $date }.
update-whats-new = Neu in Version { $version }
update-mac-steps = Laden Sie sie herunter, öffnen Sie das Disk-Image und ziehen Sie { -app-name } in „Programme“, um diese Version zu ersetzen.
update-download = Herunterladen
update-from-page = Laden Sie die neue Version von der Webseite von { -app-name } herunter.
update-open-page = Download-Seite öffnen
update-downloading = Wird heruntergeladen...
update-install = Installieren und neu starten
update-downloading-progress = Wird heruntergeladen... { $done } von { $total }
update-failed = { $error }. Sie können sie stattdessen von der Webseite von { -app-name } herunterladen.

menu-file = _Datei
menu-edit = _Bearbeiten
menu-view = _Ansicht
menu-tools = _Werkzeuge
menu-window = _Fenster
menu-help = _Hilfe
menu-login = _Anmelden…
menu-disconnect = _Trennen
menu-preferences = _Einstellungen…
menu-settings = Ein_stellungen…
menu-quit = _Beenden
menu-theme = Thema
menu-log-level = _Protokollstufe
menu-open-log = Protokoll_datei öffnen
menu-check-updates = Nach _Aktualisierungen suchen…
menu-auto-updates = Beim _Start suchen
menu-about = _Über { -app-name }

edit-undo = Rückgängig
edit-redo = Wiederherstellen
edit-cut = Ausschneiden
edit-copy = Kopieren
edit-paste = Einfügen
edit-delete = Löschen
edit-select-all = Alles auswählen

not-connected = Nicht verbunden
new-connection = Neue Verbindung
new-connection-tooltip = In einem neuen Tab mit einem anderen Server verbinden
this-computer-title = Dieser Computer · { $name }
this-computer = diesen Computer
status-connected = Verbunden mit { $name }
status-connected-pair = Verbunden mit { $left } und { $right }
status-transfers-tooltip = Gleichzeitige Übertragungen über alle Verbindungen. Klicken, um sie zu begrenzen.
status-log-level = Protokollstufe: { $level }
session-activity = Übertragungen laufen
session-close = Trennen und diese Sitzung schließen
close-session-title = Sitzung schließen
close-session-question = Die Sitzung schließen?
stop-transfers-detail =
    { $count ->
        [one] { $count } Übertragung läuft noch. Sie wird angehalten und kann bei der nächsten Verbindung zu diesem Server fortgesetzt werden.
       *[other] { $count } Übertragungen laufen noch. Sie werden angehalten und können bei der nächsten Verbindung zu diesem Server fortgesetzt werden.
    }
delete-title = Löschen
delete-detail = Ordner werden mit ihrem gesamten Inhalt gelöscht. Dies kann nicht rückgängig gemacht werden.

update-notice-available = Aktualisierung verfügbar: { $version }
update-notice-waiting = Aktualisierung wartet auf Übertragungen
update-notice-restart = Neu starten, um die Aktualisierung abzuschließen
update-notice-available-tooltip = { -app-name } { $version } ist verfügbar (Sie haben { $current }). Klicken, um sie anzusehen.
update-notice-waiting-tooltip = { -app-name } startet mit { $version } neu, sobald die laufenden Übertragungen fertig sind. Klicken, um jetzt oder später neu zu starten.
update-notice-restart-tooltip = { -app-name } { $version } ist bereit. Klicken, um damit neu zu starten.
update-notice-hide = Diesen Hinweis ausblenden

limit-title = Gleichzeitige Übertragungen
limit-unlimited = Unbegrenzt
limit-at-most = Höchstens
limit-hint = Über alle Verbindungen. Weitere warten in der Warteschlange und starten, sobald andere fertig sind.

relay-tooltip = Zwischen den beiden Servern kopierte Dateien laufen über diesen Computer
relay-to-right = Kopieren vom linken zum rechten Server
relay-to-left = Kopieren vom rechten zum linken Server
relay-receiving = Empfangen vom Server, von dem kopiert wird
relay-sending = Senden an den Server, auf den kopiert wird

panel-log = Protokoll
list-queue = Warteschlange
list-completed = Abgeschlossen
list-failed = Fehlgeschlagen
list-queue-tooltip = Übertragungen
list-completed-tooltip = Abgeschlossene Übertragungen
list-failed-tooltip = Fehlgeschlagene Übertragungen
list-for-connection = { $list } für { $name }
list-not-connected = { $list } (nicht verbunden)
log-unavailable = Protokollierung in eine Datei ist nicht verfügbar, daher gibt es nichts anzuzeigen.

theme-system = Systemstandard
theme-swatch = Fenster- und Akzentfarben
disk-tooltip = Laufwerk

about-title = Über { -app-name }
about-version = Version { $version } ({ $revision })
about-description = Ein grafischer SFTP-, SCP- und FTP-Client für Linux, macOS und Windows, inspiriert von WinSCP.
about-authors = Von { $authors }
about-license = Freie Software: { $license }
about-system-details = Systemdetails
about-copy-details = Details kopieren

pane-gathering = Informationen werden gesammelt...
pane-gathering-tooltip = Informationen werden gesammelt
pane-saved-gathering = Gespeicherte Liste vom { $saved } · Informationen werden gesammelt...
pane-saved-stale = Gespeicherte Liste vom { $saved }; Änderungen konnten nicht geprüft werden: { $error }
pane-refresh-tooltip = Aktualisieren
pane-items =
    { $count ->
        [one] { $count } Element
       *[other] { $count } Elemente
    }
pane-created = Ordner { $name } erstellt
pane-renamed = Umbenannt in { $name }
pane-new-folder-title = Neuer Ordner
pane-new-folder-prompt = Einen Ordner erstellen in { $folder }:
pane-new-folder-create = Erstellen
pane-new-folder-name = Neuer Ordner
pane-rename-title = Umbenennen
pane-rename-prompt = Neuer Name für { $name }:
pane-open-title = Datei öffnen
pane-open-failed = { $name } kann nicht geöffnet werden
pane-open-error = { $error }.
pane-column-name = Name
pane-column-size = Größe
pane-column-modified = Geändert

pane-transfer =
    { $kind ->
        [upload] _Hochladen
        [download] _Herunterladen
       *[copy] _Kopieren
    }
pane-transfer-to =
    { $kind ->
        [upload] _Hochladen nach { $name }
        [download] _Herunterladen auf { $name }
       *[copy] _Kopieren nach { $name }
    }
pane-move = _Verschieben
pane-move-to = _Verschieben nach { $name }
pane-menu-new-folder = _Neuer Ordner…
pane-menu-rename = _Umbenennen…
pane-menu-delete = _Löschen
pane-menu-refresh = _Aktualisieren

pane-drop-copy = Hierher _kopieren
pane-drop-move = Hierher _verschieben
pane-drop-cancel = _Abbrechen

transfers-resume = Fortsetzen
transfers-discard = Verwerfen
transfers-discard-tooltip = Sie vergessen; ihre Teildateien bleiben, wo sie sind
transfers-hide-finished = Fertige Dateien ausblenden
transfers-hide-finished-tooltip = Ohne Probleme kopierte Dateien aus der Liste nehmen, damit die gerade kopierten oben bleiben
transfers-queue-empty = Warteschlange leer
transfers-completed-empty = Keine abgeschlossenen Übertragungen
transfers-failed-empty = Keine fehlgeschlagenen Übertragungen
transfers-clear = Leeren
transfers-interrupted =
    { $count ->
        [one] { $count } Übertragung wurde unterbrochen und kann fortgesetzt werden.
       *[other] { $count } Übertragungen wurden unterbrochen und können fortgesetzt werden.
    }
transfers-blocked = Nicht gestartet: „{ $other }“ verwendet dieselben Dateien. Erneut versuchen, sobald sie fertig ist.

progress-percent = { $percent } %
progress-left = noch { $time }
progress-speed = { $size }/s
progress-files = { $done } von { $total } Dateien
progress-bytes = { $done } von { $total }

job-waiting-slot = Wartet auf einen freien Übertragungsplatz
job-pausing = Wird angehalten...
job-paused = Angehalten
job-paused-copied = Angehalten: { $done } von { $total } kopiert
job-preparing = Wird vorbereitet...
job-deleting = { $done } von { $total } gelöscht
job-relay-right =
    { $kind ->
        [move] { $subject } verschieben: { $from } → { $to }
       *[copy] { $subject } kopieren: { $from } → { $to }
    }
job-relay-left =
    { $kind ->
        [move] { $subject } verschieben: { $to } ← { $from }
       *[copy] { $subject } kopieren: { $to } ← { $from }
    }
job-resumed = { $title } (fortgesetzt)
job-cancel = Die ganze Übertragung abbrechen
job-cancel-paused = Die angehaltene Übertragung abbrechen
job-pause = Anhalten: vorerst stoppen und Kopiertes behalten
job-resume = Fortsetzen: dort weitermachen, wo angehalten wurde
job-retry = Wiederholen: dort weitermachen, wo sie stoppte
job-remove = Aus der Liste entfernen

summary-separator = {", "}
summary-failed = { $done } erledigt, { $failed } fehlgeschlagen
summary-cancelled-job = Abgebrochen: { $done } erledigt
summary-deleted =
    { $count ->
        [one] { $count } Element gelöscht
       *[other] { $count } Elemente gelöscht
    }
summary-copied =
    { $count ->
        [one] { $count } Element · { $size }
       *[other] { $count } Elemente · { $size }
    }
summary-cancelled = { $count } abgebrochen
summary-skipped = { $count } übersprungen
summary-stopped = { $count } gestoppt

item-deleted = Gelöscht
item-skipped = Übersprungen: Die vorhandene Datei wurde behalten
item-paused = Angehalten
item-paused-slot = Angehalten: wartet auf einen freien Übertragungsplatz
item-paused-part = Angehalten: { $done } von { $total }
item-cancelled = Abgebrochen
item-stopped = Gestoppt
item-stopped-delete = Gestoppt: nicht gelöscht
item-asking = Existiert bereits: wartet auf Ihre Antwort
item-waiting = Wartend
item-stop-file = Diese Datei stoppen; der Rest läuft weiter
item-stop-folder = Diesen Ordner stoppen; der Rest läuft weiter
item-pause-file = Diese Datei anhalten; der Rest läuft weiter
item-pause-folder = Die Dateien in diesem Ordner anhalten; der Rest läuft weiter
item-resume-file = Diese Datei fortsetzen
item-resume-folder = Diesen Ordner fortsetzen

login-new-site = Neue Site
login-alias-optional = Optional
login-connecting = Verbindung zu { $name } wird hergestellt...
login-deleted-site = (gelöschte Site)
login-forgotten = Vergessen
login-deleting-partials = Teildateien werden gelöscht...
login-save-failed = Sites können nicht gespeichert werden: { $error }
login-folder-exists = Es gibt bereits einen Ordner „{ $folder }“
login-folder-move-failed = „{ $folder }“ kann nicht verschoben werden: { $error }

login-password-replace = Gespeichert – zum Ersetzen tippen
login-password-dots = •••••••• (gespeichert)
login-password-not-saved = Nicht gespeichert
login-password-saved = Passwort im Schlüsselbund gespeichert
login-password-none = Kein gespeichertes Passwort
login-password-saving = Passwort wird im Schlüsselbund gespeichert...

login-ftp-encryption = FTP · { $encryption }
login-way-key = Schlüsseldatei { $key }
login-way-agent = SSH-Agent
login-way-password = gespeichertes Passwort
login-way-none = kein gespeichertes Passwort
login-way-then = {", dann "}

login-key-chooser-title = Private Schlüsseldatei
login-key-chooser-choose = _Auswählen
login-chooser-export = _Exportieren
login-chooser-import = _Importieren
login-chooser-cancel = Abb_rechen
login-chooser-filter = Site-Exporte (*.json)

login-unfinished-tab = Unvollendet
login-unfinished-tab-count = Unvollendet ({ $count })
login-unfinished-one-site = Markieren Sie jeweils nur Übertragungen einer Site (oder eines Site-Paars).
login-unfinished-site-gone = Ihre Site wurde gelöscht, daher kann sie nicht fortgesetzt werden. Verwerfen vergisst sie (löschen Sie Teildateien von Hand).
login-unfinished-folder = { $folder } ({ $place })
login-unfinished-what =
    { $kind ->
        [move] { $subject } verschieben
        [delete] { $subject } löschen
       *[copy] { $subject } kopieren
    }
login-unfinished-saved-unknown = Unbekannt (von einer älteren Version gespeichert)

login-title = Anmelden
login-new-folder-tooltip = Neuer Ordner (im gewählten Ordner). Ziehen Sie Sites und Ordner auf einen Ordner, um sie dorthin zu verschieben.
login-rename = Umbenennen
login-edit = Bearbeiten
login-session = Sitzung
login-alias = Alias:
login-alias-tooltip = Ein Name für die Site, angezeigt in der Site-Liste und auf dem Tab der Sitzung; leer gelassen, wird die Adresse angezeigt
login-protocol = Dateiprotokoll:
login-encryption-tooltip = Wie die FTP-Verbindung verschlüsselt wird
login-passive = Passiver Modus
login-passive-tooltip = Der Client öffnet die Datenverbindungen (funktioniert durch Firewalls und NAT). Für den aktiven Modus abwählen.
login-host = Hostname:
login-port = Port:
login-user = Benutzername:
login-password = Passwort:
login-show-password = Passwort anzeigen
login-save-password = Passwort speichern
login-save-password-tooltip = Das Passwort im Schlüsselbund des Desktops aufbewahren (mit Ihrem Anmeldepasswort verschlüsselt)
login-key-file = Private Schlüsseldatei:
login-key-file-placeholder = Keine (OpenSSH- oder PuTTY-.ppk-Schlüssel)
login-browse = Durchsuchen...
login-browse-tooltip = Eine private Schlüsseldatei wählen
login-use-agent = SSH-Agent verwenden
login-use-agent-tooltip = Auch die Schlüssel des SSH-Agenten des Desktops probieren (wie ssh)
login-description = Beschreibung:
login-advanced = Erweitert
login-files-at-once-tooltip = Gleichzeitig kopierte Dateien. Mehr kann schneller sein, wenn der Server die Geschwindigkeit jeder Verbindung begrenzt; FTP öffnet für jede eine Verbindung, und manche Server erlauben nur wenige.
login-files-at-once = Dateien gleichzeitig:
login-save = Speichern
login-tab-direct = Direkt
login-site-to-site-intro = Kopien zwischen den beiden Servern laufen über diesen Computer. Wählen Sie für jede Verbindung eine gespeicherte Site und dann Anmelden.
login-connection-1 = Verbindung 1
login-connection-1-tooltip = Im linken Bereich angezeigt
login-detail-protocol = Protokoll:
login-detail-address = Adresse:
login-detail-user = Benutzer:
login-detail-login = Anmeldung:
login-connection-2 = Verbindung 2
login-connection-2-tooltip = Im rechten Bereich angezeigt
login-tab-site-to-site = Site zu Site
login-unfinished-intro = Übertragungen, die nicht fertig wurden. Markieren Sie die fortzusetzenden: { -app-name } meldet sich mit gespeichertem Passwort oder Schlüsseln bei ihrer Site an und öffnet sie dort, wo sie waren. Verwerfen löscht stattdessen ihre Teildateien.
login-detail-transfer = Übertragung:
login-detail-from = Von:
login-detail-to = Nach:
login-detail-saved = Gespeichert:
login-discard-tooltip = Anmelden, die Teildateien der markierten Übertragungen löschen und sie vergessen
login-resume-tooltip = Anmelden und die markierten Übertragungen dort fortsetzen, wo sie stoppten
login-show-at-startup = Beim Start anzeigen
login-show-at-startup-tooltip = Dieses Fenster beim Start von { -app-name } öffnen
login-import = Importieren...
login-import-tooltip = Sites aus einer { -app-name }-Exportdatei hinzufügen
login-export = Exportieren...
login-export-tooltip = Alle Sites für einen anderen Computer in einer Datei speichern
login-login-tooltip = Mit der gewählten Site verbinden
login-files-at-once-usual = (normalerweise { $count })

login-unfinished-none = Keine unvollendeten Übertragungen
login-unfinished-notice =
    { $count ->
        [one] { $count } Übertragung wurde nicht fertig: siehe Tab „Unvollendet“
       *[other] { $count } Übertragungen wurden nicht fertig: siehe Tab „Unvollendet“
    }
login-unfinished-tick-tooltip = Diese Übertragung fortsetzen oder verwerfen
login-folder-open = Ordner öffnen
login-folder-close = Ordner schließen
login-password-saved-tooltip = Passwort gespeichert
login-rename-folder = Ordner umbenennen
login-rename-folder-prompt = Neuer Name für den Ordner { $folder }:
login-new-folder-prompt = Name des neuen Ordners:
login-new-folder-prompt-in = Name des neuen Ordners in { $folder }:
login-delete-folder = Ordner löschen
login-delete-folder-question = Den Ordner { $folder } löschen?
login-delete-folder-to-top = Die Sites und Ordner darin wandern an den Anfang der Liste. Keine Site wird gelöscht.
login-delete-folder-to-parent = Die Sites und Ordner darin wandern nach { $folder }. Keine Site wird gelöscht.
login-delete-site = Site löschen
login-delete-site-question = { $name } löschen?
login-delete-site-with-password = Die Site und ihr gespeichertes Passwort (im Schlüsselbund) werden entfernt. Dies kann nicht rückgängig gemacht werden.
login-delete-site-detail = Die Site wird entfernt. Dies kann nicht rückgängig gemacht werden.
login-exporting = Wird exportiert...
login-importing = Wird importiert...
login-exported =
    { $count ->
        [one] { $count } Site ({ $passwords } mit Passwort) exportiert nach { $file }
       *[other] { $count } Sites ({ $passwords } mit Passwort) exportiert nach { $file }
    }
login-exported-unreadable = ; { $count } Passwörter konnten nicht gelesen werden
login-export-failed = Export fehlgeschlagen: { $error }
login-imported =
    { $count ->
        [one] { $count } Site importiert ({ $added } neu, { $replaced } aktualisiert, { $passwords } mit Passwort)
       *[other] { $count } Sites importiert ({ $added } neu, { $replaced } aktualisiert, { $passwords } mit Passwort)
    }
login-imported-unstored = ; { $count } Passwörter konnten nicht gespeichert werden
login-import-failed = Import fehlgeschlagen: { $error }
login-not-discarded = Nicht verworfen: { $reason }
login-discarded =
    { $count ->
        [one] { $count } Übertragung verworfen: { $deleted ->
            [one] { $deleted } Teildatei gelöscht
           *[other] { $deleted } Teildateien gelöscht
        }
       *[other] { $count } Übertragungen verworfen: { $deleted ->
            [one] { $deleted } Teildatei gelöscht
           *[other] { $deleted } Teildateien gelöscht
        }
    }
login-password-removed = Gespeichertes Passwort entfernt
login-keyring-error = Schlüsselbund: { $error }
login-password-kept = Passwort nicht gespeichert ({ $error }); es bleibt für diese Anmeldung im Feld
login-pair-needs-password = { $name } benötigt ein Passwort: Speichern Sie es im Tab „Direkt“ oder verwenden Sie SSH-Schlüssel

login-waiting-key = Wartet darauf, dass Sie den Schlüssel des Servers prüfen
login-waiting-certificate = Wartet darauf, dass Sie das Zertifikat des Servers prüfen
login-waiting-passphrase = Wartet auf die Passphrase des Schlüssels
login-trust = Vertrauen und verbinden
login-trust-key = Unbekannter Server
login-trust-key-question = Dem Server { $host } vertrauen?
login-trust-key-detail =
    Dies ist die erste Verbindung zu diesem Server, daher kann seine Identität noch nicht geprüft werden. Der Fingerabdruck seines Host-Schlüssels ist:

    { $fingerprint }

    Wenn er dem echten Schlüssel des Servers entspricht, vertrauen Sie ihm: Er wird in ~/.ssh/known_hosts gespeichert.
login-trust-certificate = Nicht vertrauenswürdiges Zertifikat
login-trust-certificate-question = Dem Zertifikat von { $host } vertrauen?
login-trust-certificate-detail =
    Das TLS-Zertifikat des Servers kann nicht geprüft werden: { $reason }. FTP-Server verwenden oft ein selbstsigniertes Zertifikat. Sein Fingerabdruck ist:

    { $fingerprint }

    Wenn es dem echten Zertifikat des Servers entspricht, vertrauen Sie ihm: Es wird für diesen Server gespeichert.

size-decimal-mark = ,
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

log-level-error = Fehler
log-level-warn = Warnung
log-level-info = Info
log-level-debug = Debug
log-level-trace = Trace

ftp-encryption-explicit-if-available = Explizites FTP über TLS verwenden, falls verfügbar
ftp-encryption-explicit = Explizites FTP über TLS erzwingen
ftp-encryption-implicit = Implizites FTP über TLS erzwingen
ftp-encryption-none = Nur unverschlüsseltes FTP verwenden (unsicher)

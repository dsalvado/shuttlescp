# Les textes de l'interface de ShuttleSCP en français. Traduction automatique (IA),
# pas encore relue par une personne de langue française : les corrections sont les
# bienvenues (voir le README). Mêmes noms de messages que i18n/en-US/shuttlescp.ftl.
#
# Syntaxe Fluent : https://projectfluent.org/fluent/guide/

-app-name = ShuttleSCP

language-name = Français (traduction automatique)

dialog-close = Fermer

preferences-title = Préférences
preferences-language = _Langue de l'interface :
preferences-restart = Redémarrez { -app-name } pour utiliser la nouvelle langue.

status-transfers-unlimited = Transferts : illimités
status-transfers-limited = Transferts : { $running } sur { $limit }
status-transfers-waiting = { $count } en attente
dialog-cancel = Annuler

quit-title = Quitter
quit-question = Quitter { -app-name } ?
quit-detail = { $unfinished } Conservez-les pour les reprendre à la prochaine connexion, ou supprimez leurs fichiers partiels.
quit-running =
    { $count ->
        [one] { $count } transfert est encore en cours.
       *[other] { $count } transferts sont encore en cours.
    }
quit-paused =
    { $count ->
        [one] { $count } transfert est en pause.
       *[other] { $count } transferts sont en pause.
    }
quit-unfinished = { $count } transferts ne sont pas terminés ({ $running } en cours, { $paused } en pause).
quit-discard = Supprimer les fichiers partiels et quitter
quit-keep = Conserver pour plus tard et quitter

passphrase-export-title = Exporter les sites
passphrase-import-title = Importer des sites
passphrase-export-explain = Les mots de passe enregistrés sont chiffrés avec une phrase secrète de votre choix. Vous en aurez besoin pour importer le fichier sur un autre ordinateur.
passphrase-import-explain = Ce fichier contient des mots de passe chiffrés. Saisissez la phrase secrète utilisée pour l'exporter.
passphrase-include = Inclure les mots de passe enregistrés
passphrase-new = Phrase secrète (au moins 8 caractères)
passphrase-field = Phrase secrète
passphrase-repeat = Répétez la phrase secrète
passphrase-export = Exporter
passphrase-import = Importer

key-passphrase-title = Phrase secrète de la clé
key-passphrase-question = La clé { $key } est protégée par une phrase secrète
key-passphrase-wrong = Cette phrase secrète n'ouvre pas la clé. Veuillez réessayer.
key-passphrase-explain = Saisissez sa phrase secrète pour vous connecter avec elle.
key-passphrase-save = Enregistrer la phrase secrète dans le trousseau
key-passphrase-unlock = Déverrouiller

conflict-title = Le fichier existe déjà
conflict-question = { $name } existe déjà dans { $folder }
conflict-existing = Existant :
conflict-new = Nouveau :
conflict-for-all = Faire de même pour les autres fichiers existants
conflict-cancel = Annuler le transfert
conflict-skip = Ignorer
conflict-overwrite = Remplacer
conflict-newer = {" "}(plus récent)
key-passphrase-save-tooltip = La conserver dans le trousseau du bureau (chiffrée avec votre mot de passe de session), pour que la clé s'ouvre sans rien demander

update-restart-title = Redémarrer
update-restart-question = Redémarrer avec { -app-name } { $version } ?
update-restart-running =
    { $count ->
        [one] { $count } transfert est encore en cours.
       *[other] { $count } transferts sont encore en cours.
    }
update-restart-detail = { $running } { -app-name } peut redémarrer de lui-même une fois qu'ils sont terminés, ou maintenant : ils s'arrêtent et pourront être repris à la prochaine connexion au serveur.
update-later = Plus tard
update-restart-now = Redémarrer maintenant
update-when-finished = À la fin des transferts
update-check-title = Rechercher des mises à jour
update-up-to-date = { -app-name } est à jour
update-latest = La version { $version } est la plus récente.
update-check-failed = Impossible de rechercher des mises à jour
update-check-error = { $error }.
update-available-title = Mise à jour disponible
update-available = { -app-name } { $version } est disponible
update-you-have = Vous avez la version { $current }. La nouvelle version est sortie le { $date }.
update-whats-new = Nouveautés de la version { $version }
update-mac-steps = Téléchargez-la, ouvrez l'image disque et faites glisser { -app-name } dans Applications, en remplaçant cette version.
update-download = Télécharger
update-from-page = Téléchargez la nouvelle version depuis la page web de { -app-name }.
update-open-page = Ouvrir la page de téléchargement
update-downloading = Téléchargement...
update-install = Installer et redémarrer
update-downloading-progress = Téléchargement... { $done } sur { $total }
update-failed = { $error }. Vous pouvez la télécharger depuis la page web de { -app-name } à la place.

menu-file = _Fichier
menu-edit = É_dition
menu-view = Affic_hage
menu-tools = _Outils
menu-window = Fe_nêtre
menu-help = _Aide
menu-login = _Connexion…
menu-disconnect = _Déconnexion
menu-preferences = _Préférences…
menu-settings = _Réglages…
menu-quit = _Quitter
menu-theme = Thème
menu-log-level = _Niveau de journalisation
menu-open-log = _Ouvrir le fichier journal
menu-check-updates = _Rechercher des mises à jour…
menu-auto-updates = Rechercher au _démarrage
menu-about = À _propos de { -app-name }

edit-undo = Annuler
edit-redo = Rétablir
edit-cut = Couper
edit-copy = Copier
edit-paste = Coller
edit-delete = Supprimer
edit-select-all = Tout sélectionner

not-connected = Non connecté
new-connection = Nouvelle connexion
new-connection-tooltip = Se connecter à un autre serveur dans un nouvel onglet
this-computer-title = Cet ordinateur · { $name }
this-computer = cet ordinateur
status-connected = Connecté à { $name }
status-connected-pair = Connecté à { $left } et { $right }
status-transfers-tooltip = Transferts en cours en même temps, toutes connexions confondues. Cliquez pour les limiter.
status-log-level = Niveau de journalisation : { $level }
session-activity = Transferts en cours
session-close = Se déconnecter et fermer cette session
close-session-title = Fermer la session
close-session-question = Fermer la session ?
stop-transfers-detail =
    { $count ->
        [one] { $count } transfert est encore en cours. Il va s'arrêter et pourra être repris à la prochaine connexion à ce serveur.
       *[other] { $count } transferts sont encore en cours. Ils vont s'arrêter et pourront être repris à la prochaine connexion à ce serveur.
    }
delete-title = Supprimer
delete-detail = Les dossiers sont supprimés avec tout leur contenu. Cette action est irréversible.

update-notice-available = Mise à jour disponible : { $version }
update-notice-waiting = Mise à jour en attente des transferts
update-notice-restart = Redémarrez pour terminer la mise à jour
update-notice-available-tooltip = { -app-name } { $version } est disponible (vous avez la { $current }). Cliquez pour la voir.
update-notice-waiting-tooltip = { -app-name } redémarrera avec la { $version } une fois les transferts en cours terminés. Cliquez pour redémarrer maintenant ou plus tard.
update-notice-restart-tooltip = { -app-name } { $version } est prêt. Cliquez pour redémarrer avec.
update-notice-hide = Masquer cet avis

limit-title = Transferts en même temps
limit-unlimited = Illimités
limit-at-most = Au plus
limit-hint = Toutes connexions confondues. Les autres attendent dans la file et démarrent à mesure que les premiers se terminent.

relay-tooltip = Les fichiers copiés entre les deux serveurs passent par cet ordinateur
relay-to-right = Copie du serveur de gauche vers celui de droite
relay-to-left = Copie du serveur de droite vers celui de gauche
relay-receiving = Réception depuis le serveur d'origine des copies
relay-sending = Envoi vers le serveur de destination des copies

panel-log = Journal
list-queue = File d'attente
list-completed = Terminés
list-failed = Échoués
list-queue-tooltip = Transferts
list-completed-tooltip = Transferts terminés
list-failed-tooltip = Transferts échoués
list-for-connection = { $list } pour { $name }
list-not-connected = { $list } (non connecté)
log-unavailable = La journalisation dans un fichier n'est pas disponible, il n'y a donc rien à afficher.

theme-system = Valeur du système
theme-swatch = Couleurs de la fenêtre et d'accentuation
disk-tooltip = Disque

about-title = À propos de { -app-name }
about-version = Version { $version } ({ $revision })
about-description = Un client SFTP, SCP et FTP graphique pour Linux, macOS et Windows, inspiré de WinSCP.
about-authors = Par { $authors }
about-license = Logiciel libre : { $license }
about-system-details = Détails du système
about-copy-details = Copier les détails

pane-gathering = Collecte des informations...
pane-gathering-tooltip = Collecte des informations
pane-saved-gathering = Liste enregistrée du { $saved } · collecte des informations...
pane-saved-stale = Liste enregistrée du { $saved } ; impossible de vérifier les changements : { $error }
pane-refresh-tooltip = Actualiser
pane-items =
    { $count ->
        [one] { $count } élément
       *[other] { $count } éléments
    }
pane-created = Dossier { $name } créé
pane-renamed = Renommé en { $name }
pane-new-folder-title = Nouveau dossier
pane-new-folder-prompt = Créer un dossier dans { $folder } :
pane-new-folder-create = Créer
pane-new-folder-name = Nouveau dossier
pane-rename-title = Renommer
pane-rename-prompt = Nouveau nom pour { $name } :
pane-open-title = Ouvrir le fichier
pane-open-failed = Impossible d'ouvrir { $name }
pane-open-error = { $error }.
pane-column-name = Nom
pane-column-size = Taille
pane-column-modified = Modifié

pane-transfer =
    { $kind ->
        [upload] _Envoyer
        [download] _Télécharger
       *[copy] _Copier
    }
pane-transfer-to =
    { $kind ->
        [upload] _Envoyer vers { $name }
        [download] _Télécharger vers { $name }
       *[copy] _Copier vers { $name }
    }
pane-move = _Déplacer
pane-move-to = _Déplacer vers { $name }
pane-menu-new-folder = _Nouveau dossier…
pane-menu-rename = _Renommer…
pane-menu-delete = _Supprimer
pane-menu-refresh = _Actualiser

pane-drop-copy = _Copier ici
pane-drop-move = _Déplacer ici
pane-drop-cancel = _Annuler

transfers-resume = Reprendre
transfers-discard = Abandonner
transfers-discard-tooltip = Les oublier ; leurs fichiers partiels restent où ils sont
transfers-hide-finished = Masquer les fichiers terminés
transfers-hide-finished-tooltip = Retirer de la liste les fichiers copiés sans problème, pour que ceux en cours de copie restent en haut
transfers-queue-empty = File d'attente vide
transfers-completed-empty = Aucun transfert terminé
transfers-failed-empty = Aucun transfert échoué
transfers-clear = Effacer
transfers-interrupted =
    { $count ->
        [one] { $count } transfert a été interrompu et peut être repris.
       *[other] { $count } transferts ont été interrompus et peuvent être repris.
    }
transfers-blocked = Non démarré : « { $other } » utilise les mêmes fichiers. Réessayez une fois qu'il est terminé.

progress-percent = { $percent } %
progress-left = { $time } restant
progress-speed = { $size }/s
progress-files = { $done } fichiers sur { $total }
progress-bytes = { $done } sur { $total }

job-waiting-slot = En attente d'une place de transfert libre
job-pausing = Mise en pause...
job-paused = En pause
job-paused-copied = En pause : { $done } sur { $total } copiés
job-preparing = Préparation...
job-deleting = { $done } sur { $total } supprimés
job-relay-right =
    { $kind ->
        [move] Déplacer { $subject } : { $from } → { $to }
       *[copy] Copier { $subject } : { $from } → { $to }
    }
job-relay-left =
    { $kind ->
        [move] Déplacer { $subject } : { $to } ← { $from }
       *[copy] Copier { $subject } : { $to } ← { $from }
    }
job-resumed = { $title } (repris)
job-cancel = Annuler tout le transfert
job-cancel-paused = Annuler le transfert en pause
job-pause = Pause : arrêter pour l'instant, en gardant ce qui est copié
job-resume = Reprendre : continuer là où il a été mis en pause
job-retry = Réessayer : continuer là où il s'est arrêté
job-remove = Retirer de la liste

summary-separator = {", "}
summary-failed = { $done } réussis, { $failed } échoués
summary-cancelled-job = Annulé : { $done } réussis
summary-deleted =
    { $count ->
        [one] { $count } élément supprimé
       *[other] { $count } éléments supprimés
    }
summary-copied =
    { $count ->
        [one] { $count } élément · { $size }
       *[other] { $count } éléments · { $size }
    }
summary-cancelled = { $count } annulés
summary-skipped = { $count } ignorés
summary-stopped = { $count } arrêtés

item-deleted = Supprimé
item-skipped = Ignoré : le fichier existant a été conservé
item-paused = En pause
item-paused-slot = En pause : en attente d'une place de transfert libre
item-paused-part = En pause : { $done } sur { $total }
item-cancelled = Annulé
item-stopped = Arrêté
item-stopped-delete = Arrêté : non supprimé
item-asking = Existe déjà : en attente de votre réponse
item-waiting = En attente
item-stop-file = Arrêter ce fichier ; le reste continue
item-stop-folder = Arrêter ce dossier ; le reste continue
item-pause-file = Mettre ce fichier en pause ; le reste continue
item-pause-folder = Mettre les fichiers de ce dossier en pause ; le reste continue
item-resume-file = Reprendre ce fichier
item-resume-folder = Reprendre ce dossier

login-new-site = Nouveau site
login-alias-optional = Facultatif
login-connecting = Connexion à { $name }...
login-deleted-site = (site supprimé)
login-forgotten = Oubliés
login-deleting-partials = Suppression des fichiers partiels...
login-save-failed = Impossible d'enregistrer les sites : { $error }
login-folder-exists = Il existe déjà un dossier « { $folder } »
login-folder-move-failed = Impossible de déplacer « { $folder } » : { $error }

login-password-replace = Enregistré - saisissez pour le remplacer
login-password-dots = •••••••• (enregistré)
login-password-not-saved = Non enregistré
login-password-saved = Mot de passe enregistré dans le trousseau
login-password-none = Aucun mot de passe enregistré
login-password-saving = Enregistrement du mot de passe dans le trousseau...

login-ftp-encryption = FTP · { $encryption }
login-way-key = fichier de clé { $key }
login-way-agent = agent SSH
login-way-password = mot de passe enregistré
login-way-none = aucun mot de passe enregistré
login-way-then = {", puis "}

login-key-chooser-title = Fichier de clé privée
login-key-chooser-choose = _Choisir
login-chooser-export = _Exporter
login-chooser-import = _Importer
login-chooser-cancel = _Annuler
login-chooser-filter = Exports de sites (*.json)

login-unfinished-tab = Inachevés
login-unfinished-tab-count = Inachevés ({ $count })
login-unfinished-one-site = Cochez les transferts d'un seul site (ou d'une seule paire de sites) à la fois.
login-unfinished-site-gone = Son site a été supprimé, il ne peut donc pas être repris. Abandonner l'oublie (supprimez les fichiers partiels à la main).
login-unfinished-folder = { $folder } ({ $place })
login-unfinished-what =
    { $kind ->
        [move] Déplacer { $subject }
        [delete] Supprimer { $subject }
       *[copy] Copier { $subject }
    }
login-unfinished-saved-unknown = Inconnu (enregistré par une ancienne version)

login-title = Connexion
login-new-folder-tooltip = Nouveau dossier (dans le dossier choisi). Faites glisser les sites et les dossiers sur un dossier pour les y déplacer.
login-rename = Renommer
login-edit = Modifier
login-session = Session
login-alias = Alias :
login-alias-tooltip = Un nom pour le site, affiché dans la liste des sites et sur l'onglet de la session ; laissé vide, c'est l'adresse qui est affichée
login-protocol = Protocole de fichiers :
login-encryption-tooltip = Comment la connexion FTP est chiffrée
login-passive = Mode passif
login-passive-tooltip = Le client ouvre les connexions de données (fonctionne à travers les pare-feu et le NAT). Décochez pour le mode actif.
login-host = Nom d'hôte :
login-port = Port :
login-user = Nom d'utilisateur :
login-password = Mot de passe :
login-show-password = Afficher le mot de passe
login-save-password = Enregistrer le mot de passe
login-save-password-tooltip = Conserver le mot de passe dans le trousseau du bureau (chiffré avec votre mot de passe de session)
login-key-file = Fichier de clé privée :
login-key-file-placeholder = Aucun (clé OpenSSH ou PuTTY .ppk)
login-browse = Parcourir...
login-browse-tooltip = Choisir un fichier de clé privée
login-use-agent = Utiliser l'agent SSH
login-use-agent-tooltip = Essayer aussi les clés de l'agent SSH du bureau (comme le fait ssh)
login-description = Description :
login-advanced = Avancé
login-files-at-once-tooltip = Fichiers copiés en même temps. Davantage peut être plus rapide quand le serveur limite la vitesse de chaque connexion ; FTP ouvre une connexion pour chacun, et certains serveurs n'en permettent que quelques-unes.
login-files-at-once = Fichiers à la fois :
login-save = Enregistrer
login-tab-direct = Direct
login-site-to-site-intro = Les copies entre les deux serveurs passent par cet ordinateur. Choisissez un site enregistré pour chaque connexion, puis Connexion.
login-connection-1 = Connexion 1
login-connection-1-tooltip = Affichée dans le volet de gauche
login-detail-protocol = Protocole :
login-detail-address = Adresse :
login-detail-user = Utilisateur :
login-detail-login = Connexion :
login-connection-2 = Connexion 2
login-connection-2-tooltip = Affichée dans le volet de droite
login-tab-site-to-site = Site à site
login-unfinished-intro = Transferts qui ne se sont pas terminés. Cochez ceux à reprendre : { -app-name } se connecte à leur site avec son mot de passe ou ses clés enregistrés et l'ouvre là où ils en étaient. Abandonner supprime plutôt leurs fichiers partiels.
login-detail-transfer = Transfert :
login-detail-from = De :
login-detail-to = Vers :
login-detail-saved = Enregistré :
login-discard-tooltip = Se connecter, supprimer les fichiers partiels des transferts cochés et les oublier
login-resume-tooltip = Se connecter et continuer les transferts cochés là où ils se sont arrêtés
login-show-at-startup = Afficher au démarrage
login-show-at-startup-tooltip = Ouvrir cette fenêtre au démarrage de { -app-name }
login-import = Importer...
login-import-tooltip = Ajouter des sites depuis un fichier d'export de { -app-name }
login-export = Exporter...
login-export-tooltip = Enregistrer tous les sites dans un fichier pour un autre ordinateur
login-login-tooltip = Se connecter au site choisi
login-files-at-once-usual = (habituellement { $count })

login-unfinished-none = Aucun transfert inachevé
login-unfinished-notice =
    { $count ->
        [one] { $count } transfert ne s'est pas terminé : voir l'onglet Inachevés
       *[other] { $count } transferts ne se sont pas terminés : voir l'onglet Inachevés
    }
login-unfinished-tick-tooltip = Reprendre ou abandonner ce transfert
login-folder-open = Ouvrir le dossier
login-folder-close = Fermer le dossier
login-password-saved-tooltip = Mot de passe enregistré
login-rename-folder = Renommer le dossier
login-rename-folder-prompt = Nouveau nom pour le dossier { $folder } :
login-new-folder-prompt = Nom du nouveau dossier :
login-new-folder-prompt-in = Nom du nouveau dossier dans { $folder } :
login-delete-folder = Supprimer le dossier
login-delete-folder-question = Supprimer le dossier { $folder } ?
login-delete-folder-to-top = Les sites et dossiers qu'il contient remontent en haut de la liste. Aucun site n'est supprimé.
login-delete-folder-to-parent = Les sites et dossiers qu'il contient passent dans { $folder }. Aucun site n'est supprimé.
login-delete-site = Supprimer le site
login-delete-site-question = Supprimer { $name } ?
login-delete-site-with-password = Le site et son mot de passe enregistré (dans le trousseau) seront supprimés. Cette action est irréversible.
login-delete-site-detail = Le site sera supprimé. Cette action est irréversible.
login-exporting = Export...
login-importing = Import...
login-exported =
    { $count ->
        [one] { $count } site exporté ({ $passwords } avec mot de passe) vers { $file }
       *[other] { $count } sites exportés ({ $passwords } avec mot de passe) vers { $file }
    }
login-exported-unreadable =  ; { $count } mots de passe n'ont pas pu être lus
login-export-failed = Échec de l'export : { $error }
login-imported =
    { $count ->
        [one] { $count } site importé ({ $added } nouveaux, { $replaced } mis à jour, { $passwords } avec mot de passe)
       *[other] { $count } sites importés ({ $added } nouveaux, { $replaced } mis à jour, { $passwords } avec mot de passe)
    }
login-imported-unstored =  ; { $count } mots de passe n'ont pas pu être enregistrés
login-import-failed = Échec de l'import : { $error }
login-not-discarded = Non abandonné : { $reason }
login-discarded =
    { $count ->
        [one] { $count } transfert abandonné : { $deleted ->
            [one] { $deleted } fichier partiel supprimé
           *[other] { $deleted } fichiers partiels supprimés
        }
       *[other] { $count } transferts abandonnés : { $deleted ->
            [one] { $deleted } fichier partiel supprimé
           *[other] { $deleted } fichiers partiels supprimés
        }
    }
login-password-removed = Mot de passe enregistré supprimé
login-keyring-error = Trousseau : { $error }
login-password-kept = Mot de passe non enregistré ({ $error }) ; il reste dans le champ pour cette connexion
login-pair-needs-password = { $name } a besoin d'un mot de passe : enregistrez-le dans l'onglet Direct, ou utilisez des clés SSH

login-waiting-key = En attente de votre vérification de la clé du serveur
login-waiting-certificate = En attente de votre vérification du certificat du serveur
login-waiting-passphrase = En attente de la phrase secrète de la clé
login-trust = Faire confiance et se connecter
login-trust-key = Serveur inconnu
login-trust-key-question = Faire confiance au serveur { $host } ?
login-trust-key-detail =
    C'est la première connexion à ce serveur, son identité ne peut donc pas encore être vérifiée. L'empreinte de sa clé d'hôte est :

    { $fingerprint }

    Si elle correspond à la vraie clé du serveur, faites-lui confiance : elle sera mémorisée dans ~/.ssh/known_hosts.
login-trust-certificate = Certificat non fiable
login-trust-certificate-question = Faire confiance au certificat de { $host } ?
login-trust-certificate-detail =
    Le certificat TLS du serveur ne peut pas être vérifié : { $reason }. Les serveurs FTP utilisent souvent un certificat auto-signé. Son empreinte est :

    { $fingerprint }

    S'il correspond au vrai certificat du serveur, faites-lui confiance : il sera mémorisé pour ce serveur.

size-decimal-mark = ,
size =
    { $unit ->
        [b] { $size } o
        [kib] { $size } Kio
        [mib] { $size } Mio
        [gib] { $size } Gio
        [tib] { $size } Tio
        [pib] { $size } Pio
       *[eib] { $size } Eio
    }

log-level-error = Erreur
log-level-warn = Avertissement
log-level-info = Info
log-level-debug = Débogage
log-level-trace = Trace

ftp-encryption-explicit-if-available = Utiliser FTP sur TLS explicite si disponible
ftp-encryption-explicit = Exiger FTP sur TLS explicite
ftp-encryption-implicit = Exiger FTP sur TLS implicite
ftp-encryption-none = N'utiliser que FTP simple (non sécurisé)

job-title =
    { $action ->
        [upload] Envoyer { $what } vers { $dest }
        [download] Télécharger { $what } vers { $dest }
        [move-to-server] { $dest }éplacer { $what } vers le serveur, { $dest }
        [move-to-computer] { $dest }éplacer { $what } vers cet ordinateur, { $dest }
        [copy-here] Copier { $what } vers { $dest }
        [copy-on-server] Copier { $what } vers { $dest } sur le serveur
        [move-here] { $dest }éplacer { $what } vers { $dest }
        [move-on-server] { $dest }éplacer { $what } vers { $dest } sur le serveur
        [copy-other-server] Copier { $what } vers { $dest } sur l'autre serveur
        [move-other-server] { $dest }éplacer { $what } vers { $dest } sur l'autre serveur
        [delete-here] Supprimer { $what } de cet ordinateur
       *[delete-on-server] Supprimer { $what } du serveur
    }
job-items =
    { $count ->
        [one] { $count } élément
       *[other] { $count } éléments
    }

error-cannot =
    { $action ->
        [read] impossible de lire { $path } : { $reason }
        [open] impossible d'ouvrir { $path } : { $reason }
        [write] impossible d'écrire { $path } : { $reason }
        [create] impossible de créer { $path } : { $reason }
        [create-folder] impossible de créer le dossier { $path } : { $reason }
        [delete] impossible de supprimer { $path } : { $reason }
        [delete-folder] impossible de supprimer le dossier { $path } : { $reason }
        [set-time] impossible de régler l'heure de { $path } : { $reason }
        [finish] impossible de terminer { $path } : { $reason }
        [download] impossible de télécharger { $path } : { $reason }
        [replace] impossible de remplacer { $path } : { $reason }
        [access] impossible d'accéder à { $path } : { $reason }
       *[make-runnable] impossible de rendre { $path } exécutable : { $reason }
    }
error-cannot-bare =
    { $action ->
        [read] impossible de lire { $path }
        [open] impossible d'ouvrir { $path }
        [write] impossible d'écrire { $path }
        [create] impossible de créer { $path }
        [create-folder] impossible de créer le dossier { $path }
        [delete] impossible de supprimer { $path }
        [delete-folder] impossible de supprimer le dossier { $path }
        [set-time] impossible de régler l'heure de { $path }
        [finish] impossible de terminer { $path }
        [download] impossible de télécharger { $path }
        [replace] impossible de remplacer { $path }
        [access] impossible d'accéder à { $path }
       *[make-runnable] impossible de rendre { $path } exécutable
    }
error-cannot-rename = impossible de renommer { $from } en { $to } : { $reason }
error-cannot-rename-bare = impossible de renommer { $from } en { $to }
error-no-such-item = aucun fichier ou dossier de ce nom
error-sftp-date = { $path } a une date que SFTP ne peut pas enregistrer
checksum-unsupported = le serveur ne peut pas calculer de sommes de contrôle
checksum-no-answer = la réponse du serveur ne contenait aucune somme de contrôle
verify-mismatch = la somme de contrôle de la copie diffère de celle de l'original : la copie n'est pas ce qui a été copié
connect-unsupported = les connexions { $protocol } ne sont pas encore prises en charge
connect-timeout = { $host } n'a pas répondu (délai dépassé)
connect-network = impossible de se connecter à { $host } : { $reason }
connect-unknown-host-key = la clé d'hôte du serveur n'est pas encore connue ({ $fingerprint })
connect-host-key-changed = ATTENTION : la clé d'hôte de { $host } a changé (ligne { $line } de known_hosts). Quelqu'un pourrait intercepter la connexion ; connexion refusée.
connect-untrusted-certificate = le certificat du serveur n'est pas fiable ({ $reason })
connect-password-required = saisissez le mot de passe
connect-keys-rejected = le serveur n'a pas accepté la clé SSH ; saisissez le mot de passe, ou vérifiez la clé
connect-key-passphrase-required = le fichier de clé { $path } a besoin de sa phrase secrète
connect-key-unreadable = impossible d'utiliser le fichier de clé { $path } : { $reason }
connect-authentication-failed = échec de la connexion : nom d'utilisateur, clé ou mot de passe incorrect
connect-sftp = le service de transfert de fichiers (SFTP) du serveur a échoué : { $reason }
connect-ftp = FTP : { $reason }
connect-host-certificates = les certificats d'hôte ne sont pas pris en charge
connect-ftp-home = impossible de lire le dossier personnel : { $reason }
connect-ftp-no-tls = le serveur ne propose pas le chiffrement TLS ({ $reply }) ; choisissez « { $choice } » pour vous connecter sans l'exiger
connect-tls-setup = échec de la configuration TLS : { $reason }
connect-no-authorities = ce système n'a aucune autorité de certification de confiance
certificate-problem =
    { $reason ->
        [unknown-issuer] il n'est pas signé par une autorité de confiance pour cet ordinateur
        [expired] il a expiré
        [not-valid-yet] il n'est pas encore valide
        [other-name] il a été émis pour un autre nom de serveur
        [revoked] il a été révoqué
       *[self-signed] il est auto-signé
    }
name-empty = saisissez un nom
name-reserved = « { $name } » ne peut pas servir de nom
name-forbidden-character = un nom ne peut pas contenir « { $character } »
name-null = un nom ne peut pas contenir de caractère nul
name-control-characters = un nom ne peut pas contenir de caractères de contrôle
name-trailing-space-or-dot = un nom ne peut pas se terminer par une espace ou un point
name-windows-device = « { $name } » est un nom que Windows réserve à un périphérique
error-already-exists = { $name } existe déjà
error-into-itself = { $name } ne peut pas aller dans lui-même
error-already-there = { $name } est déjà dans { $folder }
error-parent-not-created = le dossier qui doit le contenir n'a pas pu être créé
error-folder-there = un dossier nommé { $name } est déjà là
error-items-inside-failed = certains éléments à l'intérieur ont échoué
error-items-inside-not-deleted = certains éléments à l'intérieur n'ont pas été supprimés
error-copied-but = copié, mais { $reason }
error-not-connected = non connecté à un serveur
error-not-connected-destination = non connecté au serveur de destination des copies
error-cannot-copy-root = { $path } ne peut être ni copié ni supprimé
error-nested-too-deeply = { $path } est imbriqué trop profondément (une boucle de liens symboliques ?)
update-error-unsigned = la mise à jour n'est pas signée par la clé de publication de { -app-name }
update-error-unreadable = la description de la mise à jour est illisible : { $reason }
update-error-unexpected-place = la mise à jour serait téléchargée depuis un endroit inattendu : { $url }
update-error-cancelled = le téléchargement a été annulé
update-error-failed = le téléchargement a échoué : { $reason }
update-error-too-large = le téléchargement est plus gros que ce qu'annonce la version
update-error-save = impossible d'enregistrer le téléchargement : { $reason }
update-error-incomplete = le téléchargement est incomplet ({ $done } octets sur { $total })
update-error-mismatch = le téléchargement ne correspond pas à la version signée (SHA-256 différent)
update-error-no-key = cette version ne peut pas rechercher de mises à jour
update-error-bad-key = la clé de publication intégrée est endommagée : { $reason }
update-error-unreachable = impossible d'atteindre la page des versions : { $reason }
update-error-description = impossible de lire la description de la version : { $reason }
update-error-version = « { $version } » n'est pas un numéro de version
passphrase-problem-empty = Saisissez la phrase secrète
passphrase-problem-short = Utilisez au moins { $count } caractères
passphrase-problem-mismatch = Les phrases secrètes ne correspondent pas
export-error-json = { $path } n'est pas un export de sites valide : { $reason }
export-error-not-export = le fichier n'est pas un export de sites de { -app-name }
export-error-version = version d'export non prise en charge : { $version }
export-error-passphrase-required = une phrase secrète est nécessaire pour importer les mots de passe enregistrés
export-error-wrong-passphrase = phrase secrète incorrecte, ou le fichier a été modifié
export-error-settings = paramètres de chiffrement non valides dans le fichier : { $reason }
export-error-random = impossible de générer des données aléatoires : { $reason }
keyring-error-service = erreur du trousseau : { $reason }
keyring-error-not-text = le mot de passe enregistré n'est pas un texte valide
keyring-error-unsupported = l'enregistrement des mots de passe n'est pas encore pris en charge sur ce système
settings-error-invalid = paramètres non valides dans { $path } : { $reason }
folder-error-into-itself = un dossier ne peut pas aller dans lui-même
folder-error-exists = il existe déjà un dossier « { $folder } »

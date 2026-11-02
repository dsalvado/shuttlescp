# Los textos de la interfaz de ShuttleSCP en español. Traducción automática (IA), aún
# no revisada por una persona hispanohablante: las correcciones son bienvenidas (ver el
# README). Mismos nombres de mensajes que i18n/en-US/shuttlescp.ftl.
#
# Sintaxis de Fluent: https://projectfluent.org/fluent/guide/

-app-name = ShuttleSCP

language-name = Español (traducción automática)

dialog-close = Cerrar

preferences-title = Preferencias
preferences-language = _Idioma de la interfaz:
preferences-restart = Reinicie { -app-name } para usar el nuevo idioma.

status-transfers-unlimited = Transferencias: sin límite
status-transfers-limited = Transferencias: { $running } de { $limit }
status-transfers-waiting = { $count } en espera
dialog-cancel = Cancelar

quit-title = Salir
quit-question = ¿Salir de { -app-name }?
quit-detail = { $unfinished } Consérvelas para reanudarlas la próxima vez que se conecte, o elimine sus archivos parciales.
quit-running =
    { $count ->
        [one] { $count } transferencia sigue en curso.
       *[other] { $count } transferencias siguen en curso.
    }
quit-paused =
    { $count ->
        [one] { $count } transferencia está en pausa.
       *[other] { $count } transferencias están en pausa.
    }
quit-unfinished = { $count } transferencias no han terminado ({ $running } en curso, { $paused } en pausa).
quit-discard = Eliminar archivos parciales y salir
quit-keep = Conservar para más tarde y salir

passphrase-export-title = Exportar sitios
passphrase-import-title = Importar sitios
passphrase-export-explain = Las contraseñas guardadas se cifran con una frase de contraseña que usted elige. La necesitará para importar el archivo en otro equipo.
passphrase-import-explain = Este archivo contiene contraseñas cifradas. Introduzca la frase de contraseña con la que se exportó.
passphrase-include = Incluir las contraseñas guardadas
passphrase-new = Frase de contraseña (al menos 8 caracteres)
passphrase-field = Frase de contraseña
passphrase-repeat = Repita la frase de contraseña
passphrase-export = Exportar
passphrase-import = Importar

key-passphrase-title = Frase de contraseña de la clave
key-passphrase-question = La clave { $key } está protegida por una frase de contraseña
key-passphrase-wrong = Esa frase de contraseña no abre la clave. Inténtelo de nuevo.
key-passphrase-explain = Introduzca su frase de contraseña para iniciar sesión con ella.
key-passphrase-save = Guardar la frase de contraseña en el llavero
key-passphrase-unlock = Desbloquear

conflict-title = El archivo ya existe
conflict-question = { $name } ya existe en { $folder }
conflict-existing = Existente:
conflict-new = Nuevo:
conflict-for-all = Hacer lo mismo con los demás archivos existentes
conflict-cancel = Cancelar transferencia
conflict-skip = Omitir
conflict-overwrite = Sobrescribir
conflict-newer = {" "}(más reciente)
key-passphrase-save-tooltip = Guardarla en el llavero del escritorio (cifrada con su contraseña de sesión), para que la clave se abra sin preguntar

update-restart-title = Reiniciar
update-restart-question = ¿Reiniciar con { -app-name } { $version }?
update-restart-running =
    { $count ->
        [one] { $count } transferencia sigue en curso.
       *[other] { $count } transferencias siguen en curso.
    }
update-restart-detail = { $running } { -app-name } puede reiniciarse solo cuando terminen, o ahora: se detienen y podrán reanudarse la próxima vez que se conecte al servidor.
update-later = Más tarde
update-restart-now = Reiniciar ahora
update-when-finished = Al terminar las transferencias
update-check-title = Buscar actualizaciones
update-up-to-date = { -app-name } está actualizado
update-latest = La versión { $version } es la más reciente.
update-check-failed = No se pueden buscar actualizaciones
update-check-error = { $error }.
update-available-title = Actualización disponible
update-available = { -app-name } { $version } está disponible
update-you-have = Tiene la versión { $current }. La nueva versión se publicó el { $date }.
update-whats-new = Novedades de la versión { $version }
update-mac-steps = Descárguela, abra la imagen de disco y arrastre { -app-name } a Aplicaciones, reemplazando esta versión.
update-download = Descargar
update-from-page = Descargue la nueva versión desde la página web de { -app-name }.
update-open-page = Abrir la página de descarga
update-downloading = Descargando...
update-install = Instalar y reiniciar
update-downloading-progress = Descargando... { $done } de { $total }
update-failed = { $error }. Puede descargarla desde la página web de { -app-name }.

menu-file = _Archivo
menu-edit = _Edición
menu-view = _Ver
menu-tools = _Herramientas
menu-window = Ve_ntana
menu-help = Ay_uda
menu-login = _Iniciar sesión…
menu-disconnect = _Desconectar
menu-preferences = _Preferencias…
menu-settings = _Ajustes…
menu-quit = _Salir
menu-theme = Tema
menu-log-level = _Nivel de registro
menu-open-log = _Abrir el archivo de registro
menu-check-updates = _Buscar actualizaciones…
menu-auto-updates = Buscar al _iniciar
menu-about = _Acerca de { -app-name }

edit-undo = Deshacer
edit-redo = Rehacer
edit-cut = Cortar
edit-copy = Copiar
edit-paste = Pegar
edit-delete = Eliminar
edit-select-all = Seleccionar todo

not-connected = Sin conexión
new-connection = Nueva conexión
new-connection-tooltip = Conectarse a otro servidor en una pestaña nueva
this-computer-title = Este equipo · { $name }
this-computer = este equipo
status-connected = Conectado a { $name }
status-connected-pair = Conectado a { $left } y { $right }
status-transfers-tooltip = Transferencias simultáneas, en todas las conexiones. Haga clic para limitarlas.
status-log-level = Nivel de registro: { $level }
session-activity = Transferencias en curso
session-close = Desconectar y cerrar esta sesión
close-session-title = Cerrar sesión
close-session-question = ¿Cerrar la sesión?
stop-transfers-detail =
    { $count ->
        [one] { $count } transferencia sigue en curso. Se detendrá y podrá reanudarse la próxima vez que se conecte a este servidor.
       *[other] { $count } transferencias siguen en curso. Se detendrán y podrán reanudarse la próxima vez que se conecte a este servidor.
    }
delete-title = Eliminar
delete-detail = Las carpetas se eliminan con todo su contenido. Esta acción no se puede deshacer.

update-notice-available = Actualización disponible: { $version }
update-notice-waiting = La actualización espera a las transferencias
update-notice-restart = Reinicie para terminar la actualización
update-notice-available-tooltip = { -app-name } { $version } está disponible (tiene la { $current }). Haga clic para verla.
update-notice-waiting-tooltip = { -app-name } se reiniciará con la { $version } cuando terminen las transferencias en curso. Haga clic para reiniciar ahora o más tarde.
update-notice-restart-tooltip = { -app-name } { $version } está lista. Haga clic para reiniciar con ella.
update-notice-hide = Ocultar este aviso

limit-title = Transferencias simultáneas
limit-unlimited = Sin límite
limit-at-most = Como máximo
limit-hint = En todas las conexiones. Las demás esperan en la cola y empiezan a medida que terminan otras.

relay-tooltip = Los archivos copiados entre los dos servidores pasan por este equipo
relay-to-right = Copiando del servidor de la izquierda al de la derecha
relay-to-left = Copiando del servidor de la derecha al de la izquierda
relay-receiving = Recibiendo del servidor de origen de las copias
relay-sending = Enviando al servidor de destino de las copias

panel-log = Registro
list-queue = Cola
list-completed = Completadas
list-failed = Fallidas
list-queue-tooltip = Transferencias
list-completed-tooltip = Transferencias completadas
list-failed-tooltip = Transferencias fallidas
list-for-connection = { $list } de { $name }
list-not-connected = { $list } (sin conexión)
log-unavailable = El registro en un archivo no está disponible, así que no hay nada que mostrar.

theme-system = Predeterminado del sistema
theme-swatch = Colores de la ventana y de acento
disk-tooltip = Disco

about-title = Acerca de { -app-name }
about-version = Versión { $version } ({ $revision })
about-description = Un cliente gráfico de SFTP, SCP y FTP para Linux, macOS y Windows, inspirado en WinSCP.
about-authors = Por { $authors }
about-license = Software libre: { $license }
about-system-details = Detalles del sistema
about-copy-details = Copiar detalles

pane-gathering = Recopilando información...
pane-gathering-tooltip = Recopilando información
pane-saved-gathering = Lista guardada del { $saved } · recopilando información...
pane-saved-stale = Lista guardada del { $saved }; no se pudieron comprobar los cambios: { $error }
pane-refresh-tooltip = Actualizar
pane-items =
    { $count ->
        [one] { $count } elemento
       *[other] { $count } elementos
    }
pane-created = Carpeta { $name } creada
pane-renamed = Renombrado como { $name }
pane-new-folder-title = Nueva carpeta
pane-new-folder-prompt = Crear una carpeta en { $folder }:
pane-new-folder-create = Crear
pane-new-folder-name = Nueva carpeta
pane-rename-title = Renombrar
pane-rename-prompt = Nuevo nombre para { $name }:
pane-open-title = Abrir archivo
pane-open-failed = No se puede abrir { $name }
pane-open-error = { $error }.
pane-column-name = Nombre
pane-column-size = Tamaño
pane-column-modified = Modificado

pane-transfer =
    { $kind ->
        [upload] _Subir
        [download] _Descargar
       *[copy] _Copiar
    }
pane-transfer-to =
    { $kind ->
        [upload] _Subir a { $name }
        [download] _Descargar a { $name }
       *[copy] _Copiar a { $name }
    }
pane-move = _Mover
pane-move-to = _Mover a { $name }
pane-menu-new-folder = _Nueva carpeta…
pane-menu-rename = _Renombrar…
pane-menu-delete = _Eliminar
pane-menu-refresh = _Actualizar

pane-drop-copy = _Copiar aquí
pane-drop-move = _Mover aquí
pane-drop-cancel = C_ancelar

transfers-resume = Reanudar
transfers-discard = Descartar
transfers-discard-tooltip = Olvidarlas; sus archivos parciales se quedan donde están
transfers-hide-finished = Ocultar archivos terminados
transfers-hide-finished-tooltip = Quitar de la lista los archivos copiados sin problemas, para que los que se están copiando queden arriba
transfers-queue-empty = Cola vacía
transfers-completed-empty = No hay transferencias completadas
transfers-failed-empty = No hay transferencias fallidas
transfers-clear = Vaciar
transfers-interrupted =
    { $count ->
        [one] { $count } transferencia se interrumpió y puede reanudarse.
       *[other] { $count } transferencias se interrumpieron y pueden reanudarse.
    }
transfers-blocked = No iniciada: «{ $other }» está usando los mismos archivos. Vuelva a intentarlo cuando haya terminado.

progress-percent = { $percent } %
progress-left = quedan { $time }
progress-speed = { $size }/s
progress-files = { $done } de { $total } archivos
progress-bytes = { $done } de { $total }

job-waiting-slot = Esperando un hueco de transferencia libre
job-pausing = Pausando...
job-paused = En pausa
job-paused-copied = En pausa: { $done } de { $total } copiados
job-preparing = Preparando...
job-deleting = { $done } de { $total } eliminados
job-relay-right =
    { $kind ->
        [move] Mover { $subject }: { $from } → { $to }
       *[copy] Copiar { $subject }: { $from } → { $to }
    }
job-relay-left =
    { $kind ->
        [move] Mover { $subject }: { $to } ← { $from }
       *[copy] Copiar { $subject }: { $to } ← { $from }
    }
job-resumed = { $title } (reanudada)
job-cancel = Cancelar toda la transferencia
job-cancel-paused = Cancelar la transferencia en pausa
job-pause = Pausar: detener por ahora, conservando lo copiado
job-resume = Reanudar: continuar donde se pausó
job-retry = Reintentar: continuar donde se detuvo
job-remove = Quitar de la lista

summary-separator = {", "}
summary-failed = { $done } correctos, { $failed } fallidos
summary-cancelled-job = Cancelada: { $done } correctos
summary-deleted =
    { $count ->
        [one] { $count } elemento eliminado
       *[other] { $count } elementos eliminados
    }
summary-copied =
    { $count ->
        [one] { $count } elemento · { $size }
       *[other] { $count } elementos · { $size }
    }
summary-cancelled = { $count } cancelados
summary-skipped = { $count } omitidos
summary-stopped = { $count } detenidos

item-deleted = Eliminado
item-skipped = Omitido: se conservó el archivo existente
item-paused = En pausa
item-paused-slot = En pausa: esperando un hueco de transferencia libre
item-paused-part = En pausa: { $done } de { $total }
item-cancelled = Cancelado
item-stopped = Detenido
item-stopped-delete = Detenido: no eliminado
item-asking = Ya existe: esperando su respuesta
item-waiting = En espera
item-stop-file = Detener este archivo; el resto continúa
item-stop-folder = Detener esta carpeta; el resto continúa
item-pause-file = Pausar este archivo; el resto continúa
item-pause-folder = Pausar los archivos de esta carpeta; el resto continúa
item-resume-file = Reanudar este archivo
item-resume-folder = Reanudar esta carpeta

login-new-site = Nuevo sitio
login-alias-optional = Opcional
login-connecting = Conectando a { $name }...
login-deleted-site = (sitio eliminado)
login-forgotten = Olvidadas
login-deleting-partials = Eliminando archivos parciales...
login-save-failed = No se pueden guardar los sitios: { $error }
login-folder-exists = Ya existe una carpeta «{ $folder }»
login-folder-move-failed = No se puede mover «{ $folder }»: { $error }

login-password-replace = Guardada: escriba para reemplazarla
login-password-dots = •••••••• (guardada)
login-password-not-saved = No guardada
login-password-saved = Contraseña guardada en el llavero
login-password-none = Ninguna contraseña guardada
login-password-saving = Guardando la contraseña en el llavero...

login-ftp-encryption = FTP · { $encryption }
login-way-key = archivo de clave { $key }
login-way-agent = agente SSH
login-way-password = contraseña guardada
login-way-none = ninguna contraseña guardada
login-way-then = {", luego "}

login-key-chooser-title = Archivo de clave privada
login-key-chooser-choose = _Elegir
login-chooser-export = _Exportar
login-chooser-import = _Importar
login-chooser-cancel = _Cancelar
login-chooser-filter = Exportaciones de sitios (*.json)

login-unfinished-tab = Sin terminar
login-unfinished-tab-count = Sin terminar ({ $count })
login-unfinished-one-site = Marque transferencias de un solo sitio (o de un solo par de sitios) a la vez.
login-unfinished-site-gone = Su sitio se eliminó, así que no puede reanudarse. Descartar la olvida (elimine los archivos parciales a mano).
login-unfinished-folder = { $folder } ({ $place })
login-unfinished-what =
    { $kind ->
        [move] Mover { $subject }
        [delete] Eliminar { $subject }
       *[copy] Copiar { $subject }
    }
login-unfinished-saved-unknown = Desconocido (guardado por una versión anterior)

login-title = Iniciar sesión
login-new-folder-tooltip = Nueva carpeta (en la carpeta elegida). Arrastre sitios y carpetas sobre una carpeta para moverlos a ella.
login-rename = Renombrar
login-edit = Editar
login-session = Sesión
login-alias = Alias:
login-alias-tooltip = Un nombre para el sitio, que se muestra en la lista de sitios y en la pestaña de la sesión; si se deja en blanco, se muestra la dirección
login-protocol = Protocolo de archivos:
login-encryption-tooltip = Cómo se cifra la conexión FTP
login-passive = Modo pasivo
login-passive-tooltip = El cliente abre las conexiones de datos (funciona a través de cortafuegos y NAT). Desmárquelo para el modo activo.
login-host = Nombre del host:
login-port = Puerto:
login-user = Nombre de usuario:
login-password = Contraseña:
login-show-password = Mostrar la contraseña
login-save-password = Guardar la contraseña
login-save-password-tooltip = Guardar la contraseña en el llavero del escritorio (cifrada con su contraseña de sesión)
login-key-file = Archivo de clave privada:
login-key-file-placeholder = Ninguno (clave OpenSSH o PuTTY .ppk)
login-browse = Examinar...
login-browse-tooltip = Elegir un archivo de clave privada
login-use-agent = Usar el agente SSH
login-use-agent-tooltip = Probar también las claves del agente SSH del escritorio (como hace ssh)
login-description = Descripción:
login-advanced = Avanzado
login-files-at-once-tooltip = Archivos copiados a la vez. Más puede ser más rápido cuando el servidor limita la velocidad de cada conexión; FTP abre una conexión para cada uno, y algunos servidores solo permiten unas pocas.
login-files-at-once = Archivos a la vez:
login-save = Guardar
login-tab-direct = Directo
login-site-to-site-intro = Las copias entre los dos servidores pasan por este equipo. Elija un sitio guardado para cada conexión y luego Iniciar sesión.
login-connection-1 = Conexión 1
login-connection-1-tooltip = Se muestra en el panel izquierdo
login-detail-protocol = Protocolo:
login-detail-address = Dirección:
login-detail-user = Usuario:
login-detail-login = Inicio de sesión:
login-connection-2 = Conexión 2
login-connection-2-tooltip = Se muestra en el panel derecho
login-tab-site-to-site = De sitio a sitio
login-unfinished-intro = Transferencias que no terminaron. Marque las que quiera reanudar: { -app-name } inicia sesión en su sitio con su contraseña o claves guardadas y lo abre donde estaban. Descartar elimina en cambio sus archivos parciales.
login-detail-transfer = Transferencia:
login-detail-from = Desde:
login-detail-to = Hacia:
login-detail-saved = Guardada:
login-discard-tooltip = Iniciar sesión, eliminar los archivos parciales de las transferencias marcadas y olvidarlas
login-resume-tooltip = Iniciar sesión y continuar las transferencias marcadas donde se detuvieron
login-show-at-startup = Mostrar al iniciar
login-show-at-startup-tooltip = Abrir esta ventana al iniciar { -app-name }
login-import = Importar...
login-import-tooltip = Añadir sitios desde un archivo de exportación de { -app-name }
login-export = Exportar...
login-export-tooltip = Guardar todos los sitios en un archivo para otro equipo
login-login-tooltip = Conectarse al sitio elegido
login-files-at-once-usual = (normalmente { $count })

login-unfinished-none = No hay transferencias sin terminar
login-unfinished-notice =
    { $count ->
        [one] { $count } transferencia no terminó: vea la pestaña Sin terminar
       *[other] { $count } transferencias no terminaron: vea la pestaña Sin terminar
    }
login-unfinished-tick-tooltip = Reanudar o descartar esta transferencia
login-folder-open = Abrir carpeta
login-folder-close = Cerrar carpeta
login-password-saved-tooltip = Contraseña guardada
login-rename-folder = Renombrar carpeta
login-rename-folder-prompt = Nuevo nombre para la carpeta { $folder }:
login-new-folder-prompt = Nombre de la nueva carpeta:
login-new-folder-prompt-in = Nombre de la nueva carpeta en { $folder }:
login-delete-folder = Eliminar carpeta
login-delete-folder-question = ¿Eliminar la carpeta { $folder }?
login-delete-folder-to-top = Los sitios y carpetas que contiene suben al principio de la lista. No se elimina ningún sitio.
login-delete-folder-to-parent = Los sitios y carpetas que contiene pasan a { $folder }. No se elimina ningún sitio.
login-delete-site = Eliminar sitio
login-delete-site-question = ¿Eliminar { $name }?
login-delete-site-with-password = Se eliminarán el sitio y su contraseña guardada (en el llavero). Esta acción no se puede deshacer.
login-delete-site-detail = Se eliminará el sitio. Esta acción no se puede deshacer.
login-exporting = Exportando...
login-importing = Importando...
login-exported =
    { $count ->
        [one] { $count } sitio exportado ({ $passwords } con contraseña) a { $file }
       *[other] { $count } sitios exportados ({ $passwords } con contraseña) a { $file }
    }
login-exported-unreadable = ; no se pudieron leer { $count } contraseñas
login-export-failed = Error al exportar: { $error }
login-imported =
    { $count ->
        [one] { $count } sitio importado ({ $added } nuevos, { $replaced } actualizados, { $passwords } con contraseña)
       *[other] { $count } sitios importados ({ $added } nuevos, { $replaced } actualizados, { $passwords } con contraseña)
    }
login-imported-unstored = ; no se pudieron guardar { $count } contraseñas
login-import-failed = Error al importar: { $error }
login-not-discarded = No descartada: { $reason }
login-discarded =
    { $count ->
        [one] { $count } transferencia descartada: { $deleted ->
            [one] { $deleted } archivo parcial eliminado
           *[other] { $deleted } archivos parciales eliminados
        }
       *[other] { $count } transferencias descartadas: { $deleted ->
            [one] { $deleted } archivo parcial eliminado
           *[other] { $deleted } archivos parciales eliminados
        }
    }
login-password-removed = Contraseña guardada eliminada
login-keyring-error = Llavero: { $error }
login-password-kept = Contraseña no guardada ({ $error }); se queda en el campo para este inicio de sesión
login-pair-needs-password = { $name } necesita una contraseña: guárdela en la pestaña Directo, o use claves SSH

login-waiting-key = Esperando a que compruebe la clave del servidor
login-waiting-certificate = Esperando a que compruebe el certificado del servidor
login-waiting-passphrase = Esperando la frase de contraseña de la clave
login-trust = Confiar y conectar
login-trust-key = Servidor desconocido
login-trust-key-question = ¿Confiar en el servidor { $host }?
login-trust-key-detail =
    Es la primera conexión a este servidor, así que aún no se puede comprobar su identidad. La huella de su clave de host es:

    { $fingerprint }

    Si coincide con la clave real del servidor, confíe en ella: se recordará en ~/.ssh/known_hosts.
login-trust-certificate = Certificado no fiable
login-trust-certificate-question = ¿Confiar en el certificado de { $host }?
login-trust-certificate-detail =
    No se puede verificar el certificado TLS del servidor: { $reason }. Los servidores FTP suelen usar un certificado autofirmado. Su huella es:

    { $fingerprint }

    Si coincide con el certificado real del servidor, confíe en él: se recordará para este servidor.

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

log-level-error = Error
log-level-warn = Advertencia
log-level-info = Información
log-level-debug = Depuración
log-level-trace = Rastreo

ftp-encryption-explicit-if-available = Usar FTP sobre TLS explícito si está disponible
ftp-encryption-explicit = Exigir FTP sobre TLS explícito
ftp-encryption-implicit = Exigir FTP sobre TLS implícito
ftp-encryption-none = Usar solo FTP sin cifrar (inseguro)

job-title =
    { $action ->
        [upload] Subir { $what } a { $dest }
        [download] { $dest }escargar { $what } a { $dest }
        [move-to-server] Mover { $what } al servidor, { $dest }
        [move-to-computer] Mover { $what } a este equipo, { $dest }
        [copy-here] Copiar { $what } a { $dest }
        [copy-on-server] Copiar { $what } a { $dest } en el servidor
        [move-here] Mover { $what } a { $dest }
        [move-on-server] Mover { $what } a { $dest } en el servidor
        [copy-other-server] Copiar { $what } a { $dest } en el otro servidor
        [move-other-server] Mover { $what } a { $dest } en el otro servidor
        [delete-here] Eliminar { $what } de este equipo
       *[delete-on-server] Eliminar { $what } del servidor
    }
job-items =
    { $count ->
        [one] { $count } elemento
       *[other] { $count } elementos
    }

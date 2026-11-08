# Os textos da interface do ShuttleSCP em português do Brasil. Tradução automática
# (IA), ainda não revisada por um falante de português: correções são bem-vindas (veja
# o README). Mesmos nomes de mensagens que i18n/en-US/shuttlescp.ftl.
#
# Sintaxe do Fluent: https://projectfluent.org/fluent/guide/

-app-name = ShuttleSCP

language-name = Português do Brasil (tradução automática)

dialog-close = Fechar

preferences-title = Preferências
preferences-language = _Idioma da interface:
preferences-restart = Reinicie o { -app-name } para usar o novo idioma.

status-transfers-unlimited = Transferências: sem limite
status-transfers-limited = Transferências: { $running } de { $limit }
status-transfers-waiting = { $count } aguardando
dialog-cancel = Cancelar

quit-title = Sair
quit-question = Sair do { -app-name }?
quit-detail = { $unfinished } Mantenha-as para retomá-las na próxima vez que se conectar, ou exclua os arquivos parciais.
quit-running =
    { $count ->
        [one] { $count } transferência ainda está em andamento.
       *[other] { $count } transferências ainda estão em andamento.
    }
quit-paused =
    { $count ->
        [one] { $count } transferência está pausada.
       *[other] { $count } transferências estão pausadas.
    }
quit-unfinished = { $count } transferências não terminaram ({ $running } em andamento, { $paused } pausadas).
quit-discard = Excluir arquivos parciais e sair
quit-keep = Manter para depois e sair

passphrase-export-title = Exportar sites
passphrase-import-title = Importar sites
passphrase-export-explain = As senhas salvas são criptografadas com uma frase secreta que você escolhe. Você vai precisar dela para importar o arquivo em outro computador.
passphrase-import-explain = Este arquivo contém senhas criptografadas. Digite a frase secreta usada para exportá-lo.
passphrase-include = Incluir as senhas salvas
passphrase-new = Frase secreta (pelo menos 8 caracteres)
passphrase-field = Frase secreta
passphrase-repeat = Repita a frase secreta
passphrase-export = Exportar
passphrase-import = Importar

key-passphrase-title = Frase secreta da chave
key-passphrase-question = A chave { $key } está protegida por uma frase secreta
key-passphrase-wrong = Essa frase secreta não abre a chave. Tente novamente.
key-passphrase-explain = Digite a frase secreta dela para entrar com ela.
key-passphrase-save = Salvar a frase secreta no chaveiro
key-passphrase-unlock = Desbloquear

conflict-title = O arquivo já existe
conflict-question = { $name } já existe em { $folder }
conflict-existing = Existente:
conflict-new = Novo:
conflict-for-all = Fazer o mesmo com os outros arquivos existentes
conflict-cancel = Cancelar transferência
conflict-skip = Pular
conflict-overwrite = Substituir
conflict-newer = {" "}(mais recente)
key-passphrase-save-tooltip = Guardá-la no chaveiro da área de trabalho (criptografada com sua senha de login), para que a chave abra sem perguntar

update-restart-title = Reiniciar
update-restart-question = Reiniciar com o { -app-name } { $version }?
update-restart-running =
    { $count ->
        [one] { $count } transferência ainda está em andamento.
       *[other] { $count } transferências ainda estão em andamento.
    }
update-restart-detail = { $running } O { -app-name } pode reiniciar sozinho quando elas terminarem, ou agora: elas param e poderão ser retomadas na próxima vez que você se conectar ao servidor.
update-later = Mais tarde
update-restart-now = Reiniciar agora
update-when-finished = Quando as transferências terminarem
update-check-title = Verificar atualizações
update-up-to-date = O { -app-name } está atualizado
update-latest = A versão { $version } é a mais recente.
update-check-failed = Não é possível verificar atualizações
update-check-error = { $error }.
update-available-title = Atualização disponível
update-available = O { -app-name } { $version } está disponível
update-you-have = Você tem a versão { $current }. A nova versão foi lançada em { $date }.
update-whats-new = Novidades da versão { $version }
update-mac-steps = Baixe-a, abra a imagem de disco e arraste o { -app-name } para Aplicativos, substituindo esta versão.
update-download = Baixar
update-from-page = Baixe a nova versão na página do { -app-name }.
update-open-page = Abrir a página de download
update-downloading = Baixando...
update-install = Instalar e reiniciar
update-downloading-progress = Baixando... { $done } de { $total }
update-failed = { $error }. Você pode baixá-la na página do { -app-name }.

menu-file = _Arquivo
menu-edit = _Editar
menu-view = E_xibir
menu-tools = _Ferramentas
menu-window = _Janela
menu-help = Aj_uda
menu-login = _Entrar…
menu-disconnect = _Desconectar
menu-preferences = _Preferências…
menu-settings = _Ajustes…
menu-quit = _Sair
menu-theme = Tema
menu-log-level = _Nível de registro
menu-open-log = _Abrir o arquivo de registro
menu-check-updates = _Verificar atualizações…
menu-auto-updates = Verificar ao _iniciar
menu-about = _Sobre o { -app-name }

edit-undo = Desfazer
edit-redo = Refazer
edit-cut = Recortar
edit-copy = Copiar
edit-paste = Colar
edit-delete = Excluir
edit-select-all = Selecionar tudo

not-connected = Não conectado
new-connection = Nova conexão
new-connection-tooltip = Conectar a outro servidor em uma nova aba
this-computer-title = Este computador · { $name }
this-computer = este computador
status-connected = Conectado a { $name }
status-connected-pair = Conectado a { $left } e { $right }
status-transfers-tooltip = Transferências simultâneas, em todas as conexões. Clique para limitá-las.
status-log-level = Nível de registro: { $level }
session-activity = Transferências em andamento
session-close = Desconectar e fechar esta sessão
close-session-title = Fechar sessão
close-session-question = Fechar a sessão?
stop-transfers-detail =
    { $count ->
        [one] { $count } transferência ainda está em andamento. Ela vai parar e poderá ser retomada na próxima vez que você se conectar a este servidor.
       *[other] { $count } transferências ainda estão em andamento. Elas vão parar e poderão ser retomadas na próxima vez que você se conectar a este servidor.
    }
delete-title = Excluir
delete-detail = As pastas são excluídas com todo o seu conteúdo. Isso não pode ser desfeito.

update-notice-available = Atualização disponível: { $version }
update-notice-waiting = A atualização aguarda as transferências
update-notice-restart = Reinicie para concluir a atualização
update-notice-available-tooltip = O { -app-name } { $version } está disponível (você tem a { $current }). Clique para ver.
update-notice-waiting-tooltip = O { -app-name } vai reiniciar com a { $version } quando as transferências em andamento terminarem. Clique para reiniciar agora ou mais tarde.
update-notice-restart-tooltip = O { -app-name } { $version } está pronto. Clique para reiniciar com ele.
update-notice-hide = Ocultar este aviso

limit-title = Transferências simultâneas
limit-unlimited = Sem limite
limit-at-most = No máximo
limit-hint = Em todas as conexões. As demais aguardam na fila e começam à medida que outras terminam.

relay-tooltip = Os arquivos copiados entre os dois servidores passam por este computador
relay-to-right = Copiando do servidor da esquerda para o da direita
relay-to-left = Copiando do servidor da direita para o da esquerda
relay-receiving = Recebendo do servidor de origem das cópias
relay-sending = Enviando ao servidor de destino das cópias

panel-log = Registro
list-queue = Fila
list-completed = Concluídas
list-failed = Com falha
list-queue-tooltip = Transferências
list-completed-tooltip = Transferências concluídas
list-failed-tooltip = Transferências com falha
list-for-connection = { $list } de { $name }
list-not-connected = { $list } (não conectado)
log-unavailable = O registro em arquivo não está disponível, então não há nada para mostrar.

theme-system = Padrão do sistema
theme-swatch = Cores da janela e de destaque
disk-tooltip = Disco

about-title = Sobre o { -app-name }
about-version = Versão { $version } ({ $revision })
about-description = Um cliente gráfico de SFTP, SCP e FTP para Linux, macOS e Windows, inspirado no WinSCP.
about-authors = Por { $authors }
about-license = Software livre: { $license }
about-system-details = Detalhes do sistema
about-copy-details = Copiar detalhes

pane-gathering = Coletando informações...
pane-gathering-tooltip = Coletando informações
pane-saved-gathering = Lista salva de { $saved } · coletando informações...
pane-saved-stale = Lista salva de { $saved }; não foi possível verificar alterações: { $error }
pane-refresh-tooltip = Atualizar
pane-items =
    { $count ->
        [one] { $count } item
       *[other] { $count } itens
    }
pane-created = Pasta { $name } criada
pane-renamed = Renomeado para { $name }
pane-new-folder-title = Nova pasta
pane-new-folder-prompt = Criar uma pasta em { $folder }:
pane-new-folder-create = Criar
pane-new-folder-name = Nova pasta
pane-rename-title = Renomear
pane-rename-prompt = Novo nome para { $name }:
pane-open-title = Abrir arquivo
pane-open-failed = Não é possível abrir { $name }
pane-open-error = { $error }.
pane-column-name = Nome
pane-column-size = Tamanho
pane-column-modified = Modificado

pane-transfer =
    { $kind ->
        [upload] _Enviar
        [download] _Baixar
       *[copy] _Copiar
    }
pane-transfer-to =
    { $kind ->
        [upload] _Enviar para { $name }
        [download] _Baixar para { $name }
       *[copy] _Copiar para { $name }
    }
pane-move = _Mover
pane-move-to = _Mover para { $name }
pane-menu-new-folder = _Nova pasta…
pane-menu-rename = _Renomear…
pane-menu-delete = E_xcluir
pane-menu-refresh = A_tualizar

pane-drop-copy = _Copiar aqui
pane-drop-move = _Mover aqui
pane-drop-cancel = C_ancelar

transfers-resume = Retomar
transfers-discard = Descartar
transfers-discard-tooltip = Esquecê-las; os arquivos parciais ficam onde estão
transfers-hide-finished = Ocultar arquivos concluídos
transfers-hide-finished-tooltip = Tirar da lista os arquivos copiados sem problemas, para que os que estão sendo copiados fiquem no topo
transfers-queue-empty = Fila vazia
transfers-completed-empty = Nenhuma transferência concluída
transfers-failed-empty = Nenhuma transferência com falha
transfers-clear = Limpar
transfers-interrupted =
    { $count ->
        [one] { $count } transferência foi interrompida e pode ser retomada.
       *[other] { $count } transferências foram interrompidas e podem ser retomadas.
    }
transfers-blocked = Não iniciada: "{ $other }" está usando os mesmos arquivos. Tente novamente quando ela terminar.

progress-percent = { $percent }%
progress-left = faltam { $time }
progress-speed = { $size }/s
progress-files = { $done } de { $total } arquivos
progress-bytes = { $done } de { $total }

job-waiting-slot = Aguardando uma vaga de transferência livre
job-pausing = Pausando...
job-paused = Pausada
job-paused-copied = Pausada: { $done } de { $total } copiados
job-preparing = Preparando...
job-deleting = { $done } de { $total } excluídos
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
job-resumed = { $title } (retomada)
job-cancel = Cancelar toda a transferência
job-cancel-paused = Cancelar a transferência pausada
job-pause = Pausar: parar por enquanto, mantendo o que foi copiado
job-resume = Retomar: continuar de onde foi pausada
job-start-now = Iniciar agora
job-retry = Tentar de novo: continuar de onde parou
job-remove = Remover da lista

summary-separator = {", "}
summary-failed = { $done } concluídos, { $failed } com falha
summary-cancelled-job = Cancelada: { $done } concluídos
summary-deleted =
    { $count ->
        [one] { $count } item excluído
       *[other] { $count } itens excluídos
    }
summary-copied =
    { $count ->
        [one] { $count } item · { $size }
       *[other] { $count } itens · { $size }
    }
summary-cancelled = { $count } cancelados
summary-skipped = { $count } pulados
summary-stopped = { $count } interrompidos

item-deleted = Excluído
item-skipped = Pulado: o arquivo existente foi mantido
item-paused = Pausado
item-paused-slot = Pausado: aguardando uma vaga de transferência livre
item-paused-part = Pausado: { $done } de { $total }
item-cancelled = Cancelado
item-stopped = Interrompido
item-stopped-delete = Interrompido: não excluído
item-asking = Já existe: aguardando sua resposta
item-waiting = Aguardando
item-stop-file = Interromper este arquivo; o resto continua
item-stop-folder = Interromper esta pasta; o resto continua
item-pause-file = Pausar este arquivo; o resto continua
item-pause-folder = Pausar os arquivos desta pasta; o resto continua
item-resume-file = Retomar este arquivo
item-resume-folder = Retomar esta pasta

login-new-site = Novo site
login-alias-optional = Opcional
login-connecting = Conectando a { $name }...
login-deleted-site = (site excluído)
login-forgotten = Esquecidas
login-deleting-partials = Excluindo arquivos parciais...
login-save-failed = Não é possível salvar os sites: { $error }
login-folder-exists = Já existe uma pasta "{ $folder }"
login-folder-move-failed = Não é possível mover "{ $folder }": { $error }

login-password-replace = Salva - digite para substituir
login-password-dots = •••••••• (salva)
login-password-not-saved = Não salva
login-password-saved = Senha salva no chaveiro
login-password-none = Nenhuma senha salva
login-password-saving = Salvando a senha no chaveiro...

login-ftp-encryption = FTP · { $encryption }
login-way-key = arquivo de chave { $key }
login-way-agent = agente SSH
login-way-password = senha salva
login-way-none = nenhuma senha salva
login-way-then = {", depois "}

login-key-chooser-title = Arquivo de chave privada
login-key-chooser-choose = _Escolher
login-chooser-export = _Exportar
login-chooser-import = _Importar
login-chooser-cancel = _Cancelar
login-chooser-filter = Exportações de sites (*.json)

login-unfinished-tab = Inacabadas
login-unfinished-tab-count = Inacabadas ({ $count })
login-unfinished-one-site = Marque transferências de um só site (ou de um só par de sites) por vez.
login-unfinished-site-gone = O site dela foi excluído, então ela não pode ser retomada. Descartar a esquece (exclua os arquivos parciais manualmente).
login-unfinished-folder = { $folder } ({ $place })
login-unfinished-what =
    { $kind ->
        [move] Mover { $subject }
        [delete] Excluir { $subject }
       *[copy] Copiar { $subject }
    }
login-unfinished-saved-unknown = Desconhecido (salvo por uma versão anterior)

login-title = Entrar
login-new-folder-tooltip = Nova pasta (na pasta escolhida). Arraste sites e pastas para uma pasta para movê-los para ela.
login-rename = Renomear
login-edit = Editar
login-session = Sessão
login-alias = Apelido:
login-alias-tooltip = Um nome para o site, exibido na lista de sites e na aba da sessão; em branco, o endereço é exibido
login-protocol = Protocolo de arquivos:
login-encryption-tooltip = Como a conexão FTP é criptografada
login-passive = Modo passivo
login-passive-tooltip = O cliente abre as conexões de dados (funciona através de firewalls e NAT). Desmarque para o modo ativo.
login-host = Nome do host:
login-port = Porta:
login-user = Nome de usuário:
login-password = Senha:
login-show-password = Mostrar a senha
login-save-password = Salvar a senha
login-save-password-tooltip = Guardar a senha no chaveiro da área de trabalho (criptografada com sua senha de login)
login-key-file = Arquivo de chave privada:
login-key-file-placeholder = Nenhum (chave OpenSSH ou PuTTY .ppk)
login-browse = Procurar...
login-browse-tooltip = Escolher um arquivo de chave privada
login-use-agent = Usar o agente SSH
login-use-agent-tooltip = Tentar também as chaves do agente SSH da área de trabalho (como o ssh faz)
login-description = Descrição:
login-advanced = Avançado
login-files-at-once-tooltip = Arquivos copiados ao mesmo tempo. Mais pode ser mais rápido quando o servidor limita a velocidade de cada conexão; o FTP abre uma conexão para cada um, e alguns servidores permitem só algumas.
login-files-at-once = Arquivos por vez:
login-save = Salvar
login-tab-direct = Direto
login-site-to-site-intro = As cópias entre os dois servidores passam por este computador. Escolha um site salvo para cada conexão e depois Entrar.
login-connection-1 = Conexão 1
login-connection-1-tooltip = Exibida no painel da esquerda
login-detail-protocol = Protocolo:
login-detail-address = Endereço:
login-detail-user = Usuário:
login-detail-login = Login:
login-connection-2 = Conexão 2
login-connection-2-tooltip = Exibida no painel da direita
login-tab-site-to-site = Site a site
login-unfinished-intro = Transferências que não terminaram. Marque as que quer retomar: o { -app-name } entra no site delas com a senha ou as chaves salvas e o abre onde elas estavam. Descartar exclui os arquivos parciais delas em vez disso.
login-detail-transfer = Transferência:
login-detail-from = De:
login-detail-to = Para:
login-detail-saved = Salva:
login-discard-tooltip = Entrar, excluir os arquivos parciais das transferências marcadas e esquecê-las
login-resume-tooltip = Entrar e continuar as transferências marcadas de onde pararam
login-show-at-startup = Mostrar ao iniciar
login-show-at-startup-tooltip = Abrir esta janela quando o { -app-name } iniciar
login-import = Importar...
login-import-tooltip = Adicionar sites de um arquivo de exportação do { -app-name }
login-export = Exportar...
login-export-tooltip = Salvar todos os sites em um arquivo para outro computador
login-login-tooltip = Conectar ao site escolhido
login-files-at-once-usual = (normalmente { $count })
login-verify = Verificar somas de verificação:
login-verify-tooltip = Se as somas de verificação são conferidas após cada transferência de ou para este servidor. Somente somas do servidor: só onde o servidor consegue calculá-las, nunca relendo o arquivo (melhor para servidores lentos).
login-verify-same = Igual às Preferências
login-verify-checksums = Somente somas do servidor
login-verify-off = Desativado
preferences-verify = _Verificar as somas de verificação ao concluir
preferences-read-back = _Reler arquivos de até:
preferences-read-back-note = Se um servidor não consegue calcular somas de verificação, o arquivo é relido para verificá-lo: uma segunda transferência.
read-back-never = Nunca
read-back-any-size = Qualquer tamanho

login-unfinished-none = Nenhuma transferência inacabada
login-unfinished-notice =
    { $count ->
        [one] { $count } transferência não terminou: veja a aba Inacabadas
       *[other] { $count } transferências não terminaram: veja a aba Inacabadas
    }
login-unfinished-tick-tooltip = Retomar ou descartar esta transferência
login-folder-open = Abrir pasta
login-folder-close = Fechar pasta
login-password-saved-tooltip = Senha salva
login-rename-folder = Renomear pasta
login-rename-folder-prompt = Novo nome para a pasta { $folder }:
login-new-folder-prompt = Nome da nova pasta:
login-new-folder-prompt-in = Nome da nova pasta em { $folder }:
login-delete-folder = Excluir pasta
login-delete-folder-question = Excluir a pasta { $folder }?
login-delete-folder-to-top = Os sites e pastas dentro dela sobem para o topo da lista. Nenhum site é excluído.
login-delete-folder-to-parent = Os sites e pastas dentro dela passam para { $folder }. Nenhum site é excluído.
login-delete-site = Excluir site
login-delete-site-question = Excluir { $name }?
login-delete-site-with-password = O site e a senha salva dele (no chaveiro) serão removidos. Isso não pode ser desfeito.
login-delete-site-detail = O site será removido. Isso não pode ser desfeito.
login-exporting = Exportando...
login-importing = Importando...
login-exported =
    { $count ->
        [one] { $count } site exportado ({ $passwords } com senha) para { $file }
       *[other] { $count } sites exportados ({ $passwords } com senha) para { $file }
    }
login-exported-unreadable = ; não foi possível ler { $count } senhas
login-export-failed = Falha na exportação: { $error }
login-imported =
    { $count ->
        [one] { $count } site importado ({ $added } novos, { $replaced } atualizados, { $passwords } com senha)
       *[other] { $count } sites importados ({ $added } novos, { $replaced } atualizados, { $passwords } com senha)
    }
login-imported-unstored = ; não foi possível salvar { $count } senhas
login-import-failed = Falha na importação: { $error }
login-not-discarded = Não descartada: { $reason }
login-discarded =
    { $count ->
        [one] { $count } transferência descartada: { $deleted ->
            [one] { $deleted } arquivo parcial excluído
           *[other] { $deleted } arquivos parciais excluídos
        }
       *[other] { $count } transferências descartadas: { $deleted ->
            [one] { $deleted } arquivo parcial excluído
           *[other] { $deleted } arquivos parciais excluídos
        }
    }
login-password-removed = Senha salva removida
login-keyring-error = Chaveiro: { $error }
login-password-kept = Senha não salva ({ $error }); ela fica no campo para este login
login-pair-needs-password = { $name } precisa de uma senha: salve-a na aba Direto, ou use chaves SSH

login-waiting-key = Aguardando você verificar a chave do servidor
login-waiting-certificate = Aguardando você verificar o certificado do servidor
login-waiting-passphrase = Aguardando a frase secreta da chave
login-trust = Confiar e conectar
login-trust-key = Servidor desconhecido
login-trust-key-question = Confiar no servidor { $host }?
login-trust-key-detail =
    Esta é a primeira conexão com este servidor, então a identidade dele ainda não pode ser verificada. A impressão digital da chave de host dele é:

    { $fingerprint }

    Se ela corresponder à chave real do servidor, confie nela: ela será lembrada em ~/.ssh/known_hosts.
login-trust-certificate = Certificado não confiável
login-trust-certificate-question = Confiar no certificado de { $host }?
login-trust-certificate-detail =
    O certificado TLS do servidor não pode ser verificado: { $reason }. Servidores FTP costumam usar um certificado autoassinado. A impressão digital dele é:

    { $fingerprint }

    Se ele corresponder ao certificado real do servidor, confie nele: ele será lembrado para este servidor.

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

log-level-error = Erro
log-level-warn = Aviso
log-level-info = Informação
log-level-debug = Depuração
log-level-trace = Rastreamento

ftp-encryption-explicit-if-available = Usar FTP sobre TLS explícito se disponível
ftp-encryption-explicit = Exigir FTP sobre TLS explícito
ftp-encryption-implicit = Exigir FTP sobre TLS implícito
ftp-encryption-none = Usar só FTP sem criptografia (inseguro)

job-title =
    { $action ->
        [upload] Enviar { $what } para { $dest }
        [download] Baixar { $what } para { $dest }
        [move-to-server] Mover { $what } para o servidor, { $dest }
        [move-to-computer] Mover { $what } para este computador, { $dest }
        [copy-here] Copiar { $what } para { $dest }
        [copy-on-server] Copiar { $what } para { $dest } no servidor
        [move-here] Mover { $what } para { $dest }
        [move-on-server] Mover { $what } para { $dest } no servidor
        [copy-other-server] Copiar { $what } para { $dest } no outro servidor
        [move-other-server] Mover { $what } para { $dest } no outro servidor
        [delete-here] Excluir { $what } deste computador
       *[delete-on-server] Excluir { $what } do servidor
    }
job-items =
    { $count ->
        [one] { $count } item
       *[other] { $count } itens
    }

error-cannot =
    { $action ->
        [read] não é possível ler { $path }: { $reason }
        [open] não é possível abrir { $path }: { $reason }
        [write] não é possível gravar { $path }: { $reason }
        [create] não é possível criar { $path }: { $reason }
        [create-folder] não é possível criar a pasta { $path }: { $reason }
        [delete] não é possível excluir { $path }: { $reason }
        [delete-folder] não é possível excluir a pasta { $path }: { $reason }
        [set-time] não é possível ajustar a hora de { $path }: { $reason }
        [finish] não é possível concluir { $path }: { $reason }
        [download] não é possível baixar { $path }: { $reason }
        [replace] não é possível substituir { $path }: { $reason }
        [access] não é possível acessar { $path }: { $reason }
       *[make-runnable] não é possível tornar { $path } executável: { $reason }
    }
error-cannot-bare =
    { $action ->
        [read] não é possível ler { $path }
        [open] não é possível abrir { $path }
        [write] não é possível gravar { $path }
        [create] não é possível criar { $path }
        [create-folder] não é possível criar a pasta { $path }
        [delete] não é possível excluir { $path }
        [delete-folder] não é possível excluir a pasta { $path }
        [set-time] não é possível ajustar a hora de { $path }
        [finish] não é possível concluir { $path }
        [download] não é possível baixar { $path }
        [replace] não é possível substituir { $path }
        [access] não é possível acessar { $path }
       *[make-runnable] não é possível tornar { $path } executável
    }
error-cannot-rename = não é possível renomear { $from } para { $to }: { $reason }
error-cannot-rename-bare = não é possível renomear { $from } para { $to }
error-no-such-item = arquivo ou pasta inexistente
error-sftp-date = { $path } tem uma data que o SFTP não consegue gravar
checksum-unsupported = o servidor não consegue calcular somas de verificação
checksum-no-answer = a resposta do servidor não continha nenhuma soma de verificação
verify-mismatch = a soma de verificação da cópia difere da do original: a cópia não é o que foi copiado
verify-skipped = ignorada pelo usuário
item-verifying = Verificando…
item-verified = { $size } · verificado
item-not-verified = { $size } · não verificado
item-not-verified-large = { $size } · não verificado (arquivo grande)
item-skip-check = Pular a verificação
item-verify-now = Verificar agora (relê o arquivo)
connect-unsupported = conexões { $protocol } ainda não são suportadas
connect-timeout = { $host } não respondeu (tempo esgotado)
connect-network = não é possível conectar a { $host }: { $reason }
connect-unknown-host-key = a chave de host do servidor ainda não é conhecida ({ $fingerprint })
connect-host-key-changed = ATENÇÃO: a chave de host de { $host } mudou (linha { $line } do known_hosts). Alguém pode estar interceptando a conexão; a conexão foi recusada.
connect-untrusted-certificate = o certificado do servidor não é confiável ({ $reason })
connect-password-required = digite a senha
connect-keys-rejected = o servidor não aceitou a chave SSH; digite a senha ou verifique a chave
connect-key-passphrase-required = o arquivo de chave { $path } precisa da frase secreta
connect-key-unreadable = não é possível usar o arquivo de chave { $path }: { $reason }
connect-authentication-failed = falha no login: nome de usuário, chave ou senha incorretos
connect-sftp = o serviço de transferência de arquivos (SFTP) do servidor falhou: { $reason }
connect-ftp = FTP: { $reason }
connect-host-certificates = certificados de host não são suportados
connect-ftp-home = não é possível ler a pasta pessoal: { $reason }
connect-ftp-no-tls = o servidor não oferece criptografia TLS ({ $reply }); escolha "{ $choice }" para conectar sem exigi-la
connect-tls-setup = falha na configuração do TLS: { $reason }
connect-no-authorities = este sistema não tem autoridades de certificação confiáveis
certificate-problem =
    { $reason ->
        [unknown-issuer] ele não é assinado por uma autoridade em que este computador confia
        [expired] ele expirou
        [not-valid-yet] ele ainda não é válido
        [other-name] ele foi emitido para outro nome de servidor
        [revoked] ele foi revogado
       *[self-signed] ele é autoassinado
    }
name-empty = digite um nome
name-reserved = "{ $name }" não pode ser usado como nome
name-forbidden-character = um nome não pode conter "{ $character }"
name-null = um nome não pode conter um caractere nulo
name-control-characters = um nome não pode conter caracteres de controle
name-trailing-space-or-dot = um nome não pode terminar com espaço ou ponto
name-windows-device = "{ $name }" é um nome que o Windows reserva para um dispositivo
error-already-exists = { $name } já existe
error-into-itself = { $name } não pode ir para dentro de si mesmo
error-already-there = { $name } já está em { $folder }
error-parent-not-created = a pasta onde ele deve ficar não pôde ser criada
error-folder-there = já existe uma pasta chamada { $name } ali
error-items-inside-failed = alguns itens dentro dela falharam
error-items-inside-not-deleted = alguns itens dentro dela não foram excluídos
error-copied-but = copiado, mas { $reason }
error-not-connected = não conectado a um servidor
error-not-connected-destination = não conectado ao servidor de destino das cópias
error-cannot-copy-root = { $path } não pode ser copiado nem excluído
error-nested-too-deeply = { $path } está aninhado profundamente demais (um laço de links simbólicos?)
update-error-unsigned = a atualização não está assinada com a chave de publicação do { -app-name }
update-error-unreadable = não é possível ler a descrição da atualização: { $reason }
update-error-unexpected-place = a atualização seria baixada de um lugar inesperado: { $url }
update-error-cancelled = o download foi cancelado
update-error-failed = o download falhou: { $reason }
update-error-too-large = o download é maior do que a versão informa
update-error-save = não é possível salvar o download: { $reason }
update-error-incomplete = o download está incompleto ({ $done } de { $total } bytes)
update-error-mismatch = o download não corresponde à versão assinada (SHA-256 diferente)
update-error-no-key = esta compilação não pode verificar atualizações
update-error-bad-key = a chave de publicação embutida está danificada: { $reason }
update-error-unreachable = não é possível acessar a página de versões: { $reason }
update-error-description = não é possível ler a descrição da versão: { $reason }
update-error-version = "{ $version }" não é um número de versão
passphrase-problem-empty = Digite a frase secreta
passphrase-problem-short = Use pelo menos { $count } caracteres
passphrase-problem-mismatch = As frases secretas não coincidem
export-error-json = { $path } não é uma exportação de sites válida: { $reason }
export-error-not-export = o arquivo não é uma exportação de sites do { -app-name }
export-error-version = versão de exportação não suportada: { $version }
export-error-passphrase-required = é preciso uma frase secreta para importar as senhas salvas
export-error-wrong-passphrase = frase secreta incorreta, ou o arquivo foi alterado
export-error-settings = configurações de criptografia inválidas no arquivo: { $reason }
export-error-random = não é possível gerar dados aleatórios: { $reason }
keyring-error-service = erro do chaveiro: { $reason }
keyring-error-not-text = a senha salva não é um texto válido
keyring-error-unsupported = salvar senhas ainda não é suportado neste sistema
settings-error-invalid = configurações inválidas em { $path }: { $reason }
folder-error-into-itself = uma pasta não pode ir para dentro de si mesma
folder-error-exists = já existe uma pasta "{ $folder }"

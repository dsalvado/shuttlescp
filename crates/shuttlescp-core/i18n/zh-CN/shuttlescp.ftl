# ShuttleSCP 界面的简体中文文本。机器翻译（AI），尚未经过中文使用者审校：欢迎改正
# （见 README）。消息名称与 i18n/en-US/shuttlescp.ftl 相同。
#
# Fluent 语法：https://projectfluent.org/fluent/guide/

-app-name = ShuttleSCP

language-name = 简体中文（机器翻译）

dialog-close = 关闭

preferences-title = 首选项
preferences-language = 界面语言(_L)：
preferences-restart = 重新启动 { -app-name } 以使用新语言。

status-transfers-unlimited = 文件：不限
status-transfers-limited = 文件：{ $running } / { $limit }
status-transfers-waiting = { $count } 个等待中
dialog-cancel = 取消

quit-title = 退出
quit-question = 退出 { -app-name }？
quit-detail = { $unfinished }保留它们，以便下次连接时继续；或删除它们的部分文件。
quit-running = { $count } 个传输仍在进行。
quit-paused = { $count } 个传输已暂停。
quit-unfinished = { $count } 个传输未完成（{ $running } 个进行中，{ $paused } 个已暂停）。
quit-discard = 删除部分文件并退出
quit-keep = 保留以后继续并退出

passphrase-export-title = 导出站点
passphrase-import-title = 导入站点
passphrase-export-explain = 保存的密码用你选择的密码短语加密。在另一台计算机上导入该文件时需要它。
passphrase-import-explain = 此文件包含加密的密码。请输入导出时使用的密码短语。
passphrase-include = 包含保存的密码
passphrase-new = 密码短语（至少 8 个字符）
passphrase-field = 密码短语
passphrase-repeat = 再次输入密码短语
passphrase-export = 导出
passphrase-import = 导入

key-passphrase-title = 密钥密码短语
key-passphrase-question = 密钥 { $key } 受密码短语保护
key-passphrase-wrong = 该密码短语无法打开密钥。请重试。
key-passphrase-explain = 输入它的密码短语以使用它登录。
key-passphrase-save = 将密码短语保存到密钥环
key-passphrase-unlock = 解锁

conflict-title = 文件已存在
conflict-question = { $name } 已存在于 { $folder }
conflict-existing = 现有：
conflict-new = 新的：
conflict-for-all = 对其他已存在的文件执行相同操作
conflict-cancel = 取消传输
conflict-skip = 跳过
conflict-overwrite = 覆盖
conflict-newer = {" "}（较新）
key-passphrase-save-tooltip = 保存在桌面密钥环中（用你的登录密码加密），这样打开密钥时无需再询问

update-restart-title = 重新启动
update-restart-question = 重新启动为 { -app-name } { $version }？
update-restart-running = { $count } 个传输仍在进行。
update-restart-detail = { $running }{ -app-name } 可以在它们完成后自动重新启动，或者现在重新启动：它们会停止，下次连接服务器时可以继续。
update-later = 稍后
update-restart-now = 立即重新启动
update-when-finished = 传输完成时
update-check-title = 检查更新
update-up-to-date = { -app-name } 已是最新版本
update-latest = 版本 { $version } 是最新发布的版本。
update-check-failed = 无法检查更新
update-check-error = { $error }。
update-available-title = 有可用更新
update-available = { -app-name } { $version } 可用
update-you-have = 你使用的是版本 { $current }。新版本发布于 { $date }。
update-whats-new = { $version } 版本的新内容
update-mac-steps = 下载后打开磁盘映像，将 { -app-name } 拖到“应用程序”中，替换此版本。
update-download = 下载
update-from-page = 从 { -app-name } 网页下载新版本。
update-open-page = 打开下载页面
update-downloading = 正在下载……
update-install = 安装并重新启动
update-downloading-progress = 正在下载……{ $done }/{ $total }
update-failed = { $error }。你可以改为从 { -app-name } 网页下载。

menu-file = 文件(_F)
menu-edit = 编辑(_E)
menu-view = 查看(_V)
menu-tools = 工具(_T)
menu-window = 窗口(_W)
menu-help = 帮助(_H)
menu-login = 登录(_L)…
menu-disconnect = 断开连接(_D)
menu-preferences = 首选项(_P)…
menu-settings = 设置(_S)…
menu-quit = 退出(_Q)
menu-theme = 主题
menu-log-level = 日志级别(_L)
menu-open-log = 打开日志文件(_O)
menu-check-updates = 检查更新(_U)…
menu-auto-updates = 启动时检查更新(_S)
menu-about = 关于 { -app-name }(_A)

edit-undo = 撤销
edit-redo = 重做
edit-cut = 剪切
edit-copy = 复制
edit-paste = 粘贴
edit-delete = 删除
edit-select-all = 全选

not-connected = 未连接
new-connection = 新建连接
new-connection-tooltip = 在新标签页中连接另一台服务器
this-computer-title = 本机 · { $name }
this-computer = 本机
status-connected = 已连接到 { $name }
status-connected-pair = 已连接到 { $left } 和 { $right }
status-transfers-tooltip = 所有连接中同时复制的文件数。点击以限制。
status-log-level = 日志级别：{ $level }
session-activity = 传输进行中
session-close = 断开连接并关闭此会话
close-session-title = 关闭会话
close-session-question = 关闭会话？
stop-transfers-detail = { $count } 个传输仍在进行。它们会停止，下次连接此服务器时可以继续。
delete-title = 删除
delete-detail = 文件夹会连同其中的所有内容一起删除。此操作无法撤销。

update-notice-available = 有可用更新：{ $version }
update-notice-waiting = 更新正在等待传输完成
update-notice-restart = 重新启动以完成更新
update-notice-available-tooltip = { -app-name } { $version } 可用（你使用的是 { $current }）。点击查看。
update-notice-waiting-tooltip = 正在进行的传输完成后，{ -app-name } 将重新启动为 { $version }。点击以立即或稍后重新启动。
update-notice-restart-tooltip = { -app-name } { $version } 已就绪。点击以重新启动为新版本。
update-notice-hide = 隐藏此通知

limit-title = 同时复制的文件数
limit-unlimited = 不限
limit-at-most = 最多
limit-hint = 所有连接合计。其余文件按顺序等待，先开始的传输优先。

relay-tooltip = 在两台服务器之间复制的文件会经过本机
relay-to-right = 正在从左侧服务器复制到右侧服务器
relay-to-left = 正在从右侧服务器复制到左侧服务器
relay-receiving = 正在从复制来源服务器接收
relay-sending = 正在向复制目标服务器发送

panel-log = 日志
list-queue = 队列
list-completed = 已完成
list-failed = 失败
list-queue-tooltip = 传输
list-completed-tooltip = 已完成的传输
list-failed-tooltip = 失败的传输
list-for-connection = { $name } 的{ $list }
list-not-connected = { $list }（未连接）
log-unavailable = 无法记录到文件，因此没有可显示的内容。

theme-system = 系统默认
theme-swatch = 窗口和强调色
disk-tooltip = 磁盘

about-title = 关于 { -app-name }
about-version = 版本 { $version }（{ $revision }）
about-description = 一款适用于 Linux、macOS 和 Windows 的图形化 SFTP、SCP 和 FTP 客户端，灵感来自 WinSCP。
about-authors = 作者：{ $authors }
about-license = 自由软件：{ $license }
about-system-details = 系统详情
about-copy-details = 复制详情

pane-gathering = 正在收集信息……
pane-gathering-tooltip = 正在收集信息
pane-saved-gathering = { $saved } 保存的列表 · 正在收集信息……
pane-saved-stale = { $saved } 保存的列表；无法检查更改：{ $error }
pane-refresh-tooltip = 刷新
pane-items = { $count } 项
pane-created = 已创建文件夹 { $name }
pane-renamed = 已重命名为 { $name }
pane-new-folder-title = 新建文件夹
pane-new-folder-prompt = 在 { $folder } 中创建文件夹：
pane-new-folder-create = 创建
pane-new-folder-name = 新建文件夹
pane-rename-title = 重命名
pane-rename-prompt = { $name } 的新名称：
pane-open-title = 打开文件
pane-open-failed = 无法打开 { $name }
pane-open-error = { $error }。
pane-column-name = 名称
pane-column-size = 大小
pane-column-modified = 修改时间

pane-transfer =
    { $kind ->
        [upload] 上传(_U)
        [download] 下载(_D)
       *[copy] 复制(_C)
    }
pane-transfer-to =
    { $kind ->
        [upload] 上传到 { $name }(_U)
        [download] 下载到 { $name }(_D)
       *[copy] 复制到 { $name }(_C)
    }
pane-move = 移动(_M)
pane-move-to = 移动到 { $name }(_M)
pane-menu-new-folder = 新建文件夹(_N)…
pane-menu-rename = 重命名(_R)…
pane-menu-delete = 删除(_E)
pane-menu-refresh = 刷新(_F)

pane-drop-copy = 复制到此处(_C)
pane-drop-move = 移动到此处(_M)
pane-drop-cancel = 取消(_A)

transfers-resume = 继续
transfers-discard = 放弃
transfers-discard-tooltip = 不再记录它们；它们的部分文件保留在原处
transfers-hide-finished = 隐藏已完成的文件
transfers-hide-finished-tooltip = 将顺利复制的文件移出列表，使正在复制的文件保持在顶部
transfers-queue-empty = 队列为空
transfers-completed-empty = 没有已完成的传输
transfers-failed-empty = 没有失败的传输
transfers-clear = 清除
transfers-interrupted = { $count } 个传输被中断，可以继续。
transfers-blocked = 未开始：“{ $other }”正在使用相同的文件。请在它完成后重试。

progress-percent = { $percent }%
progress-left = 剩余 { $time }
progress-speed = { $size }/秒
progress-files = { $done }/{ $total } 个文件
progress-bytes = { $done }/{ $total }

job-pausing = 正在暂停……
job-paused = 已暂停
job-paused-copied = 已暂停：已复制 { $done }/{ $total }
job-preparing = 正在准备……
job-deleting = 已删除 { $done }/{ $total }
job-relay-right =
    { $kind ->
        [move] 移动 { $subject }：{ $from } → { $to }
       *[copy] 复制 { $subject }：{ $from } → { $to }
    }
job-relay-left =
    { $kind ->
        [move] 移动 { $subject }：{ $to } ← { $from }
       *[copy] 复制 { $subject }：{ $to } ← { $from }
    }
job-resumed = { $title }（已继续）
job-cancel = 取消整个传输
job-cancel-paused = 取消已暂停的传输
job-pause = 暂停：暂时停止，保留已复制的内容
job-resume = 继续：从暂停处继续
job-start-now = 立即开始
job-retry = 重试：从停止处继续
job-remove = 从列表中移除

summary-separator = {"，"}
summary-failed = { $done } 个完成，{ $failed } 个失败
summary-cancelled-job = 已取消：{ $done } 个完成
summary-deleted = 已删除 { $count } 项
summary-copied = { $count } 项 · { $size }
summary-cancelled = { $count } 个已取消
summary-skipped = { $count } 个已跳过
summary-stopped = { $count } 个已停止

item-deleted = 已删除
item-skipped = 已跳过：保留了现有文件
item-paused = 已暂停
item-paused-slot = 已暂停：正在等待空闲的传输位置
item-paused-part = 已暂停：{ $done }/{ $total }
item-cancelled = 已取消
item-stopped = 已停止
item-stopped-delete = 已停止：未删除
item-asking = 已存在：正在等待你的回答
item-waiting = 等待中
item-stop-file = 停止此文件；其余继续
item-stop-folder = 停止此文件夹；其余继续
item-pause-file = 暂停此文件；其余继续
item-pause-folder = 暂停此文件夹中的文件；其余继续
item-resume-file = 继续此文件
item-resume-folder = 继续此文件夹

login-new-site = 新建站点
login-alias-optional = 可选
login-connecting = 正在连接到 { $name }……
login-deleted-site = （已删除的站点）
login-forgotten = 已不再记录
login-deleting-partials = 正在删除部分文件……
login-save-failed = 无法保存站点：{ $error }
login-folder-exists = 已有名为“{ $folder }”的文件夹
login-folder-move-failed = 无法移动“{ $folder }”：{ $error }

login-password-replace = 已保存——输入以替换
login-password-dots = ••••••••（已保存）
login-password-not-saved = 未保存
login-password-saved = 密码已保存到密钥环
login-password-none = 没有保存的密码
login-password-saving = 正在将密码保存到密钥环……

login-ftp-encryption = FTP · { $encryption }
login-way-key = 密钥文件 { $key }
login-way-agent = SSH 代理
login-way-password = 保存的密码
login-way-none = 没有保存的密码
login-way-then = {"，然后"}

login-key-chooser-title = 私钥文件
login-key-chooser-choose = 选择(_C)
login-chooser-export = 导出(_E)
login-chooser-import = 导入(_I)
login-chooser-filter = 站点导出文件（*.json）

login-unfinished-tab = 未完成
login-unfinished-tab-count = 未完成（{ $count }）
login-unfinished-one-site = 一次只勾选一个站点（或一对站点）的传输。
login-unfinished-site-gone = 它的站点已被删除，因此无法继续。“放弃”会不再记录它（请手动删除部分文件）。
login-unfinished-folder = { $folder }（{ $place }）
login-unfinished-what =
    { $kind ->
        [move] 移动 { $subject }
        [delete] 删除 { $subject }
       *[copy] 复制 { $subject }
    }
login-unfinished-saved-unknown = 未知（由旧版本保存）

login-title = 登录
login-new-folder-tooltip = 新建文件夹（在所选文件夹中）。将站点和文件夹拖到某个文件夹上即可移入其中。
login-rename = 重命名
login-edit = 编辑
login-session = 会话
login-alias = 别名：
login-alias-tooltip = 站点的名称，显示在站点列表和会话标签上；留空则显示地址
login-protocol = 文件协议：
login-encryption-tooltip = FTP 连接的加密方式
login-passive = 被动模式
login-passive-tooltip = 由客户端打开数据连接（可穿过防火墙和 NAT）。取消勾选则使用主动模式。
login-host = 主机名：
login-port = 端口：
login-user = 用户名：
login-password = 密码：
login-show-password = 显示密码
login-save-password = 保存密码
login-save-password-tooltip = 将密码保存在桌面密钥环中（用你的登录密码加密）
login-key-file = 私钥文件：
login-key-file-placeholder = 无（OpenSSH 或 PuTTY .ppk 密钥）
login-browse = 浏览……
login-browse-tooltip = 选择私钥文件
login-use-agent = 使用 SSH 代理
login-use-agent-tooltip = 同时尝试桌面 SSH 代理中的密钥（与 ssh 相同）
login-description = 描述：
login-advanced = 高级
login-files-at-once-tooltip = 同时复制的文件数。服务器限制每个连接的速度时，更多可能更快；FTP 为每个文件打开一个连接，有些服务器只允许少量连接。
login-files-at-once = 同时文件数：
login-save = 保存
login-tab-direct = 直连
login-site-to-site-intro = 两台服务器之间的复制会经过本机。为每个连接选择一个已保存的站点，然后点击“登录”。
login-connection-1 = 连接 1
login-connection-1-tooltip = 显示在左侧窗格中
login-detail-protocol = 协议：
login-detail-address = 地址：
login-detail-user = 用户：
login-detail-login = 登录方式：
login-connection-2 = 连接 2
login-connection-2-tooltip = 显示在右侧窗格中
login-tab-site-to-site = 站点到站点
login-unfinished-intro = 未完成的传输。勾选要继续的传输：{ -app-name } 会用保存的密码或密钥登录它们的站点，并打开到它们中断的位置。“放弃”则会删除它们的部分文件。
login-detail-transfer = 传输：
login-detail-from = 从：
login-detail-to = 到：
login-detail-saved = 保存于：
login-discard-tooltip = 登录，删除所勾选传输的部分文件，并不再记录它们
login-resume-tooltip = 登录，并从停止处继续所勾选的传输
login-show-at-startup = 启动时显示
login-show-at-startup-tooltip = { -app-name } 启动时打开此窗口
login-import = 导入……
login-import-tooltip = 从 { -app-name } 导出文件中添加站点
login-export = 导出……
login-export-tooltip = 将所有站点保存到文件中，供另一台计算机使用
login-login-tooltip = 连接到所选站点
login-files-at-once-usual = （通常为 { $count }）
login-verify = 验证校验和：
login-verify-tooltip = 每次与此服务器之间传输后是否验证文件校验和。仅服务器校验和：仅在服务器能计算时验证，从不回读文件（最适合慢速服务器）。
login-verify-same = 与首选项相同
login-verify-checksums = 仅服务器校验和
login-verify-off = 关闭
preferences-verify = 完成后验证文件校验和(_V)
preferences-read-back = 回读文件，最大(_R)：
preferences-read-back-note = 服务器无法计算校验和时，会回读文件进行验证：即第二次传输。
read-back-never = 从不
read-back-any-size = 任意大小

login-unfinished-none = 没有未完成的传输
login-unfinished-notice = { $count } 个传输未完成：请查看“未完成”标签页
login-unfinished-tick-tooltip = 继续或放弃此传输
login-folder-open = 展开文件夹
login-folder-close = 折叠文件夹
login-password-saved-tooltip = 密码已保存
login-rename-folder = 重命名文件夹
login-rename-folder-prompt = 文件夹 { $folder } 的新名称：
login-new-folder-prompt = 新文件夹的名称：
login-new-folder-prompt-in = { $folder } 中新文件夹的名称：
login-delete-folder = 删除文件夹
login-delete-folder-question = 删除文件夹 { $folder }？
login-delete-folder-to-top = 其中的站点和文件夹会移到列表顶部。不会删除任何站点。
login-delete-folder-to-parent = 其中的站点和文件夹会移到 { $folder }。不会删除任何站点。
login-delete-site = 删除站点
login-delete-site-question = 删除 { $name }？
login-delete-site-with-password = 该站点及其保存的密码（在密钥环中）将被移除。此操作无法撤销。
login-delete-site-detail = 该站点将被移除。此操作无法撤销。
login-exporting = 正在导出……
login-importing = 正在导入……
login-exported = 已将 { $count } 个站点（{ $passwords } 个含密码）导出到 { $file }
login-exported-unreadable = ；{ $count } 个密码无法读取
login-export-failed = 导出失败：{ $error }
login-imported = 已导入 { $count } 个站点（{ $added } 个新增，{ $replaced } 个更新，{ $passwords } 个含密码）
login-imported-unstored = ；{ $count } 个密码无法保存
login-import-failed = 导入失败：{ $error }
login-not-discarded = 未放弃：{ $reason }
login-discarded = 已放弃 { $count } 个传输：删除了 { $deleted } 个部分文件
login-password-removed = 已移除保存的密码
login-keyring-error = 密钥环：{ $error }
login-password-kept = 密码未保存（{ $error }）；它会保留在输入框中供本次登录使用
login-pair-needs-password = { $name } 需要密码：请在“直连”标签页中保存，或使用 SSH 密钥

login-waiting-key = 正在等待你核对服务器的密钥
login-waiting-certificate = 正在等待你核对服务器的证书
login-waiting-passphrase = 正在等待密钥的密码短语
login-trust = 信任并连接
login-trust-key = 未知服务器
login-trust-key-question = 信任服务器 { $host }？
login-trust-key-detail =
    这是第一次连接到此服务器，因此还无法核实它的身份。它的主机密钥指纹是：

    { $fingerprint }

    如果它与服务器的真实密钥一致，请信任它：它会被记录在 ~/.ssh/known_hosts 中。
login-trust-certificate = 不受信任的证书
login-trust-certificate-question = 信任 { $host } 的证书？
login-trust-certificate-detail =
    无法验证服务器的 TLS 证书：{ $reason }。FTP 服务器通常使用自签名证书。它的指纹是：

    { $fingerprint }

    如果它与服务器的真实证书一致，请信任它：它会为此服务器记住。

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

log-level-error = 错误
log-level-warn = 警告
log-level-info = 信息
log-level-debug = 调试
log-level-trace = 跟踪

ftp-encryption-explicit-if-available = 如可用则使用显式 FTP over TLS
ftp-encryption-explicit = 要求显式 FTP over TLS
ftp-encryption-implicit = 要求隐式 FTP over TLS
ftp-encryption-none = 仅使用普通 FTP（不安全）

job-title =
    { $action ->
        [upload] 上传 { $what } 到 { $dest }
        [download] 下载 { $what } 到 { $dest }
        [move-to-server] 将 { $what } 移动到服务器 { $dest }
        [move-to-computer] 将 { $what } 移动到本机 { $dest }
        [copy-here] 复制 { $what } 到 { $dest }
        [copy-on-server] 在服务器上复制 { $what } 到 { $dest }
        [move-here] 移动 { $what } 到 { $dest }
        [move-on-server] 在服务器上移动 { $what } 到 { $dest }
        [copy-other-server] 复制 { $what } 到另一台服务器的 { $dest }
        [move-other-server] 移动 { $what } 到另一台服务器的 { $dest }
        [delete-here] 从本机删除 { $what }
       *[delete-on-server] 从服务器删除 { $what }
    }
job-items = { $count } 项

error-cannot =
    { $action ->
        [read] 无法读取 { $path }：{ $reason }
        [open] 无法打开 { $path }：{ $reason }
        [write] 无法写入 { $path }：{ $reason }
        [create] 无法创建 { $path }：{ $reason }
        [create-folder] 无法创建文件夹 { $path }：{ $reason }
        [delete] 无法删除 { $path }：{ $reason }
        [delete-folder] 无法删除文件夹 { $path }：{ $reason }
        [set-time] 无法设置 { $path } 的时间：{ $reason }
        [finish] 无法完成 { $path }：{ $reason }
        [download] 无法下载 { $path }：{ $reason }
        [replace] 无法替换 { $path }：{ $reason }
        [access] 无法访问 { $path }：{ $reason }
       *[make-runnable] 无法使 { $path } 可执行：{ $reason }
    }
error-cannot-bare =
    { $action ->
        [read] 无法读取 { $path }
        [open] 无法打开 { $path }
        [write] 无法写入 { $path }
        [create] 无法创建 { $path }
        [create-folder] 无法创建文件夹 { $path }
        [delete] 无法删除 { $path }
        [delete-folder] 无法删除文件夹 { $path }
        [set-time] 无法设置 { $path } 的时间
        [finish] 无法完成 { $path }
        [download] 无法下载 { $path }
        [replace] 无法替换 { $path }
        [access] 无法访问 { $path }
       *[make-runnable] 无法使 { $path } 可执行
    }
error-cannot-rename = 无法将 { $from } 重命名为 { $to }：{ $reason }
error-cannot-rename-bare = 无法将 { $from } 重命名为 { $to }
error-no-such-item = 没有这个文件或文件夹
error-sftp-date = { $path } 的日期无法用 SFTP 保存
checksum-unsupported = 服务器无法计算校验和
checksum-no-answer = 服务器的回复中没有校验和
verify-mismatch = 副本的校验和与原文件不同：副本与所复制的内容不一致
verify-skipped = 已被用户跳过
item-verifying = 正在验证…
item-verified = { $size } · 已验证
item-not-verified = { $size } · 未验证
item-not-verified-large = { $size } · 未验证（大文件）
item-skip-check = 跳过检查
item-verify-now = 立即验证（回读文件）
connect-unsupported = 尚不支持 { $protocol } 连接
connect-timeout = { $host } 没有响应（超时）
connect-network = 无法连接到 { $host }：{ $reason }
connect-unknown-host-key = 服务器的主机密钥尚未记录（{ $fingerprint }）
connect-host-key-changed = 警告：{ $host } 的主机密钥已更改（known_hosts 第 { $line } 行）。可能有人正在拦截连接；拒绝连接。
connect-untrusted-certificate = 服务器的证书不受信任（{ $reason }）
connect-password-required = 请输入密码
connect-keys-rejected = 服务器不接受该 SSH 密钥；请输入密码，或检查密钥
connect-key-passphrase-required = 密钥文件 { $path } 需要其密码短语
connect-key-unreadable = 无法使用密钥文件 { $path }：{ $reason }
connect-authentication-failed = 登录失败：用户名、密钥或密码错误
connect-sftp = 服务器的文件传输（SFTP）服务失败：{ $reason }
connect-ftp = FTP：{ $reason }
connect-host-certificates = 不支持主机证书
connect-ftp-home = 无法读取主文件夹：{ $reason }
connect-ftp-no-tls = 服务器不提供 TLS 加密（{ $reply }）；请选择“{ $choice }”以在不强制加密的情况下连接
connect-tls-setup = TLS 设置失败：{ $reason }
connect-no-authorities = 此系统没有受信任的证书颁发机构
certificate-problem =
    { $reason ->
        [unknown-issuer] 它不是由本机信任的机构签发的
        [expired] 它已过期
        [not-valid-yet] 它尚未生效
        [other-name] 它是为其他服务器名称签发的
        [revoked] 它已被吊销
       *[self-signed] 它是自签名的
    }
name-empty = 请输入名称
name-reserved = “{ $name }”不能用作名称
name-forbidden-character = 名称不能包含“{ $character }”
name-null = 名称不能包含空字符
name-control-characters = 名称不能包含控制字符
name-trailing-space-or-dot = 名称不能以空格或句点结尾
name-windows-device = “{ $name }”是 Windows 为设备保留的名称
error-already-exists = { $name } 已存在
error-into-itself = { $name } 不能放入其自身
error-already-there = { $name } 已在 { $folder } 中
error-parent-not-created = 无法创建它所在的文件夹
error-folder-there = 那里已有名为 { $name } 的文件夹
error-items-inside-failed = 其中的部分项目失败
error-items-inside-not-deleted = 其中的部分项目未被删除
error-copied-but = 已复制，但{ $reason }
error-not-connected = 未连接到服务器
error-not-connected-destination = 未连接到复制目标服务器
error-cannot-copy-root = { $path } 无法复制或删除
error-nested-too-deeply = { $path } 嵌套过深（符号链接循环？）
update-error-unsigned = 此更新未使用 { -app-name } 发布密钥签名
update-error-unreadable = 无法读取更新说明：{ $reason }
update-error-unexpected-place = 更新将从意外的位置下载：{ $url }
update-error-cancelled = 下载已取消
update-error-failed = 下载失败：{ $reason }
update-error-too-large = 下载内容比发布说明的大
update-error-save = 无法保存下载内容：{ $reason }
update-error-incomplete = 下载不完整（{ $done }/{ $total } 字节）
update-error-mismatch = 下载内容与签名的发布版本不一致（SHA-256 不同）
update-error-no-key = 此版本无法检查更新
update-error-bad-key = 内置的发布密钥已损坏：{ $reason }
update-error-unreachable = 无法访问发布页面：{ $reason }
update-error-description = 无法读取发布说明：{ $reason }
update-error-version = “{ $version }”不是版本号
passphrase-problem-empty = 请输入密码短语
passphrase-problem-short = 至少使用 { $count } 个字符
passphrase-problem-mismatch = 两次输入的密码短语不一致
export-error-json = { $path } 不是有效的站点导出文件：{ $reason }
export-error-not-export = 此文件不是 { -app-name } 站点导出文件
export-error-version = 不支持的导出版本 { $version }
export-error-passphrase-required = 导入保存的密码需要密码短语
export-error-wrong-passphrase = 密码短语错误，或文件已被修改
export-error-settings = 文件中的加密设置无效：{ $reason }
export-error-random = 无法生成随机数据：{ $reason }
keyring-error-service = 密钥环错误：{ $reason }
keyring-error-not-text = 保存的密码不是有效的文本
keyring-error-unsupported = 此系统尚不支持保存密码
settings-error-invalid = { $path } 中的设置无效：{ $reason }
folder-error-into-itself = 文件夹不能放入其自身
folder-error-exists = 已有名为“{ $folder }”的文件夹

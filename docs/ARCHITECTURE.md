# ZShell 架构

个人使用的跨平台（macOS / Windows）SSH / Telnet / 串口终端。本文只记录跨模块的设计决策；模块内的细节（边界情况、库的坑）写在代码注释里。

| 文档 | 内容 |
|---|---|
| [ROADMAP.md](ROADMAP.md) | 产品规划：现状、里程碑、未实现功能的设计要点 |
| ARCHITECTURE.md | 总体结构与跨模块的设计决策 |
| [I18N.md](I18N.md) | 语言约定与国际化 |
| [DEVELOPMENT.md](DEVELOPMENT.md) | 开发环境、测试、约定、CI 与发布 |

## 技术栈

| 层 | 选型 |
|---|---|
| 应用框架 | Tauri 2 |
| 后端 | Rust + tokio |
| SSH / SFTP | `russh`（`ring` 加密后端，避免 Windows 上依赖 CMake/NASM）/ `russh-sftp` |
| 凭据 | `keyring`（macOS Keychain / Windows Credential Manager） |
| ssh_config 导入 | `ssh2-config` |
| 本地终端 | `portable-pty`（Windows 走 ConPTY） |
| 串口 | `serialport` |
| Telnet | 自己实现（协议与选项协商） |
| 前端 | Vite + React + TypeScript |
| 终端渲染 | `@xterm/xterm` + fit / webgl / unicode11 / web-links / search 插件 |

## 总体结构

```
┌──────────── 前端 (React + xterm.js) ────────────┐
│ 会话列表 │ 终端标签页 │ SFTP 面板 │ 端口转发面板 │
└───────────────┬──────────────▲─────────────────┘
          invoke (输入/命令)    │ Channel (输出字节流/事件/进度)
┌───────────────▼──────────────┴─────────────────┐
│ SessionManager: 会话任务（SSH / Telnet / 串口 / 本地 PTY）│
│ Connections: 会话 id → 已认证的 SSH 连接         │
│   一个 SSH 连接 ──┬─ shell channel              │
│                   ├─ sftp subsystem channel     │
│                   └─ direct-tcpip / forwarded   │
│ auth / known_hosts / secrets / config / settings│
└─────────────────────────────────────────────────┘
```

| 模块（`src-tauri/src/`） | 职责 |
|---|---|
| `session/` | 会话抽象：`TermIo`、`SessionManager`、`SessionSink`、输出流控 `Flow` |
| `ssh/` | 连接与 shell（`mod.rs`）、russh 回调（`handler.rs`）、主机密钥确认（`host_key.rs`）、known_hosts 的列出 / 查找 / 删除（`known_hosts.rs`）、认证（`auth.rs`）、连接注册表（`connections.rs`） |
| `sftp/` | 目录浏览与文件操作（`mod.rs`）、递归上传下载与进度（`transfer.rs`）、本地编辑远端文件（`edit.rs`）、拖出窗口（`drag/`）、文件名转码（`names.rs`） |
| `forward/` | 转发规则运行管理（`mod.rs`）、`-L` / `-R` / `-D`（`local.rs` / `remote.rs` / `dynamic.rs`）、SOCKS（`socks.rs`） |
| `pty/` | 本地终端（`mod.rs`）、默认 shell 与环境变量（`shell.rs`） |
| `telnet/` | Telnet 会话与登录提示自动填入（`mod.rs`）、协议与选项协商（`protocol.rs`） |
| `serial/` | 串口会话与设备列表（`mod.rs`） |
| `net.rs` | 出站连接（SSH 第一跳与 Telnet 共用，直连或经代理）、`Route`、TCP keepalive |
| `proxy/` | 代理：配置与连接（`mod.rs`）、SOCKS5 客户端（`socks.rs`）、HTTP CONNECT（`http.rs`）、ProxyCommand（`command.rs`） |
| `zmodem/` | rz / sz：检测与会话接管（`mod.rs`）、帧格式与 CRC（`frame.rs`）、收发字节流（`link.rs`）、接收（`receive.rs`）、发送（`send.rs`） |
| `config.rs` / `settings.rs` / `secrets.rs` | 会话、文件夹与代理的配置（`profiles.json`、`folders.json`、`proxies.json`）、应用设置 `settings.json`、钥匙串 |
| `encoding.rs` | 会话的字符编码：支持的编码、流式解码与编码 |
| `local_name.rs` | 对方（SFTP 服务器的列表、ZMODEM 发送方）给出的文件名转成本地文件名 |
| `import.rs` / `backup.rs` | ssh_config 导入；会话导出与导入 |
| `i18n.rs` / `error.rs` | 后端消息目录、结构化错误（见 [I18N.md](I18N.md)） |
| `commands.rs` | Tauri 命令入口 |
| `bindings.rs` | 生成前端的 TypeScript 类型（`src/lib/bindings.ts`） |

前端（`src/`）：

- **标签页与窗格**：状态在 `lib/tabs.ts` 的 store 里（类型在 `lib/panes.ts`，窗格树的运算在 `lib/layout.ts`）。打开、分屏、关闭、移动等每个修改都是一个 action，由纯函数 `reduceTabs` 处理；store 在修改后立即是新状态，不必等下一次渲染。`App.tsx` 订阅它并分发 action。
- **窗格的会话**：`lib/sessionRegistry.ts` 为每个窗格建一个 `PaneSession`（`lib/paneSession.ts`），负责打开会话、手动与自动重连、复制窗格时共享连接、登录后命令和输出流控。它只经一个小接口读写终端，状态、后端会话 id、转发与日志直接写进 store。`TerminalView` 只管终端本身（输入、粘贴、菜单、查找、ZMODEM）。
- **对话框**：经 `useDialog`（`lib/dialogs.ts`）登记到一个栈，用 `Modal` 渲染到 body 末尾。只有最上层的对话框响应 Esc 与点击背景，所以从设置里打开的代理对话框单独关闭。非活动标签或收起的侧栏里的对话框（如后台保存发现的编辑冲突）先隐藏，等它显示时再出现，期间也不挡快捷键。
- **快捷键**：经 `useShortcuts`（`lib/shortcuts.ts`）注册，由一个捕获阶段的监听统一分发。注册时说明何时生效：作用于标签（有对话框时不处理）、总是生效（打开设置），或只在某个对话框位于最上层时生效（设置里的搜索）。输入法组字中的按键一律不处理。
- **其他**：`components/` 下每个面板一个组件；`lib/api.ts` 封装全部后端命令，与后端交换的类型都来自生成的 `lib/bindings.ts`。纯逻辑模块有 vitest 单元测试（与模块放在一起的 `*.test.ts`）。

## 设计决策

### 会话与数据流

- **前后端类型**：与前端交换的 Rust 类型都派生 ts-rs 的 `TS`，`bindings.rs` 的测试从一组根类型出发收集全部依赖，写成一个 `src/lib/bindings.ts`（提交到仓库）：`cargo test` 在它过时时失败，CI 因此能发现只改了一边的类型；`UPDATE_BINDINGS=1 cargo test bindings` 重新生成。前端沿用的名字与 Rust 不同时用 `#[ts(rename)]`；省略时不写入的 `Option` 字段标 `#[ts(optional)]`，在 TypeScript 里是 `x?: T`。`Error` 的序列化是手写的，TS 类型也手写（`CommandError`），其 `code` 是从英语语言包 `errors.*` 生成的联合类型 `ErrorCode`，前端拿错误码比较时由 `tsc` 检查。选 ts-rs 而非 specta：specta 2.0 仍是候选版，`skip_serializing_if` 需要分输入、输出两套类型，u64 / usize 字段要逐个标注。命令的调用函数仍在 `api.ts` 里手写。
- **打开会话**：只有一个命令 `session_open`，参数是带标签的 `SessionSpec`：`profile`（保存的会话，按协议选择后端；`carry` 见端口转发）、`quick`（快速连接）、`shared`（复制标签，见 SSH）、`local`（本地终端）。每种先解析成日志命名、编码和后端（`Backend`），日志槽位与会话任务只在一处建立。
- **会话是一个后台任务**：`SessionManager` 为每个会话创建 `TermIo`（输出 Channel、事件 Channel、输入队列），后端任务（SSH shell、Telnet、串口或本地 PTY）持有它运行。新的会话后端只需实现同样的任务形态。
- **会话的关闭**：`SessionManager::remove` 是唯一的入口（`session_close`、页面重载、窗口销毁），先释放会话持有的资源，再中止任务。任务之外的资源（SSH 连接的登记、等待用户回答的 ZMODEM 传输、读线程可能在等的流控）在会话的 `Lifetime` 上登记释放动作；`remove`、会话自己结束（`TermIo::finish`，在报告结束之前，标签可能立即重连）、任务被丢弃三者谁先发生谁释放，只释放一次；会话关闭之后才登记的（任务恰好在关闭时完成认证）立即释放。所以释放不依赖调用顺序，也不依赖中止的任务何时被丢弃。任务内部的资源（PTY、串口线、shell channel）由各自的 Drop 收尾，阻塞线程靠 channel 关闭退出（串口），等待 shell 退出用条件变量而不是轮询。
- **会话属于 webview**：会话记下打开它的 webview 的 label。webview 开始加载页面（重载、内容进程崩溃后重新加载，开发时的热重载）或窗口销毁时，关闭该 webview 的全部会话：新页面没有原来的标签，这些会话没有人显示也没有人关闭，PTY 的读线程会卡在流控上，SSH 的输出会堆在 IPC 队列里。远端会话（SSH、Telnet、串口）结束时都用 `TermIo::finish` 报告，终端里的提示与 `Closed` 事件一致。
- **终端输出不走 event**：经 Tauri 2 `Channel` 以 `InvokeResponseBody::Raw` 推送，前端直接 `term.write(Uint8Array)`，避免 JSON 序列化开销。会话事件走同一个 channel：每条消息第一个字节是类型（0 输出、1 JSON 编码的事件）。两个 channel 之间没有顺序保证，合在一起后"连接已关闭"之类的提示总在最后一段输出之后。输入走 `invoke`，尺寸变化发 `window-change`。
- **输出流控**：前端在 `term.write` 回调里累计已处理字节，每 64 KiB 调一次 `session_ack`；后端未确认超过 2 MiB 时暂停读取，降到 512 KiB 以下再继续（`Flow`：读线程用条件变量等，读任务用 `Notify` 等）。所有后端都据此暂停，否则快速的远端程序（`cat` 一个大文件）的输出会堆在 Tauri 没有上限的 IPC 队列里，xterm.js 积压过多时还会丢弃写入。暂停的代价：Telnet 与串口由 TCP、驱动和线路流控挡住对方；SSH 暂停读 shell channel 会让 russh 的整条连接停下（同一连接上的 SFTP、转发、复制出的标签一起等），按最慢的终端暂停，但有上限，比内存无限增长好。
- **SSH 读写并行**：写入等待服务器窗口时仍持续读取输出。russh 在连接任务里把输出送入有界队列，队列满会卡住连接任务，窗口调整也就收不到，形成死锁（大段粘贴、ZMODEM 上传时远端还在输出）。写入未完成时不取新的输入，以此形成背压。本地终端的输入队列同样有界（写线程按 shell 的读取速度消费）。
- **字符编码**：会话配置的 `encoding`（默认 UTF-8，另有 GB18030、GBK、Big5、Shift_JIS、EUC-JP、EUC-KR、Windows-1252）用于保存的 SSH、Telnet、串口会话（快速连接与本地终端总是 UTF-8）。应用内部一律是 UTF-8，在与远端交界的地方转换：
  - 输出：`SessionSink::output` 先在原始字节上检测 ZMODEM，要显示的部分再经 `SessionSink::remote` 流式解码（跨两次读取的多字节字符能拼完整），然后写入终端和日志，所以日志记的是转码后的文本。ZMODEM 结束后交还的剩余输出同样走 `remote`。会话结束时和 ZMODEM 传输开始前结束解码流（`Codec::flush`），留下的不完整字符显示为替换字符，不会丢掉，也不会和传输之后的输出拼在一起；我们自己的提示文字本来就是 UTF-8，直接 `write`。
  - 输入：`TermIo::recv` 把键盘、粘贴、撰写栏、快速命令、登录后命令编码后交给后端。ZMODEM 数据不转；终端内的认证提示（`read_line`）读 UTF-8，这是 SSH 协议的规定。目标编码表示不了的字符发 `?`。
  - SFTP：russh-sftp 把文件名一律按 UTF-8 读（有损），所以非 UTF-8 会话在 SFTP 通道与 russh-sftp 之间加一层（`sftp/names.rs`），按 SFTP v3 的包格式只改写路径字段：请求里的路径转成会话编码，`SSH_FXP_NAME` 回复里的文件名转成 UTF-8，其余原样转发。
    - 限制：文件名在应用里是 UTF-8 字符串，转换不可逆的名字无法再定位到原文件。非 UTF-8 会话里，不符合会话编码的字节（如 GBK 会话里混入的 UTF-8 或乱码名字）解码成替换字符，回写时变成 `?`；UTF-8 会话里，不是合法 UTF-8 的名字（russh-sftp 有损读取）同样如此。这类文件在列表中能看到，但打开、下载、重命名、删除都会因找不到文件而失败；递归删除或下载遇到它们时可能报错中止。要处理需改成全程按原始字节传递路径（russh-sftp 不支持），暂不做。
  - ZMODEM：`ZFILE` 里的文件名按会话编码收发。
  - 选 UTF-8 时以上全部跳过。修改编码从下一次连接起生效；复制标签沿用源连接当时的配置。
- **会话事件**：`Connected`、`Forward`（转发规则状态）、`Zmodem`（见下文）、`Closed { reason, status, error }`。`reason` 为 `exited`（shell 退出，或 Telnet 服务器关闭连接）、`lost`（已建立的连接断开、串口设备被拔出）、`failed`（连接、认证、打开设备或启动阶段失败），前端据此决定重连、自动关闭标签或提示。
- **Break**：输入队列里的 `SessionInput::Break`（`session_break`），串口发送约 250 ms 的 break 信号，Telnet 发 `IAC BRK`，SSH 与本地终端忽略。
- **命令在哪个线程上运行**：Tauri 的同步命令跑在主线程上，等待时整个窗口卡住。碰文件系统、钥匙串或系统设备列表的命令是 async 的，阻塞部分放进 `spawn_blocking`（`commands.rs` 的 `blocking`）；会话任务里读钥匙串用 `secrets::password` 等异步接口（Telnet 的自动登录在任务内用 `block_in_place`），不占住 tokio 的工作线程。升级后签名变了，macOS 会对每个钥匙串条目重新询问，这时其他标签的输出、转发和 SFTP 照常运行。例外：前端连续发出、不等结果的整体替换（设置、快速命令、转发规则列表）仍是同步命令，因为 async 命令之间没有顺序保证，最后发出的未必最后写入；它们只写一个小文件，设置里耗时的日志清理放到后台。

### SSH

- **一个连接多路复用**：认证完成后连接登记到 `Connections`；SFTP 首次使用时在同一连接上开 subsystem channel 并缓存，端口转发也开在同一连接上。上传、下载与拖出另开一个 SFTP channel（同一 channel 上的请求按顺序应答，大文件传输时列目录要排在传输的读请求后面），浏览与编辑用原来的；服务器的 `MaxSessions` 不允许再开时，传输共用浏览的那个。
- **复制标签**：已连接的 SSH 标签复制或分屏时，新标签（窗格）用 `session_open` 的 `shared` 在源标签的连接上开一个新的 shell channel，不重新连接和认证（类似 OpenSSH 的 ControlMaster），也不重复自动启动转发。`Connections` 按会话 id 登记，多个会话可以指向同一连接，最后一个使用它的会话结束时才断开。登记的同时在会话的 `Lifetime` 上登记注销（复制标签在任务开始之前就登记），关闭标签时同步注销，手动重连时旧连接上的规则先停下、新连接才启动它们；连接关闭后不再启动转发规则。转发状态推送给连接上所有会话的标签，后加入的标签先收到各规则的当前状态。源连接已断开时按正常流程新建连接，包括已断开但会话还没发现的情况（笔记本睡眠唤醒后，keepalive 尚未判定断线时开 channel 失败）；复制出的标签断线重连也总是新建连接。关闭标签时显式关闭它的 shell channel（russh 拆开读写的 channel 丢弃时不会关闭），否则共享连接上服务器那一侧的 shell 一直留着，占用 sshd 的 `MaxSessions`。
- **提示都在终端里**：主机指纹确认、密码、私钥口令、keyboard-interactive 问题都通过 `TermIo::read_line` 在终端里询问，与 OpenSSH 一致，不弹窗。russh 在自己的任务里回调 `check_server_key`，handler 通过 mpsc + oneshot 把问题转交给会话任务。
- **认证**："自动"（默认）按 OpenSSH 的顺序尝试 agent 中的密钥 → 会话指定的私钥与 `~/.ssh/id_ed25519`、`id_ecdsa`、`id_rsa` → keyboard-interactive → 密码。加密的私钥只在服务器接受其公钥后才询问口令（细节见 `ssh/auth.rs`）。
- **多因素认证**：服务器要求多种方法时（`AuthenticationMethods publickey,keyboard-interactive` 等），通过的一步得到 partial success 与仍需的方法，认证按这些方法继续：第一步用会话设置的方法，之后按"自动"的顺序；同一连接上提交过的密钥不再提交（`publickey,publickey` 要两把不同的密钥）。仍需的方法里没有能用的，报 `auth.moreRequired`，列出服务器要求的方法；会话设置的方法服务器根本不接受时报 `auth.notOffered`。保存的密码只回答一次：密码方法，或 keyboard-interactive 中唯一一个提示文字含 "password" 的不回显提示；验证码之类的提示总是在终端里询问，以免把密码当验证码提交、消耗服务器的尝试次数。验证码不能无人值守地输入，自动重连时同样在终端里询问。
- **shell 通道的选项**：先请求 agent 转发（`auth-agent-req@openssh.com`），再按会话的 `termType`（默认 `xterm-256color`）开 pty，逐个发 `env` 请求，最后启动 shell。`env` 请求用 `want_reply=false`，与 OpenSSH 一样不报告被服务器 `AcceptEnv` 拒绝的变量。复制标签的新 shell 用连接登记时保存的会话配置。
- **agent 转发**：只转发给目标主机，不转发给跳板机。`ClientHandler` 只在本连接请求过转发时接受服务器开的 auth-agent 通道（russh 默认会接受）。每个通道各连一次本地 agent，与认证共用 `connect_agent`（macOS 用 `SSH_AUTH_SOCK`，Windows 先试 OpenSSH 命名管道再试 Pageant），在单独的任务里双向转发，不阻塞 russh 的连接任务；连不上本地 agent 时拒绝该通道。
- **主机校验**：读写 `~/.ssh/known_hosts`，并与 OpenSSH 的默认值一样也读 `~/.ssh/known_hosts2` 和系统的 `ssh_known_hosts`、`ssh_known_hosts2`（`/etc/ssh`，Windows 的 OpenSSH 为 `%ProgramData%\ssh`），所有文件合成一个列表判断；这些额外的文件只读不写，读不出时跳过。首次连接确认指纹，指纹变化时显式告警并拒绝连接（同 OpenSSH），告警里写出冲突的文件、行号和删除旧记录的方法（在 `~/.ssh/known_hosts` 里时提到设置里的 Known Hosts，并给出完整的 `ssh-keygen -f "<路径>" -R "<主机>"`，双引号在 cmd、PowerShell、POSIX shell 里都能用）。校验由我们自己的解析（`ssh/known_hosts.rs`）按 OpenSSH 的规则做：任一行有服务器的密钥即通过（即使别的行有同类型的另一把密钥），`@revoked` 的密钥一律拒绝并显示吊销告警，`@cert-authority` 行只用于证书，主机名忽略大小写并支持 `*` / `?` 通配与 `!` 否定。russh 自带的校验遇到第一行同类型的不同密钥就判为变化、忽略这些标记和通配，且行号不计注释行，所以不用。添加新密钥仍用 russh 的 `learn_known_hosts`。
- **known_hosts 管理**：设置里的 Known Hosts 打开单独的对话框，列出每行的主机、算法、SHA256 指纹与 `@cert-authority` / `@revoked` 标记；读不出密钥的行也列出（便于删掉坏行）。哈希过的主机名只显示为哈希，搜索框里输入 `host`、`host:port` 或 `[host]:port` 时由后端按 HMAC-SHA1 找出对应的哈希条目（同 `ssh-keygen -F`）。删除按"行号 + 该行原文"进行，文件在列出之后被改过（如 ssh 又加了一行）则报 `knownHosts.changed` 并重新列出；删除前把原文件存为 `known_hosts.old`（同 `ssh-keygen -R`），新内容先写临时文件再改名替换，保留原文件的权限和其余各行的换行符。一行里有多个主机名时删除整行。
- **ProxyJump**：会话的 `jumpHosts` 按顺序引用其他会话，每一跳用被引用会话的地址和认证，但不展开它自己的跳板机。在上一跳连接上开 `direct-tcpip` channel，以其 `ChannelStream` 作为下一跳的传输层；主机密钥按每一跳自己的 host:port 校验，各跳的提示都在同一个终端里。被引用的会话不能删除，也必须保持是 SSH 会话。建跳板的部分（`ssh::tunnel`）SSH 与 Telnet 共用，返回到目标的字节流和各跳连接（`JumpChain`，丢弃时由后往前断开）。第一跳经代理连接时用的是第一台跳板机自己的代理（见下文"代理"）。
- **keepalive 与重连**：会话配置 `keepaliveInterval`（默认 30 秒，连续 3 次无响应判定断线）与 `autoReconnect`（默认开）。`lost` 时前端按 2、4、8、16、30 秒退避重连，一直重试到标签页关闭；认证错误、主机密钥被拒绝，以及代理要求或拒绝凭据时停止。Enter 立即重连（只认单独按下的 Enter；断开时粘贴的内容直接丢弃，多行粘贴不会触发重连），Ctrl+C 取消，`online` 事件立即重试。重连是新会话：SFTP 面板回到原目录，自动启动的转发规则重新启动（手动重连时之前开着的规则也重新启动，见「端口转发」）。会话结束时后端先重置终端模式再打印结束提示（`SessionSink::reset_modes`：离开备用屏幕、关闭鼠标报告，再软重置 bracketed paste、应用光标键等），因为断线时程序来不及关掉它们，下一个 shell 会把鼠标报告当成输入，提示也会落在备用屏幕里看不到；在连接中手动重连时由前端重置。
- **连接阶段的超时**：TCP 连接 15 秒；SOCKS / HTTP 代理的握手（含代理连接目标）30 秒；SSH 的版本与密钥交换 30 秒，不计等待用户回答主机密钥的时间。认证阶段不设超时（有密码等提示，服务器有自己的 `LoginGraceTime`）。超时不算认证错误，自动重连照常重试。

### Telnet 与串口

- **会话配置**（2.0 起）：会话是共同字段（名称、文件夹、编码、自动重连、登录后命令、自动记录日志、快速命令分组、外观）加 `connection`，后者按 `protocol` 区分：`ssh` 为 `Remote`（主机、端口、用户名、跳板机、代理、keepalive、终端类型）加认证、agent 转发、环境变量与转发规则，`telnet` 为 `Remote`，`serial` 为设备与线路参数。改协议后保存只留新协议的字段：转发规则随之删除，正在运行的规则停止。保存时按协议校验必填项：SSH 要主机和用户名，Telnet 要主机，串口要设备。SSH 后端（含跳板机）只接收 `SshProfile`（会话的 id、名称、编码与 SSH 选项），Telnet 后端只接收 `Remote`，拿到的一定是对应协议的配置。2.0 之前的扁平格式不读取：旧的 `profiles.json` 按"读不出的文件"处理。
- **打开会话的协议**：快速连接支持 SSH 与 Telnet（搜索框里的 `telnet host[:port]`、`telnet://…`）。前端的标签目标为 `profile`（保存的会话）、`quick`、`local`，标签上记录当前会话的协议（每次连接时按会话配置更新），文件与转发面板、复制标签的共享连接、Break 菜单项都按它判断：文件与转发只用于 SSH；串口设备只能打开一次，不能复制标签。
- **Telnet**：经 `net::connect`（或 `ssh::tunnel` 的跳板机通道）得到字节流，协议层是纯状态机（`telnet/protocol.rs`，RFC 1143 的 Q 方法）：开场请求 BINARY（双向）、ECHO、SGA，提供 TTYPE 与 NAWS，其余一律拒绝；终端类型取会话的 `termType`，窗口大小在每次 resize 时上报。非二进制模式下按 NVT 发送回车（CR NUL）并去掉服务器 CR 之后的 NUL。服务器不回显时在本地回显。写入在单独的任务里进行，等待写入时仍持续读取（与 SSH 同理）。服务器关闭连接算 `exited`（与 telnet 命令一样无法区分退出与掉线），读写出错或跳板机连接断开算 `lost`；直连时用会话的保活间隔开 TCP keepalive。
- **Telnet 登录**：保存的用户名与密码（钥匙串，与 SSH 共用同一条目）由后端在输出以 `login:` / `username:` / `password:` 结尾并停顿 300 ms 后各填一次；用户按过键、超过 30 秒或没有保存时交给用户，密码不经过前端。登录后命令照常由前端在出现提示符后发送。
- **串口**：`serialport` 的句柄是阻塞的，与本地终端一样各用一个线程读、写；读取每隔一段时间醒来，检查线路是否已被丢弃（报告故障的 channel 已关闭；Windows 上同一句柄的读写互相等待，间隔取 10 ms），写线程在输入 channel 关闭时退出。设备被拔出时读写出错，算 `lost`，前端按断线重连的节奏重试，插回后自动接上。端口只能被一个句柄打开（macOS 上 serialport 独占打开，Windows 上本来如此），而关闭的会话的读线程最多要等一个读取间隔才放手，所以打开设备前先等本应用里仍持有该设备的线程退出（最多 1 秒），手动重连才不会报设备忙。macOS 只列出 `/dev/cu.*`（`tty.*` 打开时等待载波）；伪终端（`/dev/ttysNNN`，如 QEMU 的 `-serial pty`）没有线路速率，不设波特率。

### 代理

- **配置**：代理是独立保存的配置（`proxies.json`，有名称），类型为 SOCKS5、HTTP（CONNECT）或命令（ProxyCommand）；会话的 `proxy` 字段引用其 id，SSH 与 Telnet 可用。与会话一样，切换类型时其他类型的字段保留不动。被会话引用的代理不能删除（`proxy.inUse`），而不是悄悄改成直连。密码存钥匙串，条目名为 `proxy:<id>`，与会话密码（条目名为会话 id）分开；只有设了用户名的 SOCKS5 / HTTP 代理才读写钥匙串。
- **只用于第一个连接**：`ProfileStore::route` 得出 `net::Route`（跳板机列表 + 第一个连接的代理）。代理取第一台跳板机会话自己的设置，没有跳板机时取会话的设置，与 OpenSSH 一致（ProxyJump 连跳板机时用该主机 Host 块里的 ProxyCommand）。所以会话有跳板机时保存会清掉它自己的 `proxy`，编辑界面里代理下拉框禁用并显示第一台跳板机的代理；串口会话保存时同样清掉。快速连接不经代理。
- **接入点**：`net::connect` 是 SSH 第一跳与 Telnet 直连的唯一入口，有代理时交给 `proxy::connect`，返回 `Box<dyn Stream>`（russh 的 `connect_stream` 接受任意字节流）。TCP keepalive 设在直连或到代理服务器的 TCP 连接上（Telnet 用；SSH 用自己的 keepalive）。
- **SOCKS5 / HTTP**：主机名交给代理解析（socks5h），代理那一侧的内网域名也能连；IP 地址按地址类型发送。HTTP 的凭据随 CONNECT 请求一起发送（Basic），响应头逐字节读到空行为止，之后的字节属于隧道（服务器可能先说话，如 SSH 的版本串）。需要密码而钥匙串里没有时在终端里内联询问；密码被拒绝时重新建立到代理的连接再问，最多 3 次（与 SSH 密码相同）。错误码区分连不上代理（`proxy.unreachable`）、要求认证、认证失败、代理拒绝转发（`proxy.refused`，附 SOCKS 回复码或 HTTP 状态行）与协议错误，和目标主机的错误分开。
- **ProxyCommand**：替换 `%h`、`%p`、`%r`、`%%` 后启动本地进程，以其 stdin / stdout 作传输层，stderr 以暗色写进终端（`ssh` 也让它直接输出），stream 丢弃时结束进程，进程退出即连接结束。macOS 用 `$SHELL -c "exec …"` 运行（`exec` 让进程替换 shell，结束的是命令本身），PATH 取登录 shell 的（只在第一次用时启动一次 `$SHELL -l` 读取，Finder 启动的应用没有 `/opt/homebrew/bin`）；Windows 不经 `cmd`，直接启动程序（否则结束的只是 `cmd`，程序留在后台），`CREATE_NO_WINDOW` 不弹控制台窗口。命令的 stdin 被占用，不能交互式询问密码。代入 `%h`、`%r` 的主机名与用户名不能含 shell 元字符、空白、控制字符或以 `-` 开头（同 OpenSSH 对 CVE-2023-51385 的处理），否则拒绝连接（`proxy.unsafeName`）：导入别人的会话文件不应能借此执行命令。SOCKS / HTTP 代理同样拒绝含空白或控制字符的主机名（换行会加进 HTTP 请求），保存会话时也做这项校验。

### 文件传输与端口转发

- **SFTP 传输**：先扫描生成计划（目录、文件、总字节数），再逐个文件复制；进度经 Channel 每 100 ms 推送一次；取消通过共享的 `AtomicBool`，未完成的新文件会被删除；关闭窗格时取消其文件面板里的传输（分屏、复制的标签共用连接，否则传输会在看不到的地方继续）。上传在关闭远端文件时检查服务器的回复，有的服务器（NFS、配额）到这时才报告写入失败。覆盖已有文件时（上传、编辑上传、"下载到…"选了已有文件）先写同目录下的临时文件（`.<文件名>.zshell-<随机>`），写完再替换原文件，取消、出错或断线时原文件保持不变（`sftp/replace.rs`）。替换会改变内容以外的东西时仍原地写入：符号链接（要写到它指向的文件）、属主或属组不是自己的文件（替换后会变成自己的）、目录不能新建文件。临时文件带上原文件的权限。SFTP v3 的 rename 不能覆盖已有文件，russh-sftp 又发不了 `posix-rename@openssh.com`，所以改名前先删除原文件，只有这两步之间失败时新内容会留在临时文件名下（错误信息给出它的路径）。上传扫描本地目录时只跟随指向文件的符号链接（与下载一致），避免链接环。
- **SFTP 会话**：每条连接一个 SFTP 会话，第一次用到时打开。russh-sftp 不报告会话是否已结束，所以交给它的流外面包一层，读到 EOF 或出错时记下；之后再取会话时重新打开一个。服务器上的 `sftp-server` 退出而 SSH 还连着时，文件面板不用重连标签就能继续用。
- **SFTP 请求超时**：每个请求等待回复 120 秒（russh-sftp 默认 10 秒），每个文件保持 8 个读请求在途。请求要排在已发出的读请求之后，慢速链路上 10 秒不够（低于约 400 KB/s 时下载与下载中的列目录都会超时）。断线不靠这个超时发现：keepalive 或 TCP 断开连接后通道关闭，等待中的请求立即失败。
- **本地文件名**：下载、拖出、本地编辑与 ZMODEM 接收都把对方给出的名字交给 `local_name::local_file_name`，保证结果是目标文件夹里的一个文件名：`..`、含 `/` 的名字被拒绝（递归下载时跳过该项）；Windows 上不允许的字符（含 `\`、`:`）和结尾的点、空格换成 `_`，`CON`、`COM1` 等设备名前加 `_`，因为 `Path::join` 遇到绝对路径或 `C:x` 这样的盘符前缀会丢掉目标文件夹。
- **SFTP 面板**：选择、排序、过滤、隐藏文件都在前端完成，排序与隐藏文件开关存在 localStorage（按机器记住，与侧栏宽度一样）。不选位置的下载（双击、"下载"、ZMODEM 的 sz）进设置中的下载文件夹，默认是系统的"下载"；"下载到…"单个文件用保存对话框（同名由系统询问覆盖），多项或文件夹选目录（同名自动加序号）。自动加序号的名字在选定时就创建出来占住（`local_name::create_unique`，`create_new` / `create_dir`），同时开始的两次下载、大小写不敏感的文件系统上的 `README` 与 `readme` 各得一个名字；下载失败时交还没下到的项占住的名字。ZMODEM 接收和新日志文件同样命名。
- **用本地编辑器编辑**：后端把文件下载到临时目录（`ZShell-edit/<编辑 id>/<文件名>`，保留原文件名），用设置中的编辑器或系统默认程序打开，每 500 ms 检查一次本地文件的大小和修改时间，稳定一个周期后才报告"已保存"，避免上传写了一半的文件。上传由前端发起，走标签页当前的连接，所以重连后继续有效，断线期间的保存在重连后补传。上传前比较远端文件的大小和修改时间与上次下载 / 上传时是否一致，不一致则询问是否覆盖。同一文件的上传不会并行：上传中再保存只做标记，结束后再传一次，后端也按编辑任务加锁，否则后一次会把前一次写了一半的文件误认为别人的修改。上传按上面的替换方式写入，保留远端文件的属主和权限。关闭标签页或点"停止"后不再监视，临时文件留到之后启动时清理一天未动的。
- **拖出窗口**：行上按下并移动超过 5 像素后，前端调用后端开始原生拖动，拖动结束后返回结果（取消、放在本窗口、放在其他应用）和松开位置。只有放到其他应用上才下载，多项合并为一条传输。macOS 用文件承诺（`NSFilePromiseProvider`）：Finder 给出目标路径后在我们自己的操作队列上下载，Finder 显示占位文件直到完成。Windows 用 OLE 拖放（CF_HDROP）提供临时目录（`ZShell-drag`）中的路径：松开鼠标前对方取数据只得到路径；松开在其他窗口后，对方再取数据时才下载，期间处理消息以保持界面响应；支持 `IDataObjectAsyncCapability`，资源管理器会在自己的线程里取文件。放到其他应用后，传输被取消、或对方 5 分钟没有再要文件时结束这次拖放（对方可能只要其中一部分，一个都没要则算取消），释放它持有的连接；Windows 上被拒绝的拖放（效果为无、也没取过数据）算取消。放在本窗口时由前端按松开位置决定：落在文件夹行或"上级"按钮上就移动（rename）。从 Finder / 资源管理器拖入时同样可以放到某个文件夹上传。注意 wry 在 macOS 上给出的拖放坐标是点而不是物理像素。
- **端口转发**：规则保存在会话配置的 `forwards` 中，只能通过 `profile_set_forwards` 修改（`profile_save` 保留原有规则）。规则的每次运行是一个任务，持有它的全部资源：监听器、承载的连接，`-R` 还有路由表条目和服务器端的监听。停止规则是给任务发信号（丢弃信号也算），任务释放全部资源后才结束，`-R` 要等服务器确认取消监听；所以重启时只需等上一次的任务结束，同一端口立即可用，不再另外记录进行中的取消请求。规则属于会话（profile），而同一会话分别打开的标签各有自己的连接，所以一条规则只在该会话的一条连接上运行（由 `Connections` 负责）：从任一标签启动、停止、编辑都落到实际运行它的连接上；新连接自动启动时跳过同会话其他连接已在运行的规则；保存规则列表时停掉已删除却仍在运行的规则；状态经会话的 `Hub` 推送给该会话的所有标签。关闭某连接的最后一个标签时，若它上面有运行中的规则、且同会话还有别的标签连着，前端询问是否移过去继续运行（`forward_keep`），选择保留则先等旧任务结束、断开旧连接，再在另一条连接上启动；同会话没有别的标签时直接停止，不询问。在同一标签里手动重连时，旧连接上运行的规则（含手动开启的）经 `session_open` 的 `carry` 在新连接上重新启动；断线后的自动重连只启动自动启动的规则。状态（Starting / Active / Failed / Stopped）经会话事件推送；每次启动分配一个代号（generation），被替换的旧任务不能覆盖新状态。`-R` 的 `forwarded-tcpip` channel 由 `ClientHandler` 按路由表转交给规则任务，不阻塞 russh 的连接任务。`-L` / `-D` 监听绑定地址解析出的全部地址（同 OpenSSH），`localhost` 同时监听 127.0.0.1 与 ::1。`-R` 规则在服务器回复 `tcpip-forward` 之前被停止时，等回复到达后再取消监听（russh 不会在等待方被丢弃时撤回请求），否则服务器上留下没有路由的监听。

### ZMODEM

- **位置**：在会话层而不是某个后端：SSH、Telnet、串口与本地终端（包括在本地终端里 ssh 登录后）都适用。协议自己实现（帧、CRC、转义、收发状态机），以 lrzsz 为对照测试。
- **检测与接管**：后端把远端输出交给 `SessionSink::output`，在其中查找 `sz` 的 ZRQINIT / `rz` 的 ZRINIT 十六进制头（`**\x18B00` / `**\x18B01`，可跨两次输出：输出末尾从 ZDLE 起可能是头部开头的几个字节先不显示，等下一次输出再定）。识别后到传输结束：输出交给传输任务而不再写入终端；用户按键被丢弃，Ctrl+C 取消；传输任务要发送的数据经 `TermIo::recv` 注入，与键盘输入走同一条路到后端，并有背压。开头的头部在 2 秒内（从识别时算起，不是从最后一次输出算起）没有完整到达（例如 `cat` 了一个二进制文件），即视为误判，期间读到的输出原样交还终端；这期间按 Ctrl+C 会把 `^C` 发给远端程序，而不是取消序列。结束后剩余的输出（shell 提示符）交还终端，并照常检测，紧接着的下一个传输（`sz a; sz b`）也能识别；误判时交还的输出不再检测，否则会再次误判。会话结束（关闭标签、重连）时进行中的传输随之取消，包括正在等用户选择文件的。Telnet 的本地回显只回显键盘输入，不回显传输数据。
- **背压**：传输期间远端的数据进入传输任务的队列时计入会话的流控，传输任务取走时确认；写盘比链路慢时读取因此暂停，队列不会无限增长，与终端输出用同一个上限。
- **与前端交互**：经 `Zmodem { phase }` 会话事件。`sz` 时先问保存位置（`zmodem_save_to`）：默认在终端上方显示提示条，可选存进下载文件夹、选择文件夹或取消，因为 `cat` 一个恰好像 `sz` 输出的文件也会触发接收，不应在用户不知情时存下文件；设置里可改为直接存进下载文件夹，或每次立即弹出文件夹选择框（1.6.12 之前的 `askDownloadLocation` 为真时读作后者，为假时读作默认的询问）；`rz` 时问要发送的文件（`zmodem_send_files`）：终端上方显示提示条并立即弹出文件选择框，选择框取消后提示条仍在，可以拖入文件、重新选择或取消；`zmodem_cancel` 随时取消。进度（文件名、已传 / 总量、速度）和每个文件的结果由后端写在终端里。
- **兼容性**：发送时转义全部控制字符（相当于 `rz -e`），接收时也在 ZRINIT 里要求对方这样做，以通过会过滤控制字符的堡垒机；既然要求了，接收时头部和数据里未转义的控制字符就是线路噪声，与 lrzsz 一样丢弃，否则会注入字节的链路上每个包的 CRC 都会失败；使用对方支持的 32 位 CRC；对方不支持全双工或缓冲区有限时逐包确认（每包以 ZCRCW 结束本帧，收到 ZACK 后再以新的 ZDATA 头开始下一帧；无确认时重发）。下载的同名文件按 SFTP 的规则改名，保留修改时间。
- **取消与出错**：向对方发送取消序列（连续 CAN），删除未完成的本地文件，在终端里提示。下载被取消时，对方已发出的数据还在路上，先丢弃一段时间，只保留最后一个 CAN 之后的输出（对方退出后的 shell 提示符）。对方超时无响应时按 lrzsz 的习惯重发（每次 10 秒，最多 5 次）后放弃；接收数据时超时或数据损坏，从已收到的位置用 ZRPOS 请对方重发，同一位置附近连续 20 次损坏（数据没有前进）则放弃，避免链路总在同一处出错时无限重试。

### 本地终端

- 作为 `SessionManager` 的另一种后端（`session_open` 的 `local`），输入、resize、关闭沿用 SSH 会话的命令。
- **前台进程**：`session_foreground` 报告 shell 之外正在前台运行的程序名，用于关闭标签前的确认。unix 比较终端的前台进程组（`tcgetpgrp`）与 shell 的 pid，后台作业不算；Windows 没有前台进程组，以 shell 是否有子进程判断。
- **默认 shell**：macOS 以登录 shell 方式启动用户的 shell，让 `/etc/zprofile`、`~/.zprofile` 补全从 Finder 启动时缺失的 PATH；Windows 优先 PowerShell 7，否则用系统自带的 Windows PowerShell。
- **环境变量**：`TERM=xterm-256color`（仅 unix）、`COLORTERM=truecolor`、`TERM_PROGRAM=ZShell`；macOS 上没有区域设置时按系统语言生成 UTF-8 的 `LANG`。
- **结束**：以子进程退出为准，而非输出 EOF（后台作业可能一直占着终端）。shell 以 0 退出时自动关闭所在窗格（只有一个窗格时即关闭标签页，同 Terminal.app），否则保留，按 Enter 重启。关闭窗格时 unix 发 SIGHUP、Windows 关闭 pseudoconsole，2 秒后仍未退出则强制结束。

### 配置与设置

- **存储**：配置目录（macOS `~/Library/Application Support/org.boyin.zshell/`，Windows `%APPDATA%\org.boyin.zshell\`）下的 `profiles.json`、`folders.json`、`proxies.json`、`commands.json`（快速命令）、`logs.json`（会话日志索引）与 `settings.json`；密码与口令只存系统钥匙串，服务名为 bundle identifier `org.boyin.zshell`。保存会话或代理时先写配置再写钥匙串，钥匙串失败不算保存失败：命令返回已保存的项和密码错误（`Saved`），对话框改为编辑这一项并提示，再次保存是更新而不是新增。
- **单实例**：Windows 上再次打开应用会启动新进程，两个进程各自写回整份配置会互相覆盖，所以用 `tauri-plugin-single-instance` 把已有窗口提到前面后退出（插件第一个注册）。macOS 上系统本来就会激活已运行的应用，不加插件（插件在 macOS 上的锁是 `/tmp` 下与 HOME 无关的 socket，会妨碍用隔离 HOME 做端到端测试）。
- **读不出的文件**：启动时任何一个配置文件存在但读不出（崩溃截断、手工改错、更新版本写入的新枚举值）时，改名为 `<文件名>.bad-<时间>` 保留，该部分从空开始，前端启动后列出这些文件（`config_set_aside`，只返回一次）并可在访达 / 资源管理器中显示。不让应用因此无法启动，也不让下一次保存覆盖原文件。写入时先写临时文件并 `fsync`，再改名替换。一次修改要写几个文件时（删除文件夹会改 `folders.json` 和 `profiles.json`），先把所有临时文件都写好，再依次改名，写入失败（磁盘满）时所有文件都保持原样，与内存中的状态一致。导入在同一把锁下读取现有会话并写入，两次导入同时进行也不会重复添加。
- **设置**：分为 `appearance`、`terminal`、`tabs` 三组；后端校验并夹取数值。前端 `SettingsProvider` 启动时读取，修改即时生效并保存，较旧的保存结果不会覆盖较新的修改。界面文字大小（`textSize`）通过 CSS 变量 `--text-scale` 缩放界面的字号、文字行的高度和对话框等的宽度，标题栏高度、图标和终端（有自己的字号）不变，以免红绿灯和 Windows 窗口按钮错位。
- **主题**：`<html data-theme>` 选择 CSS 变量组，样式中不写死颜色；原生窗口用 `setTheme` 同步原生菜单与对话框（Windows 上还决定 WebView2 的 `prefers-color-scheme`），用 `setBackgroundColor` 同步调整大小时露出的背景；首帧背景由 `index.html` 的内联样式按系统外观给出，避免闪烁。终端配色、字体等通过 `term.options` 应用到所有已打开的终端。
- **会话与文件夹**：会话的 `folder` 字段指向所在文件夹，文件夹存在 `folders.json`（`parent` 可嵌套）。文件中的顺序即显示顺序，每个文件夹里先列子文件夹、再列会话；拖拽只有一个后端操作 `tree_move`（放进某文件夹、排在某项之前或末尾）；拖到展开且有子项的文件夹下沿算放进去（指示线画在文件夹与第一个子项之间，「之后」却会排到整棵子树后面），最近连接区不接受放下。删除文件夹时其中的内容移到上一级，不删除会话。加载时修正指向不存在文件夹的引用和循环。文件夹折叠状态与最近连接是本机的界面状态，与侧栏宽度一样存在 localStorage。
- **导出 / 导入**：导出为 JSON（`format: "zshell-sessions"`、`version: 2`，含文件夹、会话与全部代理，不含密码）。先读格式与版本，2.0 之前导出的 `version: 1` 文件不能导入（`import.unsupportedVersion`）。导入时同名、否则同地址（同协议的用户、主机、端口；串口为同一设备）的会话视为已存在，不再导入；被选中会话的跳板机一并导入，已存在的则引用现有会话；导入的会话用到的代理也一并导入，同名、否则同地址（同类型的主机、端口、用户名；命令类型为同一命令）的代理视为已存在；导入列表里显示命令类型代理的命令原文与自动启动的转发规则，因为导入后连接时会执行、监听它们。同名只在协议相同时算已存在（不让同名的 Telnet 会话顶替 SSH 跳板机）。文件夹按名称路径合并；导入的会话与代理一律分配新 id。文件可能是手写的或别人的：缺失或重复的 id 按位置补上，导入的会话、代理与转发规则都经过与保存时相同的校验和整理（如空的绑定地址改为 localhost），加载配置时也去掉指向不存在、非 SSH 或自身的跳板机。
- **快速连接**：标签目标 `quick`（`session_open` 的 `quick`）不需要已保存的会话。SSH 用"自动"认证，未写用户名时用本机用户名（同 `ssh host`）；Telnet 写了用户名时在登录提示处填入。"另存为会话"把标签目标改为新会话（协议、地址预先填好）但不重连（窗格的终端与 `PaneSession` 随窗格存在，每次连接时才读取目标），下次连接起使用会话的设置。
- **会话日志**：后端在 `SessionSink::write` 里把终端显示的全部内容（远端输出、回显、我们自己的提示，不含 ZMODEM 数据）交给日志的写入线程，不经过前端。队列有上限，日志目录慢（网络盘、休眠的磁盘）时丢弃放不下的输出并在日志里记一行缺了多少，不拖住会话；输出停顿时由写入线程自己刷盘；重连后续写同一个文件时，先等上一个连接的写入线程写完。所以密码等不回显的输入不会进日志。纯文本格式用 vte 解析，维护单行的单元格与光标列，应用回车、退格、光标左右移动与行内擦除，使 shell 的行编辑和进度条得到屏幕上的最终结果；原始格式原样写入。新会话打开时由前端说明日志如何开始（`LogOpen`）：按会话的 `autoLog` 或设置中的"自动记录本地终端"、续写（重连时接着写同一个文件并插入重连标记；路径由前端传回，只接受索引里有的文件，否则改为新建）或不记录（该标签手动停止过）；日志在会话启动前就开始写，所以包含最早的连接提示。ZShell 写过的日志记在 `logs.json`，按天数清理（启动时、之后每 6 小时、修改设置时）、删除某会话的日志、删除全部都只针对索引中的文件，并跳过正在写的文件，日志目录中的其他文件不会被删除。默认目录为"文档/ZShellLogs"。
- **ssh_config 导入**：一次性复制，导入后与 config 文件无关联。与已有会话同名或同地址的主机不再导入；`ForwardAgent`、`SetEnv` 一并映射，不支持的选项（SendEnv 等）在列表中标出。`ProxyCommand` 导入为命令类型的代理，命令相同的共用一个（也复用已有的同命令代理）；`ssh [-q] [-l user] [-p port] -W %h:%p host` 这种 ProxyJump 的旧写法识别为跳板机。OpenSSH 中 ProxyJump 与 ProxyCommand 以先出现者为准，解析库不保留顺序，两者并存时取 ProxyJump，ProxyCommand 标为未导入。解析库不认识 `Match`，会把其下的选项算进前一个 `Host`，所以交给它之前先预处理：`Include` 由我们按 OpenSSH 的方式内联展开（相对路径基于 `~/.ssh`，支持通配，最多 16 层），`Match` 行换成不匹配任何别名的 `Host` 行，有 `Match` 时每个主机都标出未导入。`IdentityFile` 在家目录下的存为 `~/…`（解析库会展开 `~`），导出到别的机器或用户名下仍可用。ProxyJump 的条目是别名时用别名的地址与认证（`admin@bastion:2222` 只改用户名与端口）；跳板机自己的 ProxyJump 展开到列表前面，因为 ZShell 不展开跳板机的跳板机。
- **`~` 与主目录**：私钥路径开头的 `~/`（或 `~\`）由后端展开为 `std::env::home_dir()`（Windows 上是 `USERPROFILE`，即 `C:\Users\<用户名>`），`~/.ssh` 下的 known_hosts、config 与默认私钥同理，与 OpenSSH 用同一个目录。用"选择…"挑的私钥在主目录下时存成 `~/…`（正斜杠，两个平台都认），导出的会话换一台电脑、换一个用户名也能用。界面上出现 `~` 的地方有问号图标，悬停显示实际路径；终端里的主机密钥告警直接写实际路径。
- **单会话外观**：会话的 `appearance` 可覆盖配色、背景色、字体和字号，未设置的项跟随设置。只由前端解释（`sessionScheme`），编辑后已打开的标签立即应用；快速连接与本地终端总是跟随设置。设了背景色的会话，标签左侧有一条同色相、固定中等亮度的色条，深色、浅色背景都能看清。
- **登录后命令**：由前端发送。每个新 shell（首次连接、重连、复制标签）收到 `Connected` 后开始：远端输出停顿 300 ms，且光标前的文字像提示符（非空、不以 `:` 或 `?` 结尾，这样 `sudo -i` 能先问完密码）时，才发下一条。按 Ctrl+C 放弃剩下的命令。

## 交互约定

### 无障碍

目标是读屏用户（VoiceOver、讲述人、NVDA）能完成连接、操作终端、使用 SFTP 与主要对话框，全部功能可以只用键盘完成；不追求逐条符合 WCAG。

- **终端**：设置 › 终端的"读屏支持"打开 xterm.js 的 `screenReaderMode`（与屏幕同步的行列表，新输出经 live region 播报），有性能开销，默认关。读屏念出的输入框名称与"输出过多"提示经 `Terminal.strings` 翻译。系统要求更高对比度（`prefers-contrast: more`、`forced-colors: active`）时设 `minimumContrastRatio` 为 4.5，配色里太接近背景的前景色在绘制时调整，配色本身不变；平时为 1，不改变用户选的配色。
- **对话框**：`Modal` 是 `role="dialog"`（确认类是 `alertdialog`，`aria-describedby` 指向 `.dialog-message`）加 `aria-modal`，名称取自其中的 `h2`（没有标题的用 `label`）。打开时聚焦第一个控件（控件自己 `autoFocus` 的除外），Tab / Shift+Tab 由对话框栈在最上层对话框内循环，关闭后若焦点落空则还给打开前的元素。不给应用根元素加 `inert`：Windows 自绘的窗口按钮在根元素里，对话框打开时也要能用。右键菜单同样在关闭后归还焦点，用 `aria-activedescendant` 指向高亮项。
- **列表**：焦点停在列表容器上，用 `aria-activedescendant` 指向当前行，方向键移动。标签栏是 `tablist`，方向键只移动焦点，Enter / 空格才切换（切换会把焦点交给终端）；会话列表是 `tree`（搜索时是 `listbox`，搜索框是 `combobox`）；文件列表是多选的 `grid`，表头是按钮以便键盘排序。WebKit 把子元素里有 treeitem / option、group、presentation 之外元素的树或列表当作普通分组，所以分节标题用 `role="presentation"`，没有项目、只显示提示文字的列表不设角色。各处的菜单都可以用 Shift+F10 或菜单键打开。键盘从 ⌘K / Ctrl+Shift+K（会话搜索）进入界面，Tab 依次经过会话列表、标签栏与工具栏。
- **播报**：`lib/announce.ts` 是全局唯一的 `aria-live="polite"` 区域，每条消息是新加的一行（同时到来或重复的消息都会念出），播报界面上一闪而过或不在视线内的变化：焦点之外的远端窗格连上或断开（有焦点的窗格在终端里自己说）、传输完成或失败、从菜单复制了地址或路径。文字走 `t()`。
- **样式**：控件的键盘焦点由 `:where(button, a, summary, [tabindex]):focus-visible` 统一画强调色轮廓，特异性低，自己用边框或选中行表示焦点的组件写 `outline: none` 覆盖。`prefers-reduced-motion` 时去掉过渡与闪烁动画。Windows 对比度主题（`forced-colors`）下只靠背景表示的状态（当前标签、选中行、打开的选项）改用 `Highlight` / `HighlightText`。主题颜色的文字与背景对比度至少 4.5:1；白字所在的填充色用比 `--accent` 深的 `--accent-fill`（`--danger-fill` 同理），`--accent` 留给背景上的文字与标记。
- **验证**：macOS 上可以用 `osascript -l JavaScript` 经 System Events 读出窗口的辅助功能树（需要给 osascript 辅助功能权限），查看实际的角色与名称；读屏的实际体验仍要用 VoiceOver、讲述人、NVDA 试。

- **快捷键**：应用快捷键在 window 的捕获阶段拦截（`useShortcuts`），终端收不到。Windows 上与 shell 冲突的快捷键加 Shift（Ctrl+F、Ctrl+W 等留给 shell）。对话框（或命令面板）打开时，除打开设置外的应用快捷键都不处理，菜单的「关闭」也不关标签，否则会切换或关闭对话框背后的标签，并把焦点移到终端。剪贴板快捷键只在终端获得焦点时生效（xterm.js 的 `attachCustomKeyEventHandler`），输入框里仍是普通的复制粘贴。设置里的 Keyboard Shortcuts 一节列出当前平台的快捷键，按键文字取自 `lib/platform.ts` 的 `*ShortcutLabel`，改快捷键时一并更新。

  | 功能 | macOS | Windows |
  |---|---|---|
  | 复制 | ⌘C（原生菜单项） | Ctrl+Shift+C、Ctrl+Insert；有选区时 Ctrl+C（复制后清除选区，无选区时仍发 `^C`） |
  | 粘贴 | ⌘V（原生菜单项） | Ctrl+Shift+V、Ctrl+V、Shift+Insert |
  | 切换标签 | Ctrl+Tab / Ctrl+Shift+Tab | 同左 |
  | 跳到第 1–8 个 / 最后一个标签 | ⌘1–8 / ⌘9 | Alt+1–8 / Alt+9 |
  | 新建本地终端 | ⌘T | Ctrl+Shift+T |
  | 关闭标签（分屏时关闭当前窗格） | ⌘W（原生菜单项） | Ctrl+Shift+W |
  | 向右 / 向下分屏 | ⌘D / ⇧⌘D | Alt+Shift+= / Alt+Shift+- |
  | 焦点移到相邻窗格 | ⌥⌘ + 方向键 | Ctrl+Alt + 方向键（Alt + 方向键是 shell 的按词移动） |
  | 关闭窗口 | ⇧⌘W（原生菜单项） | — |
  | 搜索会话 | ⌘K | Ctrl+Shift+K |
  | 设置 | ⌘,（原生菜单项） | Ctrl+, |
  | 终端搜索 | ⌘F | Ctrl+Shift+F |
  | 撰写栏 | ⇧⌘I | Ctrl+Shift+I |
  | 快速命令面板 | ⇧⌘J | Ctrl+Shift+J |
  | SFTP 面板 | ⇧⌘E | Ctrl+Shift+E |
  | 端口转发面板 | ⇧⌘P | Ctrl+Shift+P |

- **菜单**：macOS 保留原生菜单栏（WKWebView 的 ⌘C / ⌘V / ⌘A / ⌘Q、⌘, 都依赖原生菜单项），由 `lib.rs` 显式构建：去掉预置的 "Close Window"（它占用 ⌘W），File 菜单改为 Close（⌘W，交给前端：分屏的标签关闭当前窗格，否则关闭标签，可能先确认；没有标签时关闭窗口）、Close Window（⇧⌘W），与 Terminal.app 一致；应用菜单的 Quit（⌘Q）也换成自定义菜单项，关闭窗口而不是直接退出，以便前端先确认。Windows 不显示菜单栏。
- **悬停提示**：WKWebView 不显示 `title` 提示，所以由页面自己画（`Tooltips`，挂在 `App` 上）：指针停在带 `title` 的元素上 0.2 秒后显示，点击、按键、滚动或离开即消失。悬停期间暂时摘掉该元素的 `title`，免得 WebView2 同时显示原生提示，离开后放回。组件照常写 `title` 即可。说明性的问号图标（`HelpTip`）用同样的提示框，但立即显示，也能用键盘聚焦。
- **右键菜单**：终端和标签的右键菜单是页面内绘制的 `ContextMenu`（跟随主题，两个平台一致），不用原生菜单。WebView 自带的右键菜单（重新载入、检查元素等）全局屏蔽，只有文本输入框保留系统的编辑菜单。
- **滚动条**：终端的滚动条由 xterm.js 自绘，颜色取配色的前景色。其余区域用系统滚动条，`color-scheme` 让它们跟随深浅色：macOS 是系统的悬浮滚动条；Windows 通过窗口配置 `scrollBarStyle: fluentOverlay` 使用 WebView2 的 Fluent 悬浮滚动条（需要 WebView2 Runtime 125 及以上，否则为默认的经典样式）。不用 `::-webkit-scrollbar` 统一自绘，以免 macOS 失去原生悬浮滚动条。标签栏放不下时横向滚动但不显示滚动条（滚轮、触控板，当前标签自动滚入视野），"文件 / 转发"切换按钮固定在右侧。
- **入口**：本地终端在标签栏末尾的 "+"；设置在侧栏底部和 macOS 应用菜单中；新建会话、导入导出在侧栏顶行，新建文件夹在会话列表空白处和文件夹的右键菜单中。
- **会话列表**（Xshell 风格）：单击选中，双击或 Enter 在新标签中打开；单击文件夹折叠 / 展开，双击打开其中全部会话（超过 5 个先确认）。右键"连接"在该会话已有标签时切换过去，"在新标签中连接"总是新开。键盘：↑↓ 移动，←→ 折叠展开（← 在会话上回到所在文件夹）。顶行的"+"把新会话放进当前选中的文件夹。会话后面显示地址：SSH 为 `user@host[:port]`，Telnet 为 `telnet://[user@]host[:port]`，串口为 `设备 · 115200 8N1`；标签的悬停提示也显示它。搜索框按名称、地址、用户名过滤（多个词同时满足），↑↓ 与 Enter 打开；没有匹配且输入像地址（`[user@]host[:port]`，含 `@`、`.` 或端口；或以 `telnet ` / `telnet://` 开头）时提供快速连接。

### 标题栏

- **布局**：没有原生标题栏，窗口顶部一行（`--titlebar-height`，34px）在侧栏右边界处分为侧栏顶行与标签栏，侧栏从窗口顶部延伸到底部。标签栏始终显示，没有标签页时也有 "+" 和拖动区域。
- **窗口**：主窗口在配置中 `create: false`，由 `window.rs` 按平台创建：macOS 用 `titleBarStyle: Overlay` 并隐藏标题，红绿灯（`trafficLightPosition`）垂直居中于顶行，侧栏顶行左侧为其留白（全屏时取消）；没有红绿灯时（Windows、macOS 全屏）这里显示应用图标和名称，如同原生标题栏，侧栏较窄时只留图标；Windows `decorations: false`，阴影与圆角由 Tauri 的无边框阴影提供，顶边缩放由 Tauri 盖在 WebView2 上的子窗口处理。其他平台保留原生标题栏。
- **拖动**：不用 Tauri 的 `data-tauri-drag-region`，由 `lib/window.ts` 在捕获阶段处理带 `data-window-drag` 的元素（只算直接按在元素本身上）：单击 `startDragging()`（Windows 的贴靠、拖动还原等由系统处理），双击交给后端——macOS 按系统设置"连按窗口标题栏以…"（`AppleActionOnDoubleClick`）缩放或最小化，且与原生一致在松开时执行；Windows 切换最大化；Windows 右键弹出窗口的系统菜单。标签条先于其后的拖动间隔收缩，标签再多也留有拖动区域。对话框的遮罩从标题栏下方开始，遮罩顶部那一条仍可拖动窗口。
- **Windows 窗口按钮**：`WindowControls` 自绘，图标用系统字体 Segoe Fluent Icons / Segoe MDL2 Assets，层级高于对话框遮罩。Windows 11 的贴靠布局要求对最大化按钮位置的 `WM_NCHITTEST` 返回 `HTMAXBUTTON`，而 WebView2 的子窗口接收全部鼠标消息，所以在按钮上方放一个不绘制的原生子窗口返回它；位置由前端量出后发给后端（`window_set_maximize_button`），悬停 / 按下状态由后端发事件给前端设置样式，点击由后端发 `SC_MAXIMIZE` / `SC_RESTORE`。Windows 11 给非最大化窗口画的 1px 边框在贴靠到屏幕一部分时仍然保留，沿屏幕边缘看起来像没填满；这 1px 是 WebView 之外的窗口边框本身，去掉颜色只会露出透明，要让网页覆盖它须接管 Tao 的边框计算。所以窗口移动或改变大小时用 `IsWindowArranged`（运行时查找，较老的 Windows 10 没有）判断是否贴靠，贴靠时用 `DWMWA_BORDER_COLOR` 把边框设成标题栏的颜色（`--bg-sidebar`，随主题更新），否则恢复系统默认。

### 剪贴板

- 读写剪贴板用 `tauri-plugin-clipboard-manager`：右键菜单的粘贴、右键粘贴无法触发浏览器的 paste 事件，WKWebView 的 `navigator.clipboard.readText` 又会弹出系统的"粘贴"确认。macOS 的 ⌘C / ⌘V 仍走原生菜单与浏览器的 copy / paste 事件。
- 所有粘贴走同一入口：在终端容器的捕获阶段拦截 paste 事件，交给 xterm.js 的 `term.paste()`（处理换行转换与 bracketed paste）。内容含换行且远端未开启 bracketed paste（`term.modes.bracketedPasteMode`）时先弹框确认；开启时 shell 只插入不执行，无需确认。
- 选中即复制（默认关）：鼠标选择结束（左键松开）时复制，不在拖动过程中反复写剪贴板。
- 右键行为：菜单（默认）或粘贴。远端程序开启鼠标模式（vim、tmux）时右键交给程序，Shift+右键总是打开菜单。

### 终端键盘与鼠标

- **Option 作为 Meta**（仅 macOS，默认关）：xterm.js 的 `macOptionIsMeta`。`macOptionClickForcesSelection` 始终开启，否则 macOS 上在开启鼠标模式的程序里无法选择文本（其他平台用 Shift）。

### 标签页

- **分屏**：一个标签是一棵窗格树（`Tab.layout`：窗格，或横排 / 竖排的若干子节点与各自的比例），每个窗格是独立的会话。切分时新窗格与原窗格平分原窗格的空间，与父节点同方向时成为兄弟节点；关闭窗格时空间给前一个兄弟（没有则后一个），只剩一个子节点的节点被它替代，同方向的嵌套合并。标签页内的窗格按打开顺序平铺渲染、按树算出的百分比绝对定位，所以切分、关闭、调整大小都不会重新挂载其他窗格的终端（卸载即结束会话）；窗格不能在标签之间移动。新窗格默认复制当前窗格（已连接的 SSH 复用连接；串口不能复制），也可以从会话列表在新窗格中连接。拖动分隔条时两侧窗格保持最小尺寸，双击分隔条均分；太小的窗格不能再切分。布局不保存（标签页本来就不跨重启保留）。
- **当前窗格**：标签记录有键盘焦点的窗格（点击、快捷键、关闭窗格后由接替其空间的窗格获得），分屏时画一圈强调色细边框。标签的标题、状态点、色条、日志标记显示当前窗格的，标签右键菜单中针对会话的操作（重连、Break、另存为会话、日志）作用于当前窗格。文件 / 转发面板属于标签，显示当前窗格的连接：每个 SSH 窗格各有一份面板实例（切换焦点时目录与传输都在），当前窗格不是 SSH 时显示"不可用"，已打开的面板仍可关闭。
- **标题**：重命名的标题 > 远端标题（OSC 0 / 2，可在设置中关闭）> 会话名。重命名为空则恢复自动标题；重连时清除旧的远端标题，由新 shell 重新设置。
- **拖拽排序**：用鼠标事件而不是 HTML5 拖放（Windows 上 Tauri 的文件拖入会拦截 HTML5 拖放）；指针越过相邻标签的中点即交换位置。按下标签不夺走终端的焦点。
- **关闭确认**（可关闭）：已连接的远端窗格（SSH、Telnet、串口），或本地终端里有前台程序时确认；文件面板里有进行中的传输时，提示中说明关闭会取消几个传输；关闭标签时看其中所有窗格，关闭多个标签时合并为一次确认。对话框里可勾选"不再询问"，即关闭该设置。关闭窗口（即退出：关闭按钮、⌘Q、Alt+F4）会一次结束全部会话，有这样的标签时总是确认，不受该设置影响，也没有"不再询问"；由前端的 `onCloseRequested` 拦截。macOS 上从程序坞退出、注销或关机由系统直接终止应用，不经过确认（已知限制）。
- **重新连接**：标签右键菜单中，关闭当前会话后立即新建连接；旧会话迟到的输出和事件按连接代号忽略。
- **撰写栏与同步输入**：纯前端实现，按会话 id 直接 `session_write`。发送范围以窗格为单位：当前窗格、当前标签的所有窗格、全部、选中（选择列表里分屏的标签下面列出各窗格，勾选标签即勾选其全部窗格）。范围只在撰写栏打开时生效（`lib/compose.ts` 的 `scopePanes`），关闭后一律只发当前窗格，关闭时同步也随之关闭，避免范围停在"所有标签"而不自知。同步转发的是终端 `onData` 中用户键入的内容，过滤掉鼠标与焦点报告以及终端对程序查询的应答（设备属性、光标位置、状态、模式、窗口大小、OSC 颜色、DCS；每条应答单独一次 `onData`，按格式识别），只从当前窗格转发。粘贴不转发 `onData`（那是按当前窗格的模式包过 bracketed paste 标记的），而是交给每个同步窗格的终端各自 `term.paste()`（各窗格向 App 登记一个粘贴入口）；多行粘贴的确认看所有目标：只要有一个窗格没开 bracketed paste 就先询问，确认后一起粘贴。范围超出当前窗格时，含范围内窗格的标签顶部有标记（同步时为警示色），收到内容的标签闪烁；分屏的标签里，范围内的窗格顶部同样有标记、收到内容时闪烁；同步时当前终端四周有警示色边框（画在终端画布之上的一层，`outline` 会被 WebGL 画布遮住）。撰写栏历史只保存在内存中。
- **快速命令**：存在 `commands.json`，按分组保存；默认分组（id `default`，名称显示为翻译文本）总是存在且排第一，删除其他分组时其中的命令移入默认分组。前端整体编辑后整体保存，后端补 id、修正重复。会话的 `commandGroup` 是其标签默认显示的分组，标签里手动切换的分组只记在该标签上；分组被删除后回退到默认分组。快速命令与撰写栏共用发送逻辑（`scopeTabs`），撰写栏打开且范围不是当前标签时按钮栏和命令面板变为警示色并列出目标标签。入口：终端下方的按钮栏（显示与否是本机界面状态，存在 localStorage）、终端右键菜单（当前分组的前 8 条）、命令面板。
- **终端内操作**：连接结束后按 Enter 重连 / 重试 / 重启 shell，提示以暗色文字写在终端里。

## 平台注意事项

- macOS 上 xterm.js 运行在 WKWebView 里，CJK 输入法（候选框位置、组字过程）需在真机上测试。输入框与对话框里处理 Enter / Esc 前先用 `lib/platform.ts` 的 `isComposing` 判断是否在组字（确认候选的 Enter、取消候选的 Esc 不应提交或关闭）；WebKit 在确认候选的 Enter 的 keydown 之前就结束了组字，只能靠 `keyCode` 229 识别。
- macOS 默认的"按住按键显示重音字符"会让 WKWebView 里长按字母键不重复。应用启动时以注册默认值把 `ApplePressAndHoldEnabled` 设为关闭（与 Terminal.app / iTerm2 一致），用户仍可用 `defaults write org.boyin.zshell ApplePressAndHoldEnabled -bool true` 恢复。
- 中文 / emoji 宽度依赖 unicode11 插件。远端不是 UTF-8 时在会话配置里选编码。
- ConPTY 需要 Windows 10 1809 及以上。Windows PowerShell 5.1 调用的原生程序可能按 OEM 代码页输出导致中文乱码，暂不修改用户的 `[Console]::OutputEncoding`（已知限制）。
- ssh-agent：macOS 用 `SSH_AUTH_SOCK`；Windows 用 OpenSSH 命名管道或 Pageant。

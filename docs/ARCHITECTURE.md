# ZShell 架构

个人使用的跨平台（macOS / Windows / Linux）SSH / Telnet / 串口终端。本文只记录跨模块的设计决策和理由；模块内的细节（边界情况、库的坑、具体数值）写在代码注释里，这里只指向文件。

| 文档 | 内容 |
|---|---|
| [ROADMAP.md](ROADMAP.md) | 产品规划：现状、里程碑、未实现功能的设计要点 |
| ARCHITECTURE.md | 总体结构与跨模块的设计决策 |
| [I18N.md](I18N.md) | 语言约定与国际化 |
| [DEVELOPMENT.md](DEVELOPMENT.md) | 开发环境、测试、约定、CI 与发布 |
| [TODO.md](TODO.md) | 暂时搁置的已知问题，多数在等上游修复 |

## 技术栈

| 层 | 选型 |
|---|---|
| 应用框架 | Tauri 2 |
| 后端 | Rust + tokio |
| SSH / SFTP | `russh`（`ring` 加密后端，Windows 上不依赖 CMake/NASM）/ `russh-sftp` |
| 凭据 | `keyring`（macOS Keychain / Windows Credential Manager / Linux Secret Service） |
| 加密 | `ring`（AES-256-GCM、PBKDF2-HMAC-SHA256、随机数），`zeroize` |
| ssh_config 导入 | `ssh2-config` |
| 本地终端 | `portable-pty`（Windows 走 ConPTY） |
| 串口 | `serialport` |
| Linux 桌面 | `gtk`（标题栏、拖出文件）、`zbus`（设置 portal、polkit、logind、Mutter） |
| Telnet | 自己实现 |
| 前端 | Vite + React + TypeScript |
| 终端渲染 | `@xterm/xterm` + fit / webgl / unicode11 / web-links / search 插件 |
| 终端连字 | 自己实现的 `cmap` / `GDEF` / `GSUB` 读取（后端 `fonts/`）与替换（`lib/fontLigatures.ts`） |

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
│ auth / known_hosts / vault / config / settings  │
└─────────────────────────────────────────────────┘
```

| 模块（`src-tauri/src/`） | 职责 |
|---|---|
| `session/` | 会话抽象：`TermIo`、`SessionManager`、`SessionSink`、输出流控 `Flow` |
| `ssh/` | 连接、shell、认证、主机密钥与 known_hosts；不认识连接上跑的其他功能 |
| `connections/` | 会话 id → 连接及其上的 SFTP 与转发规则；ssh、sftp、forward 在这里组合 |
| `sftp/` | 浏览与文件操作、传输、本地编辑、拖出、替换写入、文件名转码 |
| `forward/` | 转发规则的运行，`-L` / `-R` / `-D`，SOCKS |
| `traffic.rs` | 各通道收发的计数与节流上报，连接编号 |
| `pty/` | 本地终端，默认 shell 与环境变量 |
| `telnet/` | Telnet 会话、自动登录、协议与选项协商 |
| `serial/` | 串口会话与设备列表 |
| `net.rs` / `proxy/` | 出站连接（直连或经代理）、`Route`；SOCKS5、HTTP CONNECT、ProxyCommand |
| `zmodem/` | rz / sz：检测与接管、帧格式、收发 |
| `logging/` | 会话日志、索引 `logs.json` 与清理 |
| `debug_log/` | 调试日志：`log` 的 logger、内存里的记录与发给日志窗口（见「调试日志」） |
| `model/` | 保存的数据类型及其校验；只依赖 serde、ts-rs 与基础工具，功能模块依赖它，反之不然 |
| `config/` / `quick.rs` / `settings.rs` | 会话、文件夹与代理；快速命令；`settings.json` |
| `vault/` / `envelope.rs` / `crypto.rs` / `secrets.rs` | 保险库；保险库与导出文件共用的信封格式；加密与口令派生；系统凭据存储 |
| `applock/` | 应用锁：锁定状态、系统认证、闲置与锁屏检测 |
| `keep_awake/` | 连接期间阻止系统睡眠，电源检测（见「阻止睡眠」） |
| `persist.rs` | JSON 文件读写：读不出的文件、原子写入 |
| `encoding.rs` / `local_name.rs` | 会话的字符编码；对方给出的文件名转成本地文件名 |
| `import/` / `backup.rs` | ssh_config 导入；会话导出与导入 |
| `window/` | 按平台创建窗口；各平台文件提供同一组函数，缺了哪个都编译不过 |
| `desktop.rs` / `dbus.rs` | Linux 桌面的配色方案与等宽字体（设置 portal）；应用锁与阻止睡眠共用的 D-Bus 连接（方法调用 25 秒超时；应用锁的认证对话框不限时，另开连接） |
| `hidden_window.rs` | Windows：接收系统广播的隐藏窗口，应用锁、读屏检测与电源检测共用这个模块，各自在自己的线程里建一个 |
| `fonts/` | 终端字体的连字规则（见「终端连字」） |
| `screen_reader/` | 系统读屏软件是否在运行（见「无障碍」） |
| `tab_drag/` | 标签拖出窗口时的落点（见「多窗口」） |
| `menu.rs` | macOS 原生菜单栏（见「快捷键与菜单」） |
| `instance.rs` | macOS 与 Linux 的单实例 |
| `update.rs` | 检查更新 |
| `i18n.rs` / `error.rs` | 后端消息目录、结构化错误（见 [I18N.md](I18N.md)） |
| `crash.rs` | release 构建 panic 即退出（`panic = "abort"`，展开会让 panic 时持有的锁中毒，应用半坏），退出前把 panic 记进配置目录的 `crash.log` |
| `events.rs` | 后端发给页面的事件，名字与 payload 类型在一处声明 |
| `launch.rs` | 把前端的 `SessionSpec` 解析成后端（`Backend`）、日志命名与编码；`session/` 不认识具体后端 |
| `commands/` | 前端能调用的全部命令，只做检查调用方、交给所属模块、通知其他窗口；跨模块的策略放在所属模块（会话变化时转发怎么办在 `connections/`，下载目录在 `settings.rs`） |
| `bindings.rs` | 生成前端的类型与共用常量（`src/ipc/bindings.ts`） |

前端（`src/`）：

- **分层**：`lib/` 是不用 React 的逻辑，调用后端只经 `ipc/`；`ipc/` 是唯一调用命令、监听事件的地方（Tauri 自己的窗口与插件 API，如剪贴板、打开链接、拖放事件、`api/app`、`api/path`，组件可以直接用，它们内部的命令不算在内）；`hooks/` 是组件共用的 store、provider 与 hook（只属于 `App` 的在 `app/`）。`layers.test.ts` 检查依赖方向。
- **外部 store**：跨组件共享的状态是每个窗口一份的 store（`lib/store.ts`），组件经 `useSyncExternalStore` 订阅，模块直接 `get()`，不经 `App` 转交。整体保存到后端的值（设置、快速命令）经 `lib/savedStore.ts` 与 `latestSave`。设置同样不经 `App` 转交：渲染时用到的经 `useSettings()` 订阅，只在事件与回调里用到的直接读 `currentSettings()`。谁读入谁负责重读：收到其他窗口的 `config-changed` 时重读。
- **标签页与窗格**：每个修改是一个 action，由纯函数 `reduceTabs`（`lib/tabs.ts`）处理；`App` 的各流程各是 `app/` 下的一个 hook，流程要问用户或显示的对话框、面板由这个 hook 自己保管状态并返回要渲染的元素（`dialog`），`App` 只负责组装和摆放。
- **会话目录**：已保存的会话、文件夹、代理和最近打开的会话（`lib/directory.ts`），用到的组件直接订阅。最近打开的存在 `localStorage`，经 `storage` 事件在窗口间同步。
- **窗格的会话**：每个窗格一个 `PaneSession`（`lib/paneSession.ts`），负责打开、重连、共享连接、登录后命令和流控，只经小接口读写终端；`TerminalView` 只管终端，它的各部分（链接、剪贴板与右键菜单、尺寸跟随、回看在 `components/terminal/`，OSC 与提示符检测在 `lib/`）各返回一个 disposable，收进 `Disposables`（`lib/disposables.ts`）一起释放。注册表（`lib/sessionRegistry.ts`）以事件通知拥有关闭、移动流程的 hook，依赖只从拥有者指向它。
- **会话结束**：和 Xshell 一样，用户退出 shell 后窗格自动关闭；服务器一连上就结束、被信号结束或文件面板有工作时不关（`closesOnExit`，`lib/panes.ts`），这时 Enter 重连。
- **挂载顺序**：`SettingsProvider` → `VaultGate`（锁定时只显示解锁界面）→ `AppLockGate` → `App`。
- **对话框与快捷键**：对话框登记到一个栈（`hooks/dialogs.ts`），只有最上层响应 Esc、点击背景和关闭标签页的快捷键（⌘W，其他平台 Ctrl+Shift+W，由 `Modal` 处理）；快捷键在捕获阶段统一分发（`hooks/shortcuts.ts`）。Esc 和关闭标签页的快捷键等同于取消按钮；点击背景只关闭关掉也不丢东西的对话框（`dismissable`：命令面板、设置、已知主机、许可证），确认问题和表单与原生对话框一样不理会，免得误点一下就替用户作答或丢掉输入。对话框正在保存或执行操作时（`persistent`），取消按钮禁用，三种方式都关不掉。
- **样式**：`styles.css` 按顺序引入 `styles/` 下的各部分，先后即层叠顺序，所以新规则写进所属部分，不由组件自己引入。

## 设计决策

### 会话与数据流

- **前后端类型**：Rust 类型经 ts-rs 生成提交到仓库的 `src/ipc/bindings.ts`，过时时 `cargo test` 失败；错误码、两边共用的常量、命令签名（`bindings.rs` 用 `syn` 读出）与事件的 payload 也从后端生成，前端只经 `ipc/call.ts` 的 `call` 和 `listenHere` / `useWindowEvent` 使用，由 `tsc` 检查。命令一律返回带错误码的 `CommandError`。
- **打开会话**：只有一个命令 `session_open`，参数 `SessionSpec` 由 `launch.rs` 解析，日志与会话任务只在一处建立。
- **会话是后台任务**：`SessionManager` 为每个会话创建 `TermIo`（输出 / 事件 Channel、输入队列）；新后端只需实现同样的任务形态。关闭只经 `SessionManager`，任务之外的资源登记在会话的 `Lifetime` 上，无论怎样结束都只释放一次。
- **会话属于 webview**：页面重载或窗口销毁时关闭它的全部会话。按会话 id 操作的命令只接受归属窗口的调用，其他窗口得到与"不存在"相同的 `session.notFound`。移动标签的几个命令是例外：`session_ack` 对非归属窗口静默返回成功（会话刚移走时旧窗口的确认），`session_handover` 只接受迁出的窗口，`session_adopt` 只接受被 offer 的窗口；传输与本地编辑同样属于发起它们的页面。会话可以移到另一个窗口（见「多窗口」）。
- **终端输出走 Channel**：原始字节直接 `term.write`，会话事件走同一个 Channel，保证"已关闭"在最后一段输出之后；输入走 `invoke`。各后端把已就绪的输出合成一条再发，减少主线程上的消息数。
- **背压**：输出通路都有上限。前端按已处理字节确认输出，未确认过多时后端暂停读取（`Flow`），因为 Tauri 的 IPC 队列与 xterm.js 的积压都不设防；输入不设上限：按键、粘贴与尺寸变化经无界队列交给各后端，对方不读时留在后端（ZMODEM 的数据另有 `INPUT_BACKLOG`），这样按键与尺寸变化总能读到，Ctrl+C 不会被卡住。代价是暂停读 SSH shell 时同一连接上的 SFTP 和转发一起等。例外是 Windows 与 Linux 上拖出文件时主线程等下载：这期间输出显示不出来也就没有确认，SSH 照常读（`ssh::KeepReading`），否则下载所在的连接停住，主线程要等到 SFTP 请求超时。阈值见 `session/`。
- **字符编码**：内部一律 UTF-8，只在与远端交界处（输出、输入、SFTP 与 ZMODEM 的文件名）按会话的 `encoding` 转换。快速连接、本地终端与认证提示总是 UTF-8。
- **会话事件**：`Closed` 的 `reason` 区分 `exited`、`lost`、`failed`，前端据此决定重连、自动关闭或提示。
- **终端里我们自己的消息**：连接中、会话如何结束、等待转发连接、转发规则启动失败、ZMODEM 传输，以及前端写的重连倒计时与操作提示，每行以打印时的本地时间开头，格式与日志的按行时间共用设置 `logs.timeFormat`（见「会话日志」）。后端走 `SessionSink::notice`（`session/notice.rs`），前端走 `lib/terminalNotice.ts`，两边测试同一个例子。照 OpenSSH 的登录问答（主机密钥、密码、口令、keyboard-interactive）、输入回显和代理命令的 stderr 仍走 `print`，不加时间。日志按行写时间时，日志里不再写这个前缀，避免两个时间。
- **线程与写入顺序**：同步命令跑在主线程，所以碰文件、凭据存储或设备的命令是 async 的（`commands/mod.rs`）。整体替换的写入由 `lib/latestSave.ts` 排队：同一时间只有一个请求，保存的是"修改"而不是整份值，两个窗口改的不同项都留下。转发规则逐条保存。
- **内容安全策略**：严格的 CSP 作为纵深防御；样式需要 `unsafe-inline`（xterm.js），为此关掉 Tauri 给 `style-src` 加的 nonce。

### SSH

- **一个连接多路复用**：认证后连接登记到 `connections::Connections`，SFTP 与转发开在同一连接上。复制标签与分屏在已有连接上开新的 shell channel（类似 ControlMaster），最后一个会话结束时才断开。
- **提示都在终端里**：主机指纹、密码、口令、keyboard-interactive 都经 `TermIo::read_line` 在终端里问，与 OpenSSH 一致。
- **认证**：按 OpenSSH 的顺序与规则；保存的密码只回答一次，且只回答问本账户密码的提示，验证码类的总在终端里问（`ssh/auth/`）。
- **主机校验**：自己解析 known_hosts 并按 OpenSSH 的规则校验与写入，包括 `@cert-authority` 主机证书（`ssh/known_hosts/`），不用 russh 自带的校验。新密钥写入时，用户与系统的 ssh_config 对该主机设了 `HashKnownHosts yes` 就写 hashed 形式（只看 `Host` 块，`Match` 块当作不适用）；连接的其他选项不读 ssh_config。
- **agent 转发**：只转发给目标主机；会话开启了 agent 转发时才接受服务器打开的 agent 通道（跳板机一律拒绝），没有本地 agent 时也拒绝。
- **ProxyJump**：在上一跳连接上开 `direct-tcpip` 作为下一跳的传输层，每一跳用被引用会话自己的地址、认证与主机密钥；SSH 与 Telnet 共用（`ssh::tunnel`）。
- **keepalive 与重连**：keepalive 无响应判定断线（`lost`），前端退避重连到标签关闭，认证或主机密钥被拒绝时停止。重连是新会话。超时与断线的分类见 `net.rs` 与 `ssh/mod.rs`。russh 等待写传输层没有时限：对端无声消失、发送缓冲区满了以后，连接任务会一直卡在写上，keepalive 也发不出去。所以传输层（`ssh/abortable.rs`）自己限时：一次写等不到空间、对端期间什么都没收走，超过 keepalive 间隔的 5 倍（至少 2 分钟）就结束连接；写有进展就重新计时，慢链路不受影响。不用 russh 的 `inactivity_timeout`：它从握手起就计时，停在密码提示或等 Duo 推送的连接也会被断开，而 OpenSSH 会一直等到服务器的 `LoginGraceTime`。
- **及早发现断线**（`ssh/liveness.rs`）：keepalive 要几分钟才判定断线，用户却在几秒内就会发觉。所以在这些时机检查连接：终端输入后（服务器会回显）、每个 SFTP 命令、本地或动态转发开通道时、转发的连接上客户端发来数据时（三种转发都算）、系统从睡眠唤醒时（应用锁的系统事件转给 `Connections::woke`）。检查先等一会儿看服务器有没有发来东西，没有就发一个要回复的 keepalive 再等；判断依据是对端的任何动静（`ssh/abortable.rs` 计数）：传输层读到的字节，以及写挂起后又写了出去（发送缓冲满了以后，只有对端确认才腾得出空间；russh 写的时候不读，慢链路上一大块要写很久），而不只是 keepalive 的回复，所以大流量和慢链路下不会误判。russh 正等着对端（读或写挂起）却一直没动静，并且要么请求之后发出的东西整段等待时间都没回应，要么一直等不到写的空间，才从外部切断传输层，按断线处理、照常重连。russh 被本地卡住（终端积压了输出，不再读通道），或者请求根本没发出去（有通道窗口用完时 russh 扣住我们要发的一切），都不下结论。等待时间固定（输入等之后先等 2 秒、发请求后再等 8 秒，唤醒后是 2 秒和 15 秒），不随 keepalive 设置变化：网络中断超过这个时间的连接会被断开重连，哪怕网络恢复后它本来还能用，这是为及早发现断线有意的取舍。同一连接同时只跑一个检查；会话关了 keepalive 时也不检查。
- **测试**：连接上的功能连到进程内的 russh 服务器（`ssh/testing.rs`），从不碰用户的 `~/.ssh`。

### Telnet 与串口

- **会话配置**（2.0 起）：共同字段加按 `protocol` 区分的 `connection`，各后端只接收自己协议的配置。
- **能力**：快速连接支持 SSH 与 Telnet；文件与转发面板、复制标签只用于 SSH；串口设备只能打开一次。
- **Telnet**：协议层是纯状态机（`telnet/protocol.rs`）。保存的用户名与密码由后端在登录提示处填写，不经过前端；本地回显时不回显密码行，也就不进日志。
- **串口**：拔出设备算 `lost`，按断线重连的节奏重试，插回后自动接上。

### 代理

- **配置**：代理是独立保存、有名称的配置，会话按 id 引用；被引用的代理不能删除，免得悄悄变成直连。
- **只用于第一个连接**：与 OpenSSH 一致，取第一台跳板机的设置，没有跳板机时取会话的；快速连接不经代理。`net::connect` 是 SSH 第一跳与 Telnet 的唯一入口。
- **安全**：主机名交给代理解析；ProxyCommand 代入的值不能含 shell 元字符，导入别人的会话文件不应能借此执行命令（`proxy/command.rs`）。

### 文件传输与端口转发

- **SFTP 传输**：先扫描生成计划再复制，小文件并发复制，进度经 Channel 推送；覆盖已有文件时先写临时文件、写到磁盘（远端在服务器支持 `fsync@openssh.com` 时）再替换，替换完成前原文件不变；新建的文件和文件夹带上原来的权限位，与 OpenSSH 的 `sftp put` / `get` 一致（`sftp/transfer/`、`sftp/replace.rs`）。传输结束（完成、失败或取消）后，后端按页面和传输 id 记下这次新建的文件与文件夹（写覆盖的文件只计数），传输列表可以据此永久删除它们：只删这些文件，而且只删大小和修改时间仍与传输结束时相同的（之后被改过的，比如又一次传输写到了同一处，就不再算它的，留着），文件夹在变空后才删；上传的文件在复制完后一并查一次，每个文件一个请求，多个同时发；上传的记录还带着它去的服务器（连接时会话的主机、端口、用户、跳板机与代理），删除时标签页连的已不是它就拒绝；删不掉的留在记录里可以再删，前端累计各次删掉的数量；列表不再显示这一项、页面重载或窗口关闭时忘掉记录，删除进行中被移除的，剩下的也不再放回。路径不经前端，前端拿到的只有数量（`sftp/transfer/left.rs`）。
- **本地文件名**：下载、拖出、本地编辑与 ZMODEM 接收都经 `local_name`，结果总是目标文件夹里的一个文件名；服务器列出的 `.`、`..` 和含 `/` 的条目直接过滤。
- **SFTP 面板**：选择、排序、过滤在前端；条目多时只画视野内的行（`useRowWindow`）。路径栏按 shell 的方式补全文件夹。
- **鼠标侧键**：WebView2 把后退 / 前进键交给页面（`button` 3、4），松开时还会让页面后退，要阻止默认行为；WebKitGTK 不交给页面，macOS 的 WebKit 在 2026 年以前的版本里当作中键交给页面（标签栏上中键会关标签）。所以 macOS 与 Linux 在原生层截下这两个键（`window/macos.rs` 替换 web view 的 `otherMouseDown:` / `otherMouseUp:`，`window/linux.rs` 处理 GTK 的 8、9 号键），不交给页面，改发 `mouse-navigation` 事件带上按下的位置，前端按位置找到下面的元素（`useMouseNavigation`）。目前只有文件面板用它。
- **终端所在的文件夹**：面板可以进入并跟随窗格 shell 所在的文件夹。后端经 SFTP 读 Linux 的 `/proc` 找到它，不在服务器上运行任何东西（`sftp/shell_dir.rs`）；没有 `/proc` 时靠 shell 报告的 OSC 7。
- **临时目录**：交给其他应用的下载放在系统临时目录；Linux 的 `/tmp` 是共享的，所以改用应用的缓存目录（`scratch_dir`）。
- **用本地编辑器编辑**：后端下载并监视保存，上传经标签当前的连接，所以重连后继续有效；远端在下载后被改过时先询问。系统会运行而不是打开的文件类型改用文本编辑器，因为文件名和内容都由服务器决定（`sftp/edit.rs`）。
- **拖出窗口**：只有放到其他应用上、对方真正取数据时才下载（`sftp/drag/`）。
- **端口转发**：规则存在会话配置里；每次运行是一个持有全部资源的任务，停止即释放，端口立即可重用（`forward/mod.rs`）。规则属于会话，同一会话的多个标签里只在一条连接上运行，关闭那个标签时可以移到同会话的其他连接；重连（手动或断线后自动）时把原来在运行的规则带到新连接；删除会话时停止它正在运行的规则，删除前列出用到这些规则的标签请用户确认（`connections/`）。
- **shell 退出后的转发**：连接上最后一个 shell 退出（`exit`、Ctrl+D、被信号终止）时，规则先停止监听，同会话还有其他连接就不问而移过去；已建立的转发连接不切断，参照 OpenSSH 等它们结束再断开。等待期间标签保持打开并显示剩余连接数，Enter 在同一连接上开新 shell（没移走的规则重新启动，连接继续，这个 shell 再退出时一并等待），Ctrl+C 立即断开并留下标签，Ctrl+D 直接关闭标签（点关闭按钮则先确认）。规则被释放时任务交还它承载的连接（`forward/draining.rs`），等待由 SSH 后端调用 `connections/exit.rs`，后端本身不知道转发。关闭仍有转发连接的标签时，确认框说明会断开多少个连接，移交规则也带不走它们。

### ZMODEM

- **位置**：在会话层，所以四种后端都适用。协议自己实现，以 lrzsz 为对照测试。
- **检测与接管**：`SessionSink::output` 识别 `sz` / `rz` 后把输出交给传输任务，误判时原样交还终端；传输计入会话的流控。
- **与前端交互**：经 `Zmodem` 事件询问保存位置或要发送的文件。接收默认先提示，因为 `cat` 一个像 `sz` 输出的文件也会触发。
- **兼容性**：转义全部控制字符（相当于 `rz -e`），以通过过滤控制字符的堡垒机。

### 本地终端

- 作为 `SessionManager` 的另一种后端，沿用 SSH 会话的命令。`session_foreground` 报告前台程序，用于关闭前确认。
- 默认用登录 shell（Windows 优先 PowerShell 7），macOS 补全从 Finder 启动时缺失的 PATH（`pty/shell.rs`）。
- Windows 上把系统的构建号告诉 xterm.js（`windowsPty`）：ConPTY 在终端改变大小时自己重画，21376 之前还自己重排换行，xterm.js 这时不再重排（`pty::conpty_build`）。

### 配置与设置

- **存储**：配置目录以应用名命名（macOS `~/Library/Application Support/ZShell/`，Windows `%APPDATA%\ZShell\`，Linux `~/.config/ZShell/`）。会话、文件夹、代理、快速命令与密码在保险库里；`settings.json` 与 `logs.json` 总是明文。会话与它的密码在同一次写入里保存。
- **单实例**：两个进程会互相覆盖配置。Windows 用 single-instance 插件，macOS 与 Linux 按配置目录加锁（`instance.rs`）。
- **读不出的文件**：`settings.json` 与 `logs.json` 读不出时保留原文件、从空开始并告诉用户，应用照常启动（`persist.rs`）；`vault.json` 读不出时停在解锁界面。
- **设置**：修改即时生效并保存，后端校验。界面文字大小经 `--text-scale` 缩放；主题经 `<html data-theme>` 选择，原生窗口同步跟随。
- **会话与文件夹**：混排，显示顺序单独保存为 `order`，旧版本仍能打开（`config/mod.rs`）。删除文件夹时内容移到上一级。指向不存在的代理的会话报错而不是直连。
- **导出 / 导入**：与 `vault.json` 同样的信封格式，带密码时整个文件用口令加密。已存在的按名称或地址匹配，不重复导入；导入前列出会执行的代理命令与会监听的转发；文件可能是别人的，导入内容经过与保存时相同的校验（`backup.rs`）。
- **会话日志**：后端在 `SessionSink` 里直接写，不经前端，所以不回显的密码不进日志；日志慢时丢弃并记下缺了多少，不拖住会话（`logging/`）。
- **时间格式**：`logs.timeFormat` 是 strftime 的子集，只有数字格式符（`%Y %y %m %d %H %M %S %3f %%`），各语言写出来一样；不允许控制字符，最长 64 个字符。只有后端解析和检查（`model/time_format.rs`），读设置时不能用的格式退回默认值；前端经 `logs_time_format` 拿到解析好的片段，自己只按片段写出时间，不解析格式（`lib/timeFormat.ts`，拿到之前用生成的 `DEFAULT_TIME_FORMAT_PARTS`）。日志按行写的时间是这一行开始显示时的时间，被整行改写（进度行）后重新计。
- **其他**：ssh_config 导入是一次性复制，同样经过保存时的校验（`import/`）；转发的监听地址 `*` 表示所有地址（本地 IPv4 与 IPv6 各监听一个，远程请求发空地址），与 OpenSSH 一致；私钥路径的 `~` 由后端展开，主目录下的存成 `~/…`；单会话的外观覆盖只由前端解释；登录后命令由前端在提示符出现后逐条发送。

### 加密保险库

- **保护方式**（`vault/`）：不加密、系统凭据存储、主密码（可同时存进凭据存储）。都只有一个文件 `vault.json`，方式写在头部。首次启动时凭据存储可用就用它，否则不加密，并当场提示可以改成主密码。
- **范围**：会话、文件夹、代理、快速命令、密码与应用锁的设置。`settings.json`（外观要在解锁前生效）、日志与 known_hosts 不加密。
- **格式**（`envelope.rs`）：加密时头部的原样字节作为 AAD，被改动即解不开。导出文件用同一格式。
- **读写**：修改一次一个，完成后发布快照，查询只读快照，主线程从不等修改。每次修改整体重写文件（数据只有几 KB），换来任何修改都只是一次改名；写出前把要写的内容重新解析，确认（加密时解密后）与要写的数据一致。读不出、解不开或版本不对时从不覆盖。
- **解锁**：启动时只读头部，不碰凭据存储（可能弹授权框或等 D-Bus 超时）；忘记主密码只能重置：旧文件改名为 `vault.json.reset-<时间>` 留在原处，凭据存储里的密钥也留着，想起密码或找回密钥后改回原名还能打开。
- **凭据存储里的密钥**：条目名带密钥 id，更改保护方式时先存新密钥、再替换文件、最后删旧的，任何一步停下文件都打得开。debug 构建用另一个 service。
- **内存中的数据**：只对密钥（包括凭据存储里的文本形式）和要加密的文件明文清零，明文按最终大小一次分配，不留扩容时的副本；解锁后的数据是普通的值，和应用锁一样不防能读进程内存的人。
- **会话读密码**：随 `Route` 交给会话任务（`vault::SavedPasswords`），需要时才读，不经全局变量。
- **凭据存储接口**（`secrets.rs`）：Linux 上可能没有 Secret Service，这时只能选主密码或不加密；存储种类由后端返回，前端不猜平台。

### 应用锁

- **范围**（`applock/`）：盖住窗口直到系统认证通过，会话照常运行；只锁界面，不重新加密内存中的数据。macOS 用 LocalAuthentication，Windows 用 Windows Hello，Linux 用 polkit（需要安装包带的策略文件）。后端报告可用的方式，前端不猜平台。
- **状态在后端**：启动即锁定，页面重载后仍锁着。认证方式确实没了（如删除了 PIN）时由用户点「不经认证解锁」，因为能做到这一步的人本来就能读进程内存。
- **设置在保险库**：加密时改文件关不掉它，所以 `AppLockGate` 在 `VaultGate` 之内。
- **触发**：系统闲置、锁屏、睡眠、切换用户，以及手动。
- **锁定界面**：模态 `<dialog>` 盖住一切，应用保持挂载。React 之外的部分（快捷键、播报、系统通知、原生拖放与对话框）经 `isAppLocked()` 判断。已经打开的原生文件对话框盖不住（已知限制）。

### 阻止睡眠

- **范围**（`keep_awake/`）：只阻止闲置睡眠，熄屏、锁屏照旧（应用锁也照常触发），合盖与手动睡眠三个平台都拦不住。macOS 用 `PreventUserIdleSystemSleep` 断言，Windows 用 `PowerRequestSystemRequired`，Linux 用会话管理器的挂起抑制：先 GNOME 的 `org.gnome.SessionManager`（Cinnamon、MATE、Budgie 也提供），再 KDE、Xfce 的 `org.freedesktop.PowerManagement.Inhibit`；两者都没有的桌面（swayidle 之类的 wlroots 环境）阻止不了。不用桌面 portal 的 Inhibit，它也是转给这些服务，失败却只在之后的信号里说；不用 logind 的 sleep 锁，因为 GNOME 在它释放后要等下一次输入才重新计算闲置。请求与释放都在后端自己的线程里做，调用方（包括主线程）不等系统。
- **算哪些连接**：每个窗口告诉后端它有没有远程窗格连着、正在连接或等待重连（`keepsAwake`，`lib/panes.ts`），后端按窗口记、合起来持有一个请求；窗口关闭或页面重载时它那份清掉。等待重连也算：这时系统早已闲置，两次重试之间一放开就会睡。本地终端不算。
- **仅接通电源时**：后端监听电源（IOKit、`GUID_ACDC_POWER_SOURCE`、UPower 的 `OnBattery`，UPower 重启后重读），UPS 算作没接电源，没有电池或判断不了时算接着。拔掉电源即释放，系统按电池的计时睡眠。
- **Modern Standby**：Windows 在这类机器上用电池时，到睡眠时间 5 分钟后结束请求；后端报告这一点（`keep_awake_limited`），设置选「始终」时提示，不设法绕过。

### 检查更新

- **查什么**（`update.rs`）：GitHub 最新 release 的版本号，只提示不下载。对外说明见 `PRIVACY.md`，改动行为时同步修改。
- **何时查**：设置开启时每天最多一次，结果缓存在 `update.json`；也可在「关于 ZShell」里手动检查，开关也在那里。
- **怎么提示**：有新版本时，会话列表标题栏的设置按钮前多一个带圆点的按钮（列表收起时在展开列表的按钮后），打开「关于 ZShell」，三个平台相同；可以跳过某个版本。只此一处标记，应用菜单与设置里都不标。
- **Microsoft Store 版**：有包身份时不检查，由 Store 更新。

### 调试日志

- **一条通路**（`debug_log/`）：后端经 `log` 门面记录，自己实现 logger，russh 与 russh-sftp 本来就用 `log`。记录进内存里的环形缓冲区，后端与页面各一份（10,000 行 / 2,000 行，各不超过 4 MB，单条超过 4 KB 截断），日志窗口开没开都在记，连接失败后再打开也看得到经过。不写盘。release 版没有控制台，原来的 `eprintln!` 都改成了 `log::warn!`；debug 构建里 warn 以上同时打到 stderr。
- **记什么**：照 OpenSSH `-v` 的粒度记连接的每一步（路由、DNS 与 TCP、代理、版本行与握手、主机密钥、认证的每次尝试、断开原因、keepalive 检查），以及会话、SFTP、转发、ZMODEM、Telnet、串口、本地终端、保险库、应用锁、检查更新与窗口的关键节点。服务器在版本行之前发来的内容由传输层记下（`ssh/abortable.rs`），russh 跳过这些行，什么都不留。
- **不记什么**：密码、口令、keyboard-interactive 的回答、终端的输入输出、私钥，前端失败的命令只记命令名与错误，不记参数。russh 的 trace 一律不要；它的 debug 里每个包、每次窗口调整都有一条，打一个字就一条，会在几分钟内把别的记录挤掉，agent 的签名数据也在里面，这些按模块与消息过滤掉（`wanted`）。其他库（Tauri、系统库）只要 warn 以上。
- **连接编号**：记录带上状态栏的那个连接编号。编号在开始连接时就分配（跳板机也有），不再等到认证之后，所以经跳板机的目标连接编号会跳号。在这条连接上工作的任务（连接、认证、shell）经 task-local 自动带上（`debug_log::about`）；在别处记录的（russh 的回调、SFTP、转发）用 `conn` 键写明。russh 自己 spawn 的连接任务带不上，它的记录按时间先后对照。
- **英语**：记录与 OpenSSH 的 `debug1:` 一样不翻译（见 [I18N.md](I18N.md)），错误用 `Error::logged` 写英文与错误码。
- **发给窗口**：窗口先注册 `debug-log-added`，再取一次快照（`debug_log_snapshot`），此后新记录每 100 ms 合成一个事件只发给它；窗口关闭或页面重载时停发。快照与事件可能交叉，前端按 `seq` 合并去重（`lib/debugLog.ts`）。清空一侧时后端返回清到的 `seq`，前端丢掉不大于它的记录，清空前已在路上的那批也进不来。
- **页面的记录**：每个页面截获自己的 `console.warn` / `console.error`、未捕获的异常与 Promise rejection，`call()` 记下失败的命令，合批后经 `debug_log_write` 送回后端，记录注明来自哪个窗口（`ipc/debugLog.ts`）。`debug_log_write` 自己失败不记，免得循环。
- **日志窗口**（`DebugLogWindow.tsx`）：标签 `debug-log`，全局最多一个，从「帮助」菜单打开（没有快捷键），与主窗口用同一个页面，`main.tsx` 按标签只渲染日志窗口，不经保险库，但在应用锁之后。它不是「应用窗口」：不参与标签的移入、拖放落点与退出前的询问，不是菜单作用的窗口；在 macOS 上它有焦点时，只有关闭、查找（以及「调试日志」本身）作用于它（`menu::DEBUG_LOG_ACTIONS`），其余菜单项先把应用窗口提到前面再作用于它，锁定与退出也一样；最后一个应用窗口关闭时它随之关闭，应用才会退出（`window::app_windows`）。
- **渲染**：记录写进只读的 xterm.js（字体、字号、连字与配色沿用终端设置），它只绘制可见的行，能跨屏选择、随窗口宽度重新折行，在末尾时跟随新记录、往上翻就停下。记录在写入前把控制字符转成可见的转义（`\x1b`、`\r`），服务器在版本行之前发来的东西可能是任何字节。新记录追加在末尾；换一侧、改级别或搜索词、记录乱序到达、清空时整批重写（`toAppend`）。右键菜单（复制选区、全选、复制所有显示的记录、清空）与往上翻时底部的"新输出"按钮沿用终端的组件。

### 渲染与 memo

- **React Compiler**：给每个组件与 hook 自动 memo。它跳过编不了的函数时不报错，所以 `compiler.test.ts` 编译 `src/`，任何函数被跳过都失败；真编不了的写 `"use no memo"` 并说明原因。
- **渲染时不碰 ref**：最新值用 `useLatest`（名字以 `Ref` 结尾）或 `useEffectEvent`，DOM 尺寸存进 state；含 ref 的对象在使用处解构。菜单的项（哪些可用）要读 store 与 ref，在打开菜单的事件里算好存进 state，打开期间不再更新。
- **渲染时不读 React 之外的状态**：外部 store 经 `useSyncExternalStore` 订阅；随语言变化的文字从 `useLocale` 取（`locale.test.ts` 检查）。
- **尚不支持的语法**：`try … finally` 等写法会让整个组件被跳过，经 `lib/attempt` 或改写。
- **热路径**：标签页 store 可能每秒变化多次。不该随之重新渲染的组件，回调经 `useStableHandlers` 组成一个对象传入，开发版的 `useStableProps` 在破坏时警告；频繁更新的列表每项拆成单独的组件（`TabItem`、`FileRow` 等），因为 Compiler 不 memo 循环里创建的元素。

### 终端连字

- **分组与字形**：xterm.js 按单元格绘制，character joiner 只决定哪几格合在一起画，字形由字体自己的规则决定。关闭时给终端加 `font-variant-ligatures: none`，因为 DOM 渲染器会自己连字。
- **三档**（`terminal.ligatures`，默认关闭）：「常用序列」用固定的序列表（`lib/ligatures.ts`），不读字体；「按字体规则」按字体的 GSUB 分组，读不到时退回常用序列。
- **为什么不用 xterm.js 的连字插件**：它经 `queryLocalFonts` 读字体，macOS 与 Linux 的 WebView 没有这个接口，而且只实现了一部分规则，分组常与画出的不符。改为后端 `fonts/` 找到终端实际使用的字体，只读 `cmap`、`GDEF`、`GSUB`，编成紧凑规则交给前端；前端照 HarfBuzz 的方式应用（`lib/fontLigatures.ts`），同一字体的终端共用一份。前后端对格式的理解由一份 fixture 检查。不照 HarfBuzz 做的几处及理由见 `fontLigatures.ts`。

## 交互约定

### 无障碍

目标是读屏用户（VoiceOver、讲述人、NVDA、Orca）能完成连接、操作终端、使用 SFTP 与主要对话框，全部功能可以只用键盘完成；不追求逐条符合 WCAG。

- **终端**：`screenReaderMode` 有性能开销，默认在系统读屏软件运行时打开。WebView 拿不到这一点，由后端查询系统并广播 `screen-reader-changed`（`screen_reader/`）。
- **对话框**：`Modal` 带 `role="dialog"` 与 `aria-modal`，以标题命名；打开时聚焦第一个控件，Tab 在内循环，关闭后焦点还回去。
- **列表与下拉**：焦点停在容器或字段上，用 `aria-activedescendant` 指向当前项。各处菜单都可以用 Shift+F10 或菜单键打开。
- **字段的标题**：对话框里的字段一律用 `Field` 写标题（`fields.test.ts` 检查），外观因此一致。单个控件用 `<label>` 包住命名；一组控件（`RadioGroup`、几个复选框、输入框加它的按钮）用 `<fieldset>` 和 `<legend>` 命名，`RadioGroup` 不再另写 `aria-label`。说明只属于一个字段时写成 `hint`，经 `aria-describedby` 随控件或这组一起读出；属于一排字段的说明仍写在它们后面。
- **字段里的按钮**：`<label>` 包住的按钮，名字会并入字段的名字（读成“Password Show password”）。只给指针用的按钮（`ComboBox` 的箭头、`SpinInput` 的 ▲▼，键盘有对应的键）加 `aria-hidden`；要用键盘操作的按钮放在 label 外，例如 `PasswordInput` 自己画 label，只包住文字和输入框。
- **播报**：`lib/announce.ts` 是唯一的 live region，播报一闪而过或不在视线内的变化。
- **样式**：键盘焦点统一画强调色轮廓，去掉轮廓必须用别的方式显示焦点；尊重减少动态效果与 Windows 对比度主题；文字对比度至少 4.5:1。

### 不用原生控件

各平台 WebView 自己绘制的控件外观不同、不跟随主题，有的还有缺陷，所以凡是会弹出东西或画法明显的控件都由页面自己画，不引入第三方组件（常见组件把读屏文字写死成英文）：

- **图标**：只用 `components/icons.tsx` 里的 SVG，不用文字字形或 emoji，它们在各平台字体里形状和基线都不同（`icons.test.ts` 检查）。
- **下拉框**：`Select`（只能选）与 `ComboBox`（可输入，带候选项），列表都是页面内的 `ListPopup`。`<datalist>` 在 WebView2 里只列出包含框中文字的候选项，框里已是其中一项时就只剩它自己。
- **数字框**：`SpinInput`，不用 `type="number"`。
- **复选框与单选框**：仍是原生的 `<input>`（键盘、`<label>`、读屏都不变），只用 CSS 画成主题色（`base.css`）；Windows 对比度主题下交还系统绘制。
- **密码框**：`PasswordInput`，右端的按钮显示或隐藏输入的内容，三个平台一致；WebView2 自带的显示按钮（`::-ms-reveal`）隐藏。
- **多行文本框**：`AutoTextarea` 随内容增高，没有右下角的拖拽手柄。
- **表单校验**：不用 `required`、`pattern`，它们弹出 WebView 自己的气泡，语言跟随系统而不是应用；空字段由表单自己的检查说明。`controls.test.ts` 检查以上几条。
- **颜色**：`ColorField`。WebView2 的颜色框里吸管无效，后端的 `color_pick` 按平台从屏幕取色（`color_pick/`）。
- **菜单与提示**：右键菜单是 `ContextMenu`（WebView 自带的只在文本框里保留）；WKWebView 不显示 `title`，悬停提示由 `Tooltips` 画，组件照常写 `title`。
- **例外**：滚动条用系统的（跟随深浅色，保留 macOS 的悬浮滚动条），终端的由 xterm.js 自绘。终端的滚动条悬浮在文字上，只在视图滚动（不含停在末尾跟着输出走、或往回看时下方来了输出）、指针移到滚动条上或拖动滑块时显示，所以终端按整个宽度排列（`lib/terminalFit.ts` 的 `FullWidthFitAddon`，fit 插件原本给它留出 14px）；滑块收窄贴在右边，少挡最后一列。

### 快捷键与菜单

- **快捷键**：在捕获阶段拦截，终端收不到。全部在 `lib/keymap.ts` 的一张表里声明；应用菜单、右键菜单的文字和设置里的列表都由它得出。Windows 上与 shell 冲突的加 Shift。对话框打开时，标签页上的动作不生效（焦点会被移到对话框后面的终端），生效的只有对话框自己的快捷键和不涉及标签页的动作：设置、关于、检查更新、锁定、发行说明、报告问题、关闭窗口和退出（`useAppActions` 与 `useCloseFlow` 里 `when: "always"` 的那些）。最上层对话框自己的快捷键先于这些动作，所以设置打开时再按一次它的快捷键会关掉设置；选菜单项不会，但 macOS 上 ⌘, 由菜单栏接收，按键和菜单项是同一回事。WebView2 自带的浏览器快捷键（F5 / Ctrl+R 刷新、Alt+← 后退、Ctrl+P 打印等）在建窗口时关掉（`window/win.rs`）：页面重新加载会关掉窗口里的全部会话；按键照常交给页面。
- **按键匹配**（3.16 起，对齐 macOS、Windows 与 VS Code、kitty 等）：字母与标点按打出的字符（`e.key`），换成 AZERTY、Dvorak、日文键盘也是键帽上的那个键；打出的不是 ASCII 时（西里尔字母、死键、macOS 上 Option 打出的字符）退回美式键盘上的位置（`e.code`），见 `lib/keys.ts` 的 `isKey`。数字、命名键（Tab、方向键）和带 Shift 的标点仍按位置：数字在各布局上都在同一排，带 Shift 的标点打出什么随布局变。不用 `keyCode`：macOS 的 WebKit 在输入法开着时给 229，Linux 的 WebKitGTK 查不到非拉丁字母；Keyboard Map API 只有 WebView2 有。macOS 菜单栏关掉了 AppKit 对快捷键的自动改写（`allowsAutomaticKeyEquivalentLocalization`，`menu.rs`），菜单与页面判断按键的方式一致，页面才能准确地把菜单的按键让给它。
- **应用菜单**：结构在 `keymap.ts` 里声明（`appMenu`）。macOS 由后端构建原生菜单栏（`menu.rs`），WKWebView 的 ⌘C / ⌘V 依赖原生菜单项，这些动作由菜单发 `menu-action` 而不由页面匹配按键。原生菜单只能收到页面没有处理的按键，所以页面放进菜单栏的应用菜单项，其按键在捕获阶段就停止传播（不取消），终端和输入框都看不到：否则 xterm.js 会把 ⌘↩ 当回车、输入栏会把它当发送。Windows 与 Linux 没有菜单栏，标题栏左上角是菜单按钮；macOS 全屏时也显示它。「关于 ZShell」在三个平台上都是页面画的对话框（`AboutDialog`）：macOS 不用 AppKit 的关于面板，它的文字跟随系统语言而非应用的语言，也放不下许可证链接与检查更新。
- **菜单项即快捷键**：`runAction` 用代表该动作的按键事件走同一套处理，所以对话框、应用锁等规则与快捷键完全一致。
- **动作表**：窗口的动作在 `app/useAppActions.ts` 的一张表里，每项写做什么、什么时候可用，快捷键与应用菜单共用：菜单项不可用时按键也什么都不做（但仍被拦下），只有拆分窗格例外，窗格闪一下并读出原因。在别处执行的动作（终端的复制粘贴与查找、会话列表的导出）也在表里，只为菜单的可用状态。
- **会话列表**（Xshell 风格）：单击选中，双击或 Enter 连接（当前窗格就是这个已断开的会话时原处重连），单击文件夹折叠；搜索框输入像地址时提供快速连接。

### 标题栏

- **布局**：没有原生标题栏。macOS 用 Overlay 标题栏；Windows 无边框，窗口按钮自绘；Linux 保留 GTK 的客户端装饰，只把标题栏换成空控件，按钮按桌面的布局自绘（`window/`）。
- **拖动**：带 `data-window-drag` 的元素由 `hooks/titleBar.ts` 处理，双击与右键交给后端，行为与系统一致。
- **收起会话列表**：标题栏的开头（应用菜单按钮、红绿灯旁的开关）属于会话列表；列表收起时，`Sidebar` 经 portal 把这一段画到标签栏开头的空位里，位置与展开时相同，列表的菜单与对话框照常挂着，应用菜单里的导入、导出仍然可用；新建会话先展开列表，新会话放进选中的文件夹，要让人看得见。标签栏空白处是标题栏，Windows 与 Linux 上右键是系统窗口菜单，所以不放自己的右键菜单。收起状态与列表宽度一样是布局状态，按机器存在 localStorage 里（`app/useSidebar.ts`）。
- **Windows 贴靠布局**：最大化按钮上方放一个不绘制的原生子窗口，让系统认出它（`window/win.rs`）。

### 剪贴板与终端输入

- 读写剪贴板用 clipboard-manager 插件，因为菜单里的粘贴触发不了 paste 事件，WKWebView 的 `readText` 又会弹出系统确认。
- 所有粘贴走 `term.paste()`；多行内容在远端没开 bracketed paste 时先确认。
- 远端开启鼠标模式时右键交给程序，Shift+右键总是打开菜单。
- **链接**：网址与 OSC 8 链接 ⌘ / Ctrl+点击用系统浏览器打开，只认 http 与 https。

### 程序的 OSC 请求

xterm.js 处理标题、调色板、OSC 8 与颜色查询；其余由 `lib/terminalOsc.ts` 注册（去处由 `TerminalView` 给出），解析在 `lib/osc.ts`，各项的协议细节见那里。都只作用于发出序列的窗格。

- **OSC 7**：文件夹，见「文件传输与端口转发」。
- **OSC 52**：只写剪贴板，从不回应读取；可关闭，有大小上限。
- **OSC 9;4**：进度显示在标签的进度环和任务栏按钮上（macOS 的 Dock 与 Linux 的启动器由后端合并各窗口，`window/progress.rs`）；会话结束或一段时间没有更新时清除，免得被打断的程序留下进度。
- **OSC 9、777、99**：通知。可关闭，每个窗格限速；标签在后台时加提醒标记，窗口没有焦点时发系统通知；应用锁定时只说有通知，不带内容。
- **OSC 22**：鼠标指针形状，照 kitty 主屏与备用屏各一个栈。

### 标签页

- **分屏**：一个标签是一棵窗格树，每个窗格是独立的会话；切分、关闭、调整大小都不会重新挂载其他窗格的终端（`lib/layout.ts`）。布局不保存。
- **终端挂在哪里**：窗格的终端与面板由 `App` 下的 `PaneHostList` 经 portal 画进每个窗格的宿主元素（`components/PaneHost.tsx`）。React 换了父节点的组件一定重新挂载，这样窗格换标签只是宿主换位置，终端与进行中的传输都不受影响。
- **移动窗格**：经右键菜单或拖动窗格顶部的抓手，移到新标签、其他标签的某一侧或其他窗口；拖着停在标签上片刻会切过去（`lib/tabRest.ts`）。抓手只给指针，键盘与读屏用户用右键菜单。
- **当前窗格**：标签的标题、状态、菜单与面板跟随有焦点的窗格。标题：重命名 > 远端标题 > 会话名。
- **提醒与响铃**（可关闭）：后台标签有输出时加圈，刚输入或改变大小后的回显与重绘不算；响铃显示铃铛并播报，紧跟在输入之后的（补全失败）不提醒。提示音由后端播放系统声音（`app_bell`）。
- **拖拽排序**：像浏览器那样滑动让位（`lib/tabStrip.ts`），拖动中只改 transform，放下时才改 store。用鼠标事件而不是 HTML5 拖放，因为 Windows 上 Tauri 的文件拖入会拦截后者。
- **关闭确认**（可关闭）：已连接的远端窗格、有前台程序的本地终端、文件面板有工作时先确认，并说明会失去什么（`paneWork`，`lib/panes.ts`）。
- **撰写栏与同步输入**：经各窗格的 `PaneSession.send()`，只发给 shell 已就绪的窗格，不会把口令发进别的认证提示。范围只在撰写栏打开时生效，免得停在"所有标签"而不自知（`lib/compose.ts`）。
- **快速命令**：按分组整体保存，会话可以指定默认分组，与撰写栏共用发送逻辑。

### 多窗口

- **窗口从标签产生**：没有空白的新窗口，移动或拖出标签、"在新窗口中连接"才会打开。窗口要显示的内容由后端暂存，页面取走（`window_take`），这样加载中的页面也拿得到，关空窗口与移入标签也能互斥。
- **只发给本窗口的事件**：`listen()` 会收到发给其他窗口的事件，所以一律用 `listenHere` / `useWindowEvent`。
- **跨窗口同步**：配置在一个窗口保存后，后端发 `config-changed`，其他窗口重读；应用锁的状态本来就在后端。
- **移动标签**（`app/useWindows.ts`）：标签连同布局与窗格状态作为数据交给目标窗口。会话由后端转交（`SessionManager::offer` / `session_adopt`），旧 Channel 最后发 `moved`；屏幕经序列化转交，与之后的输出不丢不重：旧窗口交给后端暂存（`session_handover`），新窗口接管后自己来取（`session_snapshot`，最多等 5 秒），不走事件，因为事件的 payload 会写进页面执行的脚本，长 scrollback 时有几 MB。接管失败或目标窗口中途消失时，窗格留在来源窗口。状态机在 `lib/tabMoves.ts`，有单元测试。
- **不能移动的标签**：文件面板有工作、ZMODEM 进行中、正在打开或移入的会话，这些状态属于旧窗口的页面。
- **拖出标签**（`tab_drag/`）：松手落在另一个窗口的标签栏上就并入，其他地方开新窗口。落点窗口由系统判断，光标用后端读的屏幕坐标，因为网页的 `screenX` 在多显示器下不可靠。
- **关闭与退出**：最后一个标签走后，有其他窗口时这个窗口随之关闭。⌘Q 汇总各窗口需要确认的标签后确认一次，没回答的窗口宁可多问。
- **调试日志窗口**不在上面这些规则之内，见「调试日志」。

### 状态栏与收发指示

- 变化少的数据在 tab store；收发计数、光标这类频繁变化的不进 store，状态栏与面板直接订阅并按帧更新（`lib/traffic.ts`）。不是 live region，以免光标一动、速度每秒一变就播报。
- **收发由后端按通道统计**（`traffic.rs`）：终端按会话（会话编码转换前的字节，含 ZMODEM，不含连接前提示里输入的内容和我们自己的消息），SFTP 按连接分浏览与传输两个通道（服务器不给传输单开通道时两者共用一个），转发按规则的每次运行（经 `Hub` 报给会话的所有标签，无论规则在哪条连接上）。SSH 自身的开销（keepalive、窗口调整、加密与包头）不属于任何通道，不计入其中；传输层的总量在 `ssh/abortable.rs` 另计，暂不展示。
- **上报节流**：计数变化后立即报一次，之后最多每 50 ms 一次，短于前端灯亮的 150 ms，所以持续有流量时灯常亮。事件与终端输出走同一个会话 Channel。速度由前端取最近 1 秒的计数算出，每 0.5 秒刷新一次。
- **连接与通道编号**：连接编号在本次运行内递增，开始连接时分配（跳板机的连接也占一个，见「调试日志」），附带这条连接在本机的 TCP 端口（经跳板机时是到第一跳的，经代理命令时没有），可与 `netstat` / `lsof` 或服务器日志对照；通道号是本端分配的编号，russh 不提供对端的。

## 平台注意事项

- macOS 上 CJK 输入法要在真机上测；处理 Enter / Esc 前先用 `isComposing` 判断是否在组字。
- macOS 关掉了"按住按键显示重音字符"（与 Terminal.app 一致），用户可用 `defaults write` 恢复（`lib.rs`）。
- Windows 的 S 模式不允许启动命令行程序，这时隐藏本地终端的入口。
- ConPTY 需要 Windows 10 1809 及以上。Windows PowerShell 5.1 调用的原生程序可能按 OEM 代码页输出导致中文乱码，暂不修改用户的 `[Console]::OutputEncoding`（已知限制）。
- ssh-agent：macOS 与 Linux 用 `SSH_AUTH_SOCK`；Windows 用 OpenSSH 命名管道或 Pageant。
- Linux 第一阶段只针对 GNOME + Wayland：闲置检测、窗口按钮和窗口菜单按 GNOME 做。Wayland 不让应用知道窗口位置和全局指针，所以标签拖出窗口后改用 GTK 拖放（`tab_drag/linux.rs`）。
- Linux 上终端的默认字体先用桌面设置的等宽字体，再用内置列表。
- Linux 上串口设备通常属于 `dialout`（Fedora 是 `dialout` 或 `uucp`）组，用户要加入该组才能打开。

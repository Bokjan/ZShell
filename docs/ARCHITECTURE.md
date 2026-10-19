# ZShell 架构

个人使用的跨平台（macOS / Windows / Linux）SSH / Telnet / 串口终端。本文只记录跨模块的设计决策和它们的理由；模块内的细节（边界情况、库的坑、具体数值）写在代码注释里，这里指向对应的文件。

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
| SSH / SFTP | `russh`（`ring` 加密后端，避免 Windows 上依赖 CMake/NASM）/ `russh-sftp` |
| 凭据 | `keyring`（macOS Keychain / Windows Credential Manager / Linux Secret Service） |
| 加密 | `ring`（AES-256-GCM、PBKDF2-HMAC-SHA256、随机数），`zeroize` |
| ssh_config 导入 | `ssh2-config` |
| 本地终端 | `portable-pty`（Windows 走 ConPTY） |
| 串口 | `serialport` |
| Linux 桌面 | `gtk`（标题栏、拖出文件）、`zbus`（设置 portal、polkit、logind、Mutter） |
| Telnet | 自己实现（协议与选项协商） |
| 前端 | Vite + React + TypeScript |
| 终端渲染 | `@xterm/xterm` + fit / webgl / unicode11 / web-links / search 插件 |
| 终端连字 | 自己实现的 `cmap` / `GDEF` / `GSUB` 读取（后端 `fonts/`）与替换（`lib/fontLigatures.ts`）；字体的表经 DirectWrite / CoreText（`objc2-core-text`）/ `fontconfig` 读取 |

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
| `ssh/` | 连接与 shell（`mod.rs`）、认证好的连接及其上各窗格 shell 的顺序（`connection.rs`）、russh 回调（`handler.rs`）、服务器为远程转发开的通道交给谁（`forwarded.rs`）、主机密钥确认（`host_key.rs`）、known_hosts 与主机证书（`known_hosts/`）、认证（`auth/`）。不认识连接上跑的其他功能 |
| `connections/` | 连接注册表：会话 id → 连接及其上的 SFTP 与转发规则；同一会话的转发规则在哪条连接上运行、连接关闭时如何移交。ssh、sftp、forward 在这里组合 |
| `sftp/` | 浏览与文件操作（`mod.rs`）、一条连接上的 SFTP 通道（`channels.rs`）、传输（`transfer/`）、本地编辑（`edit.rs`）、拖出（`drag/`）、替换写入（`replace.rs`）、文件名转码（`names.rs`） |
| `forward/` | 转发规则的运行（`mod.rs`）、`-L` / `-R` / `-D`（`local.rs` / `remote.rs` / `dynamic.rs`）、SOCKS（`socks.rs`） |
| `pty/` | 本地终端（`mod.rs`）、Unix 与 Windows 各自的读写与结束（`unix.rs`、`windows.rs`）、默认 shell 与环境变量（`shell.rs`） |
| `telnet/` | Telnet 会话与自动登录（`mod.rs`）、协议与选项协商（`protocol.rs`） |
| `serial/` | 串口会话与设备列表（`mod.rs`） |
| `net.rs` / `proxy/` | 出站连接（直连或经代理）、`Route`；代理：SOCKS5、HTTP CONNECT、ProxyCommand |
| `zmodem/` | rz / sz：检测与接管、帧格式、收发 |
| `logging/` | 会话日志：写入线程（`mod.rs`）、纯文本格式（`text.rs`）、索引 `logs.json` 与清理（`index.rs`） |
| `model/` | 保存下来的数据类型及其保存、导入时的校验：会话、文件夹、代理、转发规则、快速命令、应用锁设置、应用设置。只依赖 serde、ts-rs 与基础工具（错误、编码），存储、导入导出和运行它们的功能模块依赖它，反之不然 |
| `config/` / `quick.rs` / `settings.rs` | 会话、文件夹与代理的存储；快速命令的存储；应用设置 `settings.json` 的存储 |
| `vault/` / `envelope.rs` / `crypto.rs` / `secrets.rs` | 保险库（上面几类数据与密码存在哪里、如何保护）；保险库与导出文件共用的信封格式；加密与口令派生；系统凭据存储 |
| `applock/` | 应用锁：锁定状态、各平台的系统认证、闲置与锁屏检测 |
| `persist.rs` | JSON 文件的读写：读不出的文件、原子写入、手写文件的 id |
| `encoding.rs` / `local_name.rs` | 会话的字符编码；对方给出的文件名转成本地文件名 |
| `import/` / `backup.rs` | ssh_config 导入；会话导出与导入 |
| `window/` | 窗口：按平台创建，新窗口要显示的内容与退出前询问各窗口（`inbox.rs`，不依赖窗口、可单独测试）；各平台文件（`macos.rs`、`win.rs`、`linux.rs`）提供同一组函数，`mod.rs` 经 `platform` 调用，缺了哪个都编译不过：macOS 的红绿灯，Windows 的窗口按钮与贴靠边框，Linux 的空标题栏、窗口菜单与主题 |
| `desktop.rs` | Linux 桌面的设置（配色方案、等宽字体），经设置 portal 读取 |
| `hidden_window.rs` | Windows：接收只发给顶层窗口的系统广播（设置变化、挂起、会话锁定）的隐藏窗口，应用锁与读屏检测共用 |
| `fonts/` | 终端字体的连字规则：按字体名找到已安装的字体，读出 `cmap`、`GDEF` 与 `GSUB`（`cmap.rs`、`gsub.rs`），编成前端应用的紧凑规则（`rules.rs`，见「终端连字」） |
| `screen_reader/` | 系统的读屏软件是否在运行，变化时通知各窗口（见「无障碍」） |
| `tab_drag/` | 把标签拖出窗口：落点在哪个窗口、哪里（见「多窗口」） |
| `menu.rs` | macOS 的原生菜单栏，由页面给出的结构构建（见「快捷键、菜单与侧栏」） |
| `instance.rs` | macOS 与 Linux 的单实例（见「配置与设置」） |
| `update.rs` | 检查更新（见「检查更新」） |
| `i18n.rs` / `error.rs` | 后端消息目录、结构化错误（见 [I18N.md](I18N.md)） |
| `events.rs` | 后端发给页面的事件：名字与 payload 类型在一处声明 |
| `launch.rs` | 新会话运行什么：把前端的 `SessionSpec`（保存的会话、快速连接、复制标签、本地终端）解析成后端（`Backend`）、日志的命名与字符编码；`session/` 不认识具体的后端 |
| `commands/` | Tauri 命令入口，按领域分文件：前端能调用的全部命令都在这里（也只在这里），只做检查调用方、交给所属模块、通知其他窗口；逻辑在各模块里 |
| `bindings.rs` | 生成前端的 TypeScript 类型和共用常量（`src/ipc/bindings.ts`） |

前端（`src/`）：

- **分层**：组件之外分三层。`lib/` 是不用 React 的逻辑，调用后端只经 `ipc/`；`ipc/` 是唯一调用后端命令、监听后端事件的地方，类型来自生成的 `ipc/bindings.ts`；`hooks/` 是组件共用的 React store、provider 与 hook（只属于 `App` 的在 `app/`）。`layers.test.ts` 检查 `lib/` 不引 React 与 Tauri，`lib/` 与 `ipc/` 都不引 `hooks/`、`components/`、`app/`（否则会间接带进 React）。Tauri 自带的窗口与插件 API（有它们自己的类型）不受此限。`lib/` 与 `ipc/` 里不用 React、DOM 和后端命令的导出函数有相邻的 vitest 单元测试（`*.test.ts`）；组件不写单元测试，端到端检查。
- **外部 store**：组件之间共享、放在 React 之外的状态是每个窗口一份的 store，由 `lib/store.ts` 的 `createStore` 建立：reducer 加 `dispatch(action)`（标签页、会话目录），或 `set(patch)`（检查更新，`lib/updates.ts`）；值没变时不通知订阅者。组件经 `useSyncExternalStore` 订阅（`hooks/stores.ts` 等），模块与事件处理函数直接 `get()`，都不经 `App` 转交。整体保存到后端的值（设置 `lib/settings.ts`、快速命令 `lib/quickCommands.ts`）由 `lib/savedStore.ts` 建立，修改经 `latestSave`（见「会话与数据流」），store 里另有上次保存失败的原因。谁负责读入就由谁重读：设置由 `SettingsProvider` 在页面启动时读入（它的 Context 只多给出实际的主题、是否开读屏支持这类派生值），会话目录与快速命令由 `app/useDirectorySync.ts`、`app/useQuickCommandBar.ts` 在 `App` 挂载（即保险库解锁）时读入、卸载时清空；收到其他窗口的 `config-changed` 时重读。
- **标签页与窗格**：每个修改是一个 action，由纯函数 `reduceTabs`（`lib/tabs.ts`）处理，修改后立即生效，不等渲染。`App.tsx` 订阅并分发；关闭流程、撰写栏、快速命令栏与面板、窗口间移动、全屏、终端菜单里属于 `App` 的部分、启动时读取的信息各是 `app/` 下的一个 hook。`TabPage` 只在自己的标签变化时重绘。
- **会话目录**：已保存的会话、文件夹、代理和最近打开的会话（`lib/directory.ts`），用到它们的组件（侧栏、标签页、会话与代理的对话框）直接订阅。本窗口的修改经 `directory.changed(写入)` 发出，完成后自己重读，因为 `config-changed` 不发回发起修改的窗口。最近打开的会话存在 `localStorage`，各窗口经 `storage` 事件同步。
- **窗格的会话**：`lib/sessionRegistry.ts` 为每个窗格建一个 `PaneSession`（`lib/paneSession.ts`），负责打开会话、重连、共享连接、登录后命令和流控，只经一个小接口读写终端；`TerminalView` 只管终端本身。注册表由 `App` 最先建立，只读标签页、会话目录与设置的 store；要关闭窗格（`close`）、会话移到其他窗口（`movedAway`）或没能移来（`stayed`）时发出事件，拥有这些流程的 `useCloseFlow` 与 `useWindows` 经 `sessions.on` 订阅。依赖只从拥有者指向注册表，不在渲染后把之后才建立的回调交给它。
- **会话结束**：和 Xshell 一样，用户退出 shell（`exit`、Ctrl+D，即 `exited`）后窗格自动关闭，不看退出状态（bash 以上一条命令的状态退出）；本次连接里用户没在 shell 中输入过（服务器一连上就结束，信息要留着看）、shell 被信号结束（本地进程，或 SSH 服务器发来 exit-signal）、或文件面板有工作时不关（`closesOnExit`，`lib/panes.ts`）。不自动关闭的窗格里，Enter 重连，Ctrl+D 按关闭标签的流程关闭窗格。
- **解锁**：`main.tsx` 里 `SettingsProvider` 之下是 `VaultGate`，保险库锁定时只显示 `UnlockScreen`，解锁后才挂载 `AppLockGate` 与其中的 `App`（见「加密保险库」「应用锁」）。
- **对话框与快捷键**：对话框经 `useDialog`（`hooks/dialogs.ts`）登记到一个栈，只有最上层的响应 Esc 与点击背景；快捷键经 `useShortcuts`（`hooks/shortcuts.ts`）在捕获阶段统一分发，注册时说明何时生效。
- **样式**：`styles.css` 只按顺序引入 `styles/` 下按区域分的各部分（主题变量在 `theme.css`）。部分的先后就是层叠的先后：后写的规则覆盖先写的同等特异性的规则，所以新规则写进它所属的部分，而部分不按字母排序，也不由各组件自己引入（那样 CSS 的顺序由导入图决定）。
- **渲染**：组件与 hook 由 React Compiler 自动 memo，写法有约束，见下文「渲染与 memo」。

## 设计决策

### 会话与数据流

- **前后端类型**：与前端交换的 Rust 类型派生 ts-rs 的 `TS`，生成一个提交到仓库的 `src/ipc/bindings.ts`，`cargo test` 在它过时时失败，只改了一边的类型在 CI 里就会发现。错误码是从英语语言包生成的联合类型 `ErrorCode`，前端按错误码分支由 `tsc` 检查。两边都要用的常量（可选的编码、新会话的默认终端类型和串口设置、默认快速命令分组的 id、默认设置、字号与回滚行数的范围、终端输出确认的批量）也从后端的定义写进这个文件，`api.ts` 转出，前端不再另写一份；确认的批量由后端恢复读取的阈值得出，两者的关系不靠注释维持。命令与事件同样不手写两份：`bindings.rs` 用 `syn` 读 `commands/` 里每个命令的签名，生成 `Commands`（每个命令的参数名与类型、返回类型，Tauri 注入的参数除外），并核对与 `lib.rs` 登记的命令一致；前端只经 `ipc/call.ts` 的 `call` 调用命令（`invoke` 只在那里），`tsc` 按命令名检查参数和结果。命令一律返回 `crate::error::Result`，前端总是拿到带错误码的 `CommandError`；接收会话 id 的命令必须带调用方的 webview（测试检查），以便按归属检查。后端发的事件在 `events.rs` 里各是一个带 payload 类型的类型，只经它发出，生成的 `Events` 让前端的 `listenHere` / `useWindowEvent` 按事件名得到 payload 的类型。选 ts-rs 而不是 specta 的理由见 `bindings.rs`。
- **打开会话**：只有一个命令 `session_open`，参数是 `SessionSpec`（保存的会话、快速连接、复制标签、本地终端），由 `launch.rs` 解析成后端（`Backend`，所需的存储显式传入，可以单独测试），日志与会话任务只在一处建立。
- **会话是一个后台任务**：`SessionManager` 为每个会话创建 `TermIo`（输出 / 事件 Channel、输入队列），后端任务持有它运行；新的后端只需实现同样的任务形态。
- **会话的关闭**：只经 `SessionManager`：关闭标签（`remove`）与页面不在（`remove_webview`，页面重载、窗口销毁、退出应用）走同一个关闭过程。任务之外的资源（连接登记、等待回答的 ZMODEM、流控）登记在会话的 `Lifetime` 上，无论是关闭、会话自己结束还是任务被丢弃，都只释放一次，与调用顺序无关（`session/`）。
- **会话属于 webview**：页面重新加载或窗口销毁时关闭该 webview 的全部会话，否则它们无人显示、也无人关闭；打开中途页面重载的会话被拒绝（`session.pageGone`）。按会话 id 操作的命令（输入、尺寸、关闭、日志、ZMODEM、SFTP、转发、复制标签用的共享连接）只接受归属窗口的调用，其他窗口得到 `session.notFound`，与会话不存在无从区分：id 从 1 递增，知道 id 不代表有权使用。会话可以移到另一个窗口（见「多窗口」），归属随之改变。文件面板的传输与用本地编辑器编辑的文件同样属于发起它们的 webview：按页面给的 id 登记，只有这个页面能取消、上传或停止，页面不在时随会话一起取消、停止监视（`lib.rs` 的 `page_gone`）。
- **终端输出不走 event**：经 Tauri `Channel` 推送原始字节，前端直接 `term.write(Uint8Array)`；会话事件走同一个 channel，保证"连接已关闭"总在最后一段输出之后。输入走 `invoke`。Channel 的每条消息都要在主线程上 eval（1 KiB 以上还要再取一次），所以各后端读到输出后先把已经就绪的部分一起读出（本地终端与串口在 Unix 上一直读到没有为止，串口在 Windows 上查询可读字节，SSH 与 Telnet 取已到的消息；Windows 上 ConPTY 的管道一次读取通常已取到管道里的全部，不另查询），合成一条再发：持续输出时消息数降到几十分之一，单次回显仍立即发出。
- **背压**：每条数据通路都有上限。终端输出由前端按已处理字节确认，后端未确认过多时暂停读取（`Flow`），因为 Tauri 的 IPC 队列没有上限、xterm.js 积压过多会丢弃写入；SSH 写入等服务器窗口时仍持续读取，否则 russh 的有界队列会死锁；ZMODEM 与日志同样有界。反方向上，各后端在对方不读（服务器窗口满、设备流控、shell 不读）时把输入留在自己这里，经 `TermIo::recv` 的同一条规则读取：积压超过上限后 ZMODEM 上传的数据留在它自己的有界队列里，按键（可以取消传输）与尺寸变化照常读取，所以哪个后端都不会因为对方停住而看不到 Ctrl+C。终端输入本身来自用户（键入、粘贴），不设上限；服务器为远程转发开的通道排队有上限，超出的被拒绝。代价是暂停读 SSH shell 会让同一连接上的 SFTP 和转发一起等，但比内存无限增长好。具体阈值见 `session/`。
- **字符编码**：应用内部一律 UTF-8，只在与远端交界处按会话的 `encoding` 转换：输出（`SessionSink`，流式解码，日志记的是转码后的文本）、输入（`TermIo::recv`；二进制的鼠标报告原样发出）、SFTP 文件名（`sftp/names.rs`，有不可逆名字的已知限制）、ZMODEM 的文件名。快速连接、本地终端与认证提示总是 UTF-8。
- **会话事件**：`Connected`、`Forward`、`Zmodem`、`Closed { reason, status, error }`；`reason` 区分 `exited`、`lost`、`failed`，前端据此决定重连、自动关闭或提示。
- **命令在哪个线程上运行**：Tauri 的同步命令跑在主线程，等待会卡住窗口，所以碰文件、凭据存储或设备列表的命令是 async 的，阻塞部分放进 `spawn_blocking`（`commands/mod.rs`）；会话任务读保存的密码只读保险库发布的快照，不等写入。整体替换的写入（设置、快速命令）也是 async 的，顺序由前端的 `lib/latestSave.ts` 保证：同一时间只有一个请求在后端，排在后面的保存取代还没发出的，按住方向键调数字时不会每步都写盘。它保存的是"修改"而不是整份值：存下之前另一个窗口存了新值，本窗口还没存的修改重做在新值上，两个窗口改的不同项都留下。转发规则逐条保存与删除（`profile_forward_save` / `profile_forward_delete`），一个窗口改一条规则不会把其他规则写回它看到的旧样子。
- **内容安全策略**：`tauri.conf.json` 设了严格的 CSP 作为纵深防御（IPC 能执行命令、写任意文件），资源只来自应用自身。样式需要 `unsafe-inline`（xterm.js 插入 `<style>`，`index.html` 有首帧的内联样式），为此关掉 Tauri 给 `style-src` 加的 nonce，有 nonce 时浏览器会忽略 `unsafe-inline`。

### SSH

- **一个连接多路复用**：认证后 `ssh::run` 把连接交给 `connections::Connections` 登记（`ssh` 只管连接本身），SFTP 与端口转发开在同一连接上；批量传输另开一个 SFTP channel，免得列目录排在大文件的读请求后面。
- **复制标签**：已连接的 SSH 标签复制或分屏时，在源连接上开新的 shell channel，不重新连接认证（类似 ControlMaster）。多个会话可以指向同一连接，最后一个结束时才断开；源连接已断开时照常新建连接。
- **提示都在终端里**：主机指纹、密码、口令、keyboard-interactive 都经 `TermIo::read_line` 在终端里问，与 OpenSSH 一致，不弹窗。
- **认证**："自动"按 OpenSSH 的顺序尝试 agent、私钥、keyboard-interactive、密码；加密的私钥只在服务器接受其公钥后才问口令。多因素认证按服务器返回的剩余方法继续；保存的密码只回答一次，而且只回答问本账户密码的提示（`Password for root:` 在用户是 alice 时不算），验证码类的提示总在终端里问，以免把密码当验证码提交、白白计入失败次数（`ssh/auth/`）。
- **主机校验**：自己解析 `~/.ssh/known_hosts`（`ssh/known_hosts/`），按 OpenSSH 的规则校验与写入，并只读地参考 `known_hosts2` 与系统文件，不用 russh 自带的校验。指纹变化时告警并拒绝连接；只记录了其他类型的密钥时先列出再问，与 OpenSSH 措辞一致。
- **主机证书**：known_hosts 里有对这台主机生效的 `@cert-authority` 行时，才向服务器请求证书形式的主机密钥，没有时行为不变。证书按 OpenSSH 的 `check_host_key` 检查（`ssh/known_hosts/certificate.rs`）：受信任的 CA 签发且有效时直接接受；被吊销或签名无效时拒绝；其他情况警告后把证书里的公钥当普通主机密钥校验。
- **agent 转发**：只转发给目标主机，只在本连接请求过转发时接受服务器开的 agent 通道。
- **ProxyJump**：在上一跳连接上开 `direct-tcpip` channel 作为下一跳的传输层，每一跳用被引用会话自己的地址和认证，主机密钥按各跳自己的地址校验。建跳板的部分（`ssh::tunnel`）SSH 与 Telnet 共用。
- **keepalive 与重连**：连续 3 次 keepalive 无响应判定断线（`lost`），前端按退避间隔一直重连到标签关闭，认证或主机密钥被拒绝时停止。russh 认证完成后才发 SSH keepalive，之前靠同样间隔的 TCP keepalive（直连和 SOCKS / HTTP 代理）。认证途中连接断开算断线（`net.connectionLost`），会继续重连；服务器主动断开（例如 sshd 超过 `MaxAuthTries`）算认证失败（`auth.disconnected`），和 OpenSSH 一样停下。重连是新会话；会话结束时后端先重置终端模式，免得下一个 shell 把鼠标报告当成输入。连接各阶段有超时，认证阶段没有（服务器有自己的限制），见 `net.rs` 与 `ssh/mod.rs`；握手超时或标签关闭时连接会被结束，不会留在 russh 的连接任务里（`ssh/abortable.rs`）。
- **测试**：连接上跑的功能（shell、端口转发、连接关闭时的移交）在测试里连到进程内的 russh 服务器（`ssh/testing.rs`），客户端就是 ZShell 自己的 `ClientHandler`，只在测试构建里可以不读 known_hosts 而接受任何主机密钥，测试从不碰用户的 `~/.ssh`。

### Telnet 与串口

- **会话配置**（2.0 起）：会话是共同字段加按 `protocol` 区分的 `connection`（`ssh`、`telnet`、`serial`），改协议后只留新协议的字段。各后端只接收自己协议的配置。2.0 之前的扁平格式不读取。
- **协议**：快速连接支持 SSH 与 Telnet；文件与转发面板、复制标签只用于 SSH，串口设备只能打开一次。
- **Telnet**：协议层是纯状态机（`telnet/protocol.rs`），经 `net::connect` 或跳板机通道连接。服务器关闭连接算 `exited`，读写出错算 `lost`。保存的用户名与密码由后端在登录提示处各填一次，密码不经过前端。服务器不负责回显时由 ZShell 本地回显，但服务器输出停在 `password:` 时这一行不回显，手输的密码不出现在屏幕上，也就不进日志；只看服务器的输出，用户自己敲的 `password:` 不算。
- **串口**：Unix 上由会话任务经 `AsyncFd` 读写，不占线程；tokio 监视不了的设备（驱动不支持 kqueue / epoll）和 Windows 上，阻塞句柄各用一个线程读写，读取定时醒来看会话是否已关闭（Windows 上读写轮流占用句柄，所以那里每 10 ms 醒一次）。拔出设备算 `lost`，前端按断线重连的节奏重试，插回后自动接上。

### 代理

- **配置**：代理是独立保存、有名称的配置（SOCKS5、HTTP CONNECT、命令），会话按 id 引用；被引用的代理不能删除，而不是悄悄改成直连。
- **只用于第一个连接**：代理取第一台跳板机自己的设置，没有跳板机时取会话的，与 OpenSSH 一致（`ProfileStore::route` → `net::Route`）。快速连接不经代理。
- **接入点**：`net::connect` 是 SSH 第一跳与 Telnet 的唯一入口，有代理时交给 `proxy::connect`，返回任意字节流。
- **安全**：主机名交给代理解析。ProxyCommand 代入的主机名与用户名不能含 shell 元字符等（同 OpenSSH 对 CVE-2023-51385 的处理），导入别人的会话文件不应能借此执行命令（`proxy/command.rs`）。代理需要密码时同样在终端里询问。

### 文件传输与端口转发

- **SFTP 传输**：先扫描生成计划再复制，进度经 Channel 推送（每秒至多十次）；关闭窗格时取消其文件面板里的传输。小文件每个都要等几次往返，所以最多 8 个同时复制，大于 256 KiB 的单独复制，在途的请求不比逐个复制时多（请求在服务器排队过久会超时）；一个失败后不再开始新的，已在复制的照常完成。目录逐层并发列出和创建，删除目录同样逐层并发。覆盖已有文件时先写临时文件，同步到服务器的磁盘后再替换，取消、出错或断线时原文件不变（`sftp/replace.rs`）；新文件不做同步，与 `sftp put` 一致。
- **本地文件名**：下载、拖出、本地编辑与 ZMODEM 接收都经 `local_name`，保证结果是目标文件夹里的一个文件名，自动加序号的名字在选定时就占住（`local_name.rs`）。服务器列出的名字先过滤：`.`、`..` 和含 `/` 或 NUL 的条目不列出、不递归（`sftp::is_entry`），否则 `x/..` 拼成的路径会指向别处；拖出时得不到本地文件名的项直接拒绝，不交给接收方处理。
- **SFTP 面板**：选择、排序、过滤在前端完成。条目达到 500 个时只画视野内的行和光标、改名所在的行，其余留出高度（`useRowWindow`）。不选位置的下载进设置里的下载文件夹。路径栏输入时列出已输入目录中匹配的文件夹（含指向文件夹的链接），Tab 按 shell 的方式补全；每个目录在一次编辑中只列一次。
- **终端所在的文件夹**：文件面板打开时进入窗格的 shell 所在的文件夹，工具栏可随时转到那里，开启「跟随」后 shell 每换一个文件夹面板也跟过去。后端经已有的 SFTP 会话读 Linux 的 `/proc` 找到它，不在服务器上运行任何东西、也不改 shell 的环境（`sftp/shell_dir.rs`）：连接的各个 channel 都是连接进程的子进程，有终端的就是各窗格的 shell，按开启顺序与窗格对应（`Connection` 记录各窗格开 shell 的顺序）。没有 `/proc` 的系统靠 shell 自己报告的 OSC 7。前端在终端里输入命令（回车）后输出停顿 300 ms 时，以及收到 OSC 7 时，通知同一窗格的面板重新查找（`lib/shellDirs.ts`）；只在 shell 的文件夹确实变了时才跳转，用户在面板里浏览别处不会被拉回。
- **临时目录**：交给其他应用的下载（用本地编辑器编辑、拖出）放在系统的临时目录，macOS 与 Windows 上它属于用户自己；Linux 的 `/tmp` 由所有用户共享，所以放在应用的缓存目录（`~/.cache/org.boyin.zshell/`，0700，`scratch_dir`）。
- **用本地编辑器编辑**：后端下载到临时目录，监视它所在的文件夹（所有编辑共用一个文件系统事件监视器，系统不支持时退回轮询），改动停下 300 ms 才算保存完；上传由前端经标签当前的连接发起，所以重连后继续有效。远端在下载后被改过时先询问再覆盖（`sftp/edit.rs`）。没有设置编辑器时用系统的默认应用打开，但系统会运行而不是打开的类型（`.command`、`.bat`、`.lnk`、`.desktop` 等）改用文本编辑器（macOS 的 `open -t`、Windows 的记事本），Linux 上要求先设置编辑器：文件名和内容都由服务器决定，下载的文件也没有隔离标记可供 Gatekeeper / SmartScreen 提示。
- **拖出窗口**：只有放到其他应用上才下载，macOS 用文件承诺、Windows 用 OLE 拖放、Linux 用 GTK 拖放（`text/uri-list`），对方真正取数据时才下载（`sftp/drag/`；Linux 上 GTK 4 的应用在经过时就取，见 `sftp/drag/linux.rs`）。
- **端口转发**：规则存在会话配置里。规则的每次运行是一个持有全部资源（监听、远程转发在服务器上的监听、承载的连接）的任务，停止即等资源释放，重启的规则等上一次运行结束再绑定，端口立即可重用；被替换或停止的运行不再上报状态（`forward/mod.rs`）。规则属于会话，同一会话的多个标签各有连接，所以一条规则只在其中一条连接上运行，从任一标签操作都落到那条连接上，状态报告给会话的所有标签。关闭它所在的最后一个标签时，用户可以选择移到同会话的其他连接继续：先等原来的运行放开端口，再在那里启动（`connections/`）。

### ZMODEM

- **位置**：在会话层，SSH、Telnet、串口与本地终端（包括在本地终端里 ssh 登录后）都适用。协议自己实现，以 lrzsz 为对照测试。
- **检测与接管**：`SessionSink::output` 查找 `sz` / `rz` 的开头；识别后输出交给传输任务，按键被丢弃，Ctrl+C 取消。误判（如 `cat` 了一个二进制文件）时输出原样交还终端。传输数据计入会话的流控。
- **与前端交互**：经 `Zmodem` 会话事件询问保存位置或要发送的文件。接收默认先在终端上方提示，因为 `cat` 一个恰好像 `sz` 输出的文件也会触发，不应在用户不知情时存下文件。进度与结果由后端写在终端里。
- **兼容性**：转义全部控制字符（相当于 `rz -e`），以通过过滤控制字符的堡垒机。细节见 `zmodem/`。

### 本地终端

- 作为 `SessionManager` 的另一种后端，输入、resize、关闭沿用 SSH 会话的命令。
- **前台进程**：`session_foreground` 报告 shell 之外在前台运行的程序，用于关闭前确认。
- **默认 shell 与结束**：macOS 与 Linux 以用户的登录 shell（`$SHELL`）启动，macOS 还补全从 Finder 启动时缺失的 PATH；Windows 优先 PowerShell 7（`pty/shell.rs`）。以子进程退出为准判断结束，之后按上文「会话结束」关闭窗格或按 Enter 重启。
- **线程**：Unix 上读写伪终端和等 shell 退出都在 tokio 里（非阻塞的终端句柄交给 `AsyncFd`，退出靠 SIGCHLD），会话不占线程，空闲时不唤醒；关闭时先 SIGHUP，2 秒后还在就 SIGKILL。Windows 上 ConPTY 的管道只能阻塞读写，每个会话用三个线程（读、写、等退出）。

### 配置与设置

- **存储**：配置目录（macOS `~/Library/Application Support/ZShell/`，Windows `%APPDATA%\ZShell\`，Linux `~/.config/ZShell/`），以应用名命名，不用 Tauri 默认的 bundle identifier（`lib.rs` 的 `config_dir`）；凭据存储的 service、缓存与 WebView 数据仍用 identifier。会话、文件夹、代理、快速命令与密码由保险库管理（见「加密保险库」）；`settings.json` 与 `logs.json` 总是明文。unix 上目录为 0700，文件创建时即为 0600。会话或代理与它的密码在同一次写入里保存、复制与删除，没有只成功一半的情况。
- **单实例**：两个进程会互相覆盖配置。Windows 用 single-instance 插件；macOS 与 Linux 按配置目录加锁（`instance.rs`，`flock`），第二个实例把已有的提到前台后退出。macOS 只在打开同一个 `.app` 时复用已有实例（另一个副本、`open -n`、直接运行可执行文件都会再开一个）；插件在 macOS 上的锁与 HOME 无关，会挡住端到端测试的隔离实例，所以不用。Linux 上一个进程不能把另一个的窗口提到前台，由持有锁的实例经 Unix socket 接过第二个实例的激活令牌，自己提到前台。
- **读不出的文件**：`settings.json` 与 `logs.json` 经 `JsonFile`（`persist.rs`）读写。内容读不出的改名保留，从空开始；文件本身读不出的留在原处、本次运行不再写它。应用不因此无法启动，之后的保存也不覆盖原文件，前端启动后列出这些文件。写入是临时文件加改名。`vault.json` 不走这条路，读不出时停在解锁界面（见「加密保险库」）。
- **设置**：页面启动时读入（见「前端」的外部 store），修改即时生效并保存，后端校验。界面文字大小经 CSS 变量 `--text-scale` 缩放，标题栏、图标与终端不变。主题经 `<html data-theme>` 选择 CSS 变量组，原生窗口同步跟随（`window_set_theme`）。Linux 上 tao 对"跟随系统"和固定配色的处理都不对，由后端从设置 portal 读桌面的配色方案并纠正 GTK 的 prefer-dark（`window/linux.rs`）。
- **会话与文件夹**：会话与文件夹在同一个文件夹里混排，显示顺序单独存为保险库里的 `order`；会话、文件夹两个列表也按这个顺序保存，不认识 `order` 的旧版本仍能打开，只是文件夹在前（不升保险库版本，兼容细节见 `config/mod.rs`）。新建与导入的项排在所在层的最后，复制的会话紧跟原会话。拖拽只有一个后端操作 `tree_move`。删除文件夹时内容移到上一级，占据它原来的位置。加载时修正坏引用和循环；指向不存在的文件夹或代理的引用保留（手改的文件），找不到代理的会话报错而不是直连。
- **导出 / 导入**：与 `vault.json` 同样的信封格式（`format: "zshell-sessions"`、`version: 3`），含文件夹、会话、代理与它们的显示顺序（导入时新加的项按文件里的先后排在所在层的最后）；带密码时整个文件用口令加密，导入时先输口令才能看到内容（见「加密保险库」）。导入时已存在的会话、代理按名称或地址匹配，不重复导入；跳板机与代理随所选会话一并导入；列表里显示导入后会执行的代理命令与会监听的转发规则；文件在显示后变了则拒绝导入，导入的总是显示过的内容。文件可能是别人的，导入的内容经过与保存时相同的校验（`backup.rs`）。
- **快速连接**：不需要已保存的会话；"另存为会话"改变标签的目标但不重连。
- **会话日志**：后端在 `SessionSink::write` 里把终端显示的内容交给日志线程，不经过前端，所以不回显的密码不会进日志；日志慢时丢弃放不下的部分并记下缺了多少，不拖住会话。清理只针对 `logs.json` 索引里的文件（`logging/`）。
- **ssh_config 导入**：一次性复制，之后与 config 无关联。解析库不认识 `Match`、`Include` 也不限层数，所以交给它之前按 OpenSSH 的方式预处理（`import/`）。
- **`~` 与主目录**：私钥路径的 `~` 由后端展开，与 OpenSSH 用同一个目录；主目录下的私钥存成 `~/…`，导出的会话换台电脑也能用。
- **单会话外观**：会话可以覆盖配色、背景色、字体和字号，只由前端解释，编辑后已打开的标签立即应用。
- **登录后命令**：由前端在每个新 shell 出现提示符后逐条发送，用户一开始输入就放弃剩下的（`lib/paneSession.ts`）。

### 加密保险库

- **保护方式**（`vault/`）：不加密、系统凭据存储（数据密钥在凭据存储里）、主密码（数据密钥以主密码派生的密钥包装，可选同时存进凭据存储）。三种都只有一个文件 `vault.json`，当前方式写在它的头部，不另存标记。没有 `vault.json` 时（首次启动、重置之后）新建一个：凭据存储可用就用系统凭据存储，否则不加密，不先询问用户。新建它的那次 `vault_try_unlock` 返回 `created`，那个窗口随即显示一次保护方式对话框，说明用的是哪种并可以当场改成主密码；不另存"问过没有"的标记，新建本身只发生一次（重置后再新建会再问）。
- **范围**：会话、文件夹、代理、快速命令、密码与应用锁的设置。`settings.json`（外观要在解锁前生效）、日志与 `~/.ssh/known_hosts` 不加密。
- **格式**（`envelope.rs`）：`{ header, data }`（不加密）或 `{ header, sealed }`（AES-256-GCM 加密的全部数据）。头部是按 `protection` 区分的枚举，各方式只有自己的字段；加密时头部在文件里的原样字节作为 AAD，被改动即解不开，所以读出时保留原始字节，不重新序列化。口令派生用 PBKDF2-HMAC-SHA256，参数随文件保存。导出文件用同一格式，带密码时整个文件以口令派生的密钥加密，不再单独绑定内容（`backup.rs`）。
- **读写**：`Vault` 持有全部数据。修改（写文件、由主密码派生密钥、读写凭据存储，后者可能等用户回答）一次一个，完成后发布一份快照；查询只读快照，从不等修改，所以主线程上的查询（同步命令、macOS 锁屏通知）不会被卡住。各存储有自己那部分的副本（`vault::Part`，读取时做了检查与修复）；保险库数据整体换过（解锁、新建、重置）时副本在下一次查询时自己重读，不靠逐个通知存储。修改经 `Part::update` 在保险库的锁内、基于当前数据（而不是可能过时的副本）进行，再经 `Vault::update` 整体写出（临时文件加改名）。数据只有几 KB，整体重写换来的是任何修改与更改保护方式都只是一次改名。写出前在内存里把要写的字节重新解析、解密一遍，与数据比对。保险库读不出、解不开或版本不对时保持原样，从不覆盖：解锁界面可以重试（手改坏了的不加密文件改好后重试即可）、在文件管理器里显示文件或重置。
- **解锁**：启动时只读 `vault.json` 的头部（不加密的直接打开），不碰凭据存储（可能弹授权框，或在 Linux 上等 D-Bus 超时），也不新建；之后前端调用 `vault_try_unlock` 读密钥或新建，期间只显示解锁界面，没有会话也没有标签。界面外壳（`LockScreen`）与应用锁共用。忘记主密码只能重置。
- **凭据存储里的密钥**：条目名带密钥的 id（`vault-key:<id>`），文件头部的 `keyId` 总能找到打开它的那一条。更改保护方式每次换新的数据密钥：先存新密钥，再替换文件，最后删旧密钥；文件写不成就删掉刚存的新密钥。任何一步停下，磁盘上的文件都有对应的密钥或主密码，最多留下一条打不开任何东西的条目（凭据存储无法列举，残留的无法清理，但无害）。debug 构建用另一个 service（`org.boyin.zshell.dev`），测试不会碰到正式应用的密钥。
- **内存中的数据**：`zeroize` 只用于密钥（数据密钥、由口令派生的密钥，以及从凭据存储读出的密钥文本解码后的字节）和保险库文件的明文（写出前的序列化结果、读入时解密或读出的内容），用完即清零。解锁后的数据（会话与密码）在内存里是普通的值，查询读的是同一份快照，各存储的副本只取自己那部分（不复制密码）；和应用锁一样，不防能读进程内存的人。
- **会话读密码**：会话、跳板机与代理保存的密码由 `ProfileStore::route` 随 `Route` 交给会话任务（`vault::SavedPasswords`），服务器或代理要求时才从保险库读出；快速连接的 `Route` 没有。会话读什么都在交给它的参数里，不经全局变量。
- **凭据存储**（`secrets.rs`）：`CredentialStore` 接口，系统实现是 keyring（Linux 为 Secret Service），测试用内存实现。Linux 上可能没有 Secret Service，这时只能选主密码或不加密。存储种类随保险库状态返回，界面文案按种类选择，前端不猜平台。

### 应用锁

- **范围**（`applock/`）：盖住窗口直到系统认证通过，会话照常运行；只锁界面，内存中的数据不重新加密（要让保险库受生物识别保护，需把数据密钥放进要求生物识别的钥匙串项，macOS 上需要开发者证书）。macOS 用 LocalAuthentication（Touch ID，没有时退回登录密码），Windows 用 Windows Hello（认证框挂在主窗口上），Linux 用 polkit（桌面自己的认证框，GNOME Shell 的：账户密码，配置了 fprintd 时也可用指纹）。polkit 询问应用自己的 action `org.boyin.zshell.unlock`（`auth_self`，每次都问），策略文件由 deb / rpm 装进 `/usr/share/polkit-1/actions/`，从源码目录运行的构建没有它，应用锁不可用；subject 用应用在 system bus 上的名字，由 polkit 自己查出进程，不由应用报 pid。后端报告认证方式、能否检测闲置与锁屏，前端据此显示，不猜平台。
- **状态在后端**：启动即为锁定，只有后端调用系统认证通过，或输入主密码解锁保险库，才解开；页面重载后仍然锁着。锁定时不问系统有什么认证方式（睡眠与锁屏只通知一次，而 Windows Hello 在系统挂起时可能答不上或答得慢），解锁时才问：认证方式确实没了（如删除了 Windows Hello PIN）则在锁定界面说明原因，由用户点「不经认证解锁」解开，不会自己解开，因为做到这一步需要账户密码或管理员权限，而有这些的人本来就能读出进程内存里的会话；只是询问失败则照常要求认证。各平台哪些情况算「没了」见 `applock/mod.rs`。
- **设置在保险库**（`Data::app_lock`）：与会话一样受保护，加密时改文件关不掉它，否则改完重启即可绕过；不加密时改文件就能关掉（设置里有提示）。所以 `AppLockGate` 在 `VaultGate` 之内，保险库解锁后才知道是否锁定，得到结果前什么都不显示。开启时要认证一次。
- **触发**：系统的闲置时间（不只是 ZShell 窗口，终端输出不算活动；Touch ID 与人脸不算输入，所以最多从上次解锁算起；Linux 上只有 GNOME 的 Mutter 提供，经 D-Bus 查询）；系统锁屏、睡眠、切换用户，在事件发生时锁定，醒来时画面已被盖住（Linux 上是 logind 的 `PrepareForSleep`、会话的 `LockedHint` 与 `Lock`；GNOME 切换用户前先锁屏）；手动（⇧⌘L / Ctrl+Shift+L，应用菜单）。
- **锁定界面**：模态 `<dialog>`，在 top layer 里盖住一切，其余页面（包括渲染到 body 末尾的对话框、菜单与提示）都变为 inert，应用保持挂载。只在窗口有焦点时自动请求认证（闲置锁定时用户可能在别的应用里），取消后只由按钮请求。
- **锁定期间**：React 之外的部分经 `lib/appLock.ts` 的 `isAppLocked()` 判断：快捷键、对话框栈的按键、`announce()`、程序的系统通知（不带内容）、原生拖放（不受 inert 影响），以及会话自己打开的原生对话框（ZMODEM 的文件选择，解锁后再打开）。关闭窗口的确认显示在锁定界面上。锁定时已经打开的原生文件对话框盖不住（已知限制）。

### 检查更新

- **查什么**（`update.rs`）：GitHub 上最新一个已发布 release 的版本号（`releases/latest`，不含草稿与预发布），按 semver 与当前版本比较。只提示，不下载：用户在浏览器里从 release 页面下载新版本。请求的 User-Agent 只写 `ZShell`，走系统的 TLS（Security.framework / SChannel）与系统代理。对外说明见 `PRIVACY.md`，改动行为时同步修改。
- **何时查**：设置开启时（默认开启）由后端每天最多查一次，启动后稍等片刻再查。上次检查的时间和结果存在 `update.json` 里，只是缓存，读不出就当没查过；一天内多次启动不会重复检查。手动检查随时可以：帮助菜单（macOS 在 App 菜单）的"检查更新…"打开 设置 › 关于，结果显示在那里。每次检查的结果经 `update-status` 发给所有窗口。
- **怎么提示**：没被跳过的新版本标在通往 设置 › 关于 的路上：Windows 的菜单按钮、帮助子菜单和"检查更新…"项，macOS 侧栏的设置按钮，以及设置导航里的"关于"。运行中新发现的版本在有焦点的窗口播报一次。跳过的版本存在设置里。
- **Microsoft Store 版**：有包身份（`GetCurrentPackageFullName`）时不检查，相关界面都不显示，更新由 Store 负责。

### 渲染与 memo

- **React Compiler**：构建时经 Babel（`vite.config.ts`）给每个组件与 hook 自动 memo，依赖没变的部分不再重新计算和渲染。它跳过编不了的函数时不报错，那个函数就没有 memo，所以 `compiler.test.ts` 用同样的方式编译 `src/`，任何函数被跳过都失败；真编不了的写 `"use no memo"` 并在注释里说明原因。代价是构建多约 10 秒、产物多约 150 KB（运行时在 React 19 里）。
- **渲染时不碰 ref**：Compiler 跳过渲染时读写 ref 的组件。要在事件里拿到最新的 props 或 state，用 `useLatest`（在 insertion effect 里赋值，早于所有 layout effect；名字以 `Ref` 结尾，Compiler 才当它是 ref），只在 effect 里调用的回调用 `useEffectEvent`；DOM 的尺寸、元素在事件、effect 或 ResizeObserver 里取得后存进 state；ref 与普通值放在一个对象里时（`useConfirmButton`、`useDialog` 的返回值），整个对象都被当作 ref，在使用处解构。手写的 `useCallback` / `useMemo` 的依赖要与 Compiler 推断的一致（`useLatest` 的 ref 也要列进去），否则整个组件被跳过。
- **渲染时不读 React 之外的状态**：memo 之后，只有依赖变了才重新计算，靠「每次都重新渲染」才拿到新值的写法会显示旧值。外部 store 经 `useSyncExternalStore` 订阅（`hooks/stores.ts`）；随界面语言变化的文字（快捷键名称、大小与时间的格式、CJK 字体顺序）从 `useLocale` 取得，函数随语言变化，`locale.test.ts` 检查组件不直接引入 `lib/` 里的版本；会话注册表也是：它在窗格的会话与终端挂接、断开时通知订阅者，状态栏经 `useSyncExternalStore` 取流量表和终端尺寸。
- **Compiler 尚不支持的语法**：带 `finally` 或没有 `catch` 的 `try`、`try` 里的条件表达式与 `?.` / `??`、`||=` / `??=`、对模块变量的 `++`，哪怕只出现在组件内的事件处理函数里，整个组件也会被跳过。`try` 语句经 `lib/attempt`（含义相同），其余写成显式形式或组件外的函数。
- **热路径**：标签页 store 可能每秒变化多次（后台程序用 spinner 改标题），`App` 随之重新渲染。Compiler 按依赖 memo，依赖链上某处随标签页变化时（`openProfile` 用到 `useWindows` 的返回值，后者依赖 `tabs`），子组件仍会重新渲染，而且没有提示。所以不该随标签页重新渲染的组件，回调由 `useStableHandlers` 组成一个对象传入（侧栏的 `SidebarHandlers` 与应用菜单按钮 `AppMenuButton`、标签页的 `PaneHandlers`），会变的值另作 props，开发版的 `useStableProps` 在它们拿到新的函数或 handlers 对象时 `console.warn`。频繁重新渲染的列表（随标题变化的标签栏、随活动窗格变化的快速命令栏、随传输进度每秒十次的文件列表与传输列表），每一项拆成单独的组件（`TabItem`、`CommandButton`、`FileRow`、`TransferItem`）：Compiler 不 memo 循环里创建的元素与闭包，写在 `map` 里的每个元素每次都拿到新的 `onClick`，React 随之重设元素的 `onclick`；拆开后只有变了的那一项重新渲染。各项的回调同样经 handlers 对象，标签栏的由 `App` 传入（`TabBarHandlers`）。

### 终端连字

- **分组与字形**：xterm.js 按单元格绘制，连字要由 character joiner 告诉渲染器哪几格合在一起画；合并后的文字交给浏览器绘制，字形由字体自己的 `calt` 规则决定，joiner 只决定分组。WebGL 渲染器只合并 joiner 给出的格子；DOM 渲染器（隐藏的标签页、腾出 WebGL 的终端、没有 WebGL 时）把同一样式的一段整体交给浏览器，所以关闭时给终端加 `font-variant-ligatures: none`，开启时 DOM 渲染器显示字体的全部连字。光标所在的格子与部分选中的范围不合并。
- **三档**（`terminal.ligatures`，默认关闭）：「常用序列」用固定的序列表（`lib/ligatures.ts`，Iosevka 的 `calt` 集合，与 VS Code 的默认值相同），不读字体；脱离上下文后与字体的规则可能不同（更纱单独的 `<=` 不连，按列表会连成 ≤）。「按字体规则」按字体的 GSUB 规则分组，读不到字体时退回常用序列。
- **按字体规则的数据流**：xterm.js 的连字插件不直接用：它经 `queryLocalFonts` 读字体，macOS 与 Linux 的 WebView 没有这个接口，Windows 上读到的是整个 TTC，每个终端各解析一次，而且它只实现了 `calt` 里的一部分规则，分组常与浏览器画出的不符。改为后端 `fonts/` 按终端的字体列表找第一个已安装的字体（DirectWrite 能找到 Microsoft Store 安装的字体，它们不在任何字体文件夹里；CoreText 照 WebKit 先按任何语言的家族名、再按 PostScript 名查找；fontconfig 给出文件与 TTC 序号）。已安装却读不出的字体不再往后找：下一个字体不是终端画的那个，其规则只会分错组，不如退回常用序列。只读 `cmap`、`GDEF` 与 `GSUB`，编成紧凑的规则（`rules.rs`）：只有规则区分得开的字形保留编号，其余字形都算作同一个，各表写进一个数字数组，经 `terminal_ligature_rules` 交给前端（更纱约 80 KB 的 JSON）。前端 `lib/fontLigatures.ts` 用 typed array 同步地应用规则，同一字体列表的终端共用一份（`lib/fontLigatureRules.ts`），没有终端使用时释放。前后端对格式的理解由一份 fixture 检查（见 [DEVELOPMENT.md](DEVELOPMENT.md)「常用命令」）。
- **怎样应用**：照 HarfBuzz（WebView2 与 WebKitGTK 的排版引擎）处理不指定特性的横排拉丁文字：选 `cmap` 子表的顺序；`latn`（没有时 `DFLT`）脚本的默认语言系统；先 `rvrn`，再把 `ccmp`、`locl`、`rlig`、`rclt`、`calt`、`clig`、`liga`、`ltra`、`ltrm` 与必需特性的查找表按序号一起应用；lookupFlag 按 `GDEF` 的字形类别跳过字形，匹配时跳过默认可忽略字符（零宽不连字只在上下文里跳过）；嵌套的查找表增删字形时调整匹配位置。有三处不照做，因为查过的编程字体（Cascadia Code、Fira Code、JetBrains Mono、更纱等）里它们改变不了分组：不按页面语言选语言系统（DOM 渲染器用文档的语言，WebGL 的 canvas 用它自己的，两者不一定相同；在界面的语言下，这些字体最多只多出改单个字母的 `locl`），只含标点的一段不改用 `DFLT`（这些字体在 `latn` 与 `DFLT` 下的特性相同），也不做排版前的规范化（如把 `e` 与 U+0301 合成 `é`）与为没有 `GDEF` 类别的字体推测类别（有连字的字体都有 `GDEF` 类别，大多也不用 lookupFlag）。canvas 关不掉这些特性，合并后 WebGL 画出的是它们全部的结果，所以都要算进去。CoreText（macOS）的排版结果可能与 HarfBuzz 有细微差别。
- **合并哪些格子**：规则改了字形的地方，连同它匹配时的上下文（从回溯的第一个字形到前瞻的最后一个）合成一段；与一段部分重叠的其他规则匹配（包括什么都不改、只为阻止连字的规则）把这段扩大到自己的上下文，直到不再变化。渲染器把一段当作一串文字重新排版，上下文都在段里才得到同样的字形；只涉及一个字符的替换不用合并。重写时用 harfbuzzjs 对照过更纱、Cascadia Code、Consolas、Fira Code、JetBrains Mono 与一个没有 `GSUB` 的字体，各 8 万行文字：替换后的字形逐个相同，每一段（以及段外的每个字符）单独排版的结果都与整行排版时一样。
- **代价**：后端读表与编译几毫秒，前端建表约 1 ms、占几百 KB。分组结果按文字缓存（10 万字符），内容不变的重绘不再计算；大量新输出时每帧（50×200）约 1–15 ms，视字体与内容而定，常用序列约 2 ms。

## 交互约定

### 无障碍

目标是读屏用户（VoiceOver、讲述人、NVDA、Orca）能完成连接、操作终端、使用 SFTP 与主要对话框，全部功能可以只用键盘完成；不追求逐条符合 WCAG。

- **终端**：xterm.js 的 `screenReaderMode` 有性能开销，由设置里的"读屏支持"决定：默认"自动"，系统的读屏软件运行时打开，也可以固定开或关。WebView 拿不到读屏软件是否运行，由后端查询系统并在变化时向所有窗口发 `screen-reader-changed`（`screen_reader/`：macOS 用 KVO 观察 `NSWorkspace.voiceOverEnabled`，Windows 读 `SPI_GETSCREENREADER` 并由隐藏窗口接收 `WM_SETTINGCHANGE`，Linux 监听 AT-SPI 的 `org.a11y.Status.ScreenReaderEnabled`）；前端在 `SettingsProvider` 里得出实际是否开启。系统要求更高对比度时提高终端的最低对比度，配色本身不变。
- **对话框**：`Modal` 带 `role="dialog"`（确认类为 `alertdialog`）与 `aria-modal`，以标题命名。打开时聚焦第一个控件，Tab 在最上层对话框内循环，关闭后焦点还给打开前的元素。不给根元素加 `inert`，因为 Windows 的窗口按钮在里面。
- **列表**：焦点停在列表容器上，用 `aria-activedescendant` 指向当前行。标签栏是 `tablist`，会话列表是 `tree`（搜索时为 `listbox`），文件列表是多选的 `grid`。各处菜单都可以用 Shift+F10 或菜单键打开。WebKit 对角色结构的要求见 `Sidebar.tsx` 的注释。
- **播报**：`lib/announce.ts` 是唯一的 live region，播报一闪而过或不在视线内的变化（后台窗格断开、传输完成、已复制）。
- **样式**：键盘焦点统一画强调色轮廓，去掉轮廓必须用别的方式显示焦点。尊重减少动态效果与 Windows 对比度主题；文字与背景对比度至少 4.5:1，白字所在的填充用更深的 `--accent-fill`。
- **验证**：macOS 上可以用 `osascript -l JavaScript` 经 System Events 读出窗口的辅助功能树；实际体验仍要用读屏软件试。

### 图标

- **只用 SVG**：图标全部是 `components/icons.tsx` 里的 SVG 组件，不用文字字形或 emoji：×、▾、⟳、📁 这类字符在各平台字体里的形状和基线都不同，过去多次因此错位。`icons.test.ts` 在 `icons.tsx` 之外出现 `<svg>` 或这类字符时失败（例外见测试）。
- **画法**：16×16 网格，`currentColor` 描边，`aria-hidden`，名称由所在的按钮给出（只有图标的按钮用 `IconButton`，它的 `label` 也是悬停提示）。对齐规则见 `icons.tsx`。

### 颜色选择

- **不用原生颜色框**：`<input type="color">` 弹出的是各平台自己的颜色窗口：WebView2 里吸管按钮点了没反应（取色要宿主实现，WebView2 没有实现，`window.EyeDropper` 也一样，`open()` 会立即以取消告终），GTK 的对话框没有吸管，三个平台的外观也各不相同。颜色字段统一用 `components/ColorField.tsx`：点色块打开页面内的弹层，里面有饱和度/亮度区、色相条、十六进制输入框和吸管按钮。弹层不引入第三方组件，因为常见的组件把读屏文字写死成英文。
- **吸管**：从屏幕取色由后端的 `color_pick` 命令完成（`color_pick/`）。Windows 用自己的全屏窗口显示虚拟桌面的截图，macOS 用系统的 `NSColorSampler`，Linux 用截图 portal 的 `PickColor`。命令返回 `#rrggbb`，用户取消时返回 `null`。同一时间只能有一次取色，重复调用返回 `colorPick.busy`。

### 快捷键、菜单与侧栏

- **快捷键**：在捕获阶段拦截，终端收不到。所有快捷键在 `lib/keymap.ts` 的一张表里声明，按物理按键匹配，应用菜单（包括 macOS 的原生菜单栏）、右键菜单的文字和设置里的列表都由它得出，改快捷键只改这一处。Windows 上与 shell 冲突的快捷键加 Shift（Ctrl+F、Ctrl+W 留给 shell）。对话框打开时只有"打开设置"生效，免得操作对话框背后的标签。
- **终端全屏**（`app/useFullScreen.ts`）：窗口全屏并隐藏终端以外的界面（隐藏而不是卸载，退出后原样恢复）。
- **应用菜单**：结构和快捷键一起在 `keymap.ts` 里声明（`appMenu`），分为 App、文件、编辑、视图、帮助几部分。
  - macOS：页面把菜单的结构、文字和按键发给后端（`menu.rs`），后端与系统自带的项（关于、服务、隐藏、退出，编辑菜单的撤销到全选，窗口菜单）一起构建原生菜单栏。WKWebView 的 ⌘C / ⌘V / ⌘Q 等依赖原生菜单项；页面加载前菜单栏只有系统项。原生菜单项的按键先被菜单拿到，所以这些动作在 macOS 上不由页面匹配按键，而是由菜单向最近有焦点的窗口发 `menu-action`。关闭窗口与退出由后端直接处理，页面没加载或停在解锁界面时也能用；退出交给前端汇总各窗口后确认（见「多窗口」）。
  - Windows 与 Linux：没有菜单栏。标题栏左上角的图标与名称是菜单按钮（也可按 Ctrl+Shift+M），页面内的 `ContextMenu` 把各部分显示为子菜单，App 部分的项（锁定、设置、退出）放在顶层。
  - macOS 窗口全屏时红绿灯隐藏，菜单栏也看不到，标题栏左上角同样显示这个菜单按钮，菜单与 Windows 的相同（没有快捷键）。
- **菜单项即快捷键**：选中菜单项等于按下它的快捷键。`runAction` 用一个代表该动作的按键事件走 `useShortcuts` 的同一套处理，所以对话框、应用锁、当前终端等规则与快捷键完全一致；没有快捷键的动作（新建会话、导入导出、帮助）也在表里。页面内的菜单先把焦点还给打开它之前的位置，再执行动作，效果与按快捷键相同。Windows 的菜单在打开时按当前窗口的状态禁用不适用的项；macOS 的菜单项始终可用，不适用时什么也不做。
- **右键菜单**：页面内绘制的 `ContextMenu`（可以有子菜单），WebView 自带的右键菜单只在文本输入框里保留。
- **悬停提示**：WKWebView 不显示 `title`，所以由页面自己画（`Tooltips`），组件照常写 `title` 即可。
- **滚动条**：用系统滚动条，跟随深浅色，不用 `::-webkit-scrollbar` 自绘，以免 macOS 失去悬浮滚动条；终端的由 xterm.js 自绘。
- **会话列表**（Xshell 风格）：单击选中（按在行外的空白处取消选中，新建的会话与文件夹随之放在顶层），双击、Enter 或右键菜单的「连接」打开（两次点击须在同一行，第一次点击使行移动时不算双击）：当前标签页有焦点的窗格就是这个会话且已断开（含等待自动重连）时在原处重连，否则开新标签页；单击文件夹折叠或展开，打开其中全部会话在右键菜单里（双击太容易误触）。搜索框按名称、地址、用户名过滤，只靠备注匹配的会话排在后面；输入像地址时提供快速连接。会话的备注显示在它的悬停提示里（过长时截断）。

### 标题栏

- **布局**：没有原生标题栏，窗口顶部一行在侧栏右边界处分为侧栏顶行与标签栏。窗口由 `window/` 按平台创建：macOS 用 Overlay 标题栏，红绿灯居中于顶行；Windows 无边框，窗口按钮自绘（`WindowControls`）；Linux 保留 GTK 的客户端装饰（阴影、圆角、从边缘外缩放，与 GNOME 自己的应用一样），只把标题栏换成空控件（要在窗口 realize 之前，所以窗口先隐藏着创建），窗口按钮自绘成 GTK 的圆形样式，按桌面的按钮布局（`gtk-decoration-layout`，GNOME 默认只有关闭）显示，布局变化时更新。
- **拖动**：由 `hooks/titleBar.ts` 处理带 `data-window-drag` 的元素，双击与右键交给后端，行为与系统一致（macOS 与 Linux 上双击按系统设置，Linux 右键弹出 GNOME 的窗口菜单，它需要一个输入事件，所以后端记着 webview 最近一次按键）；对话框遮罩顶部那一条也能拖动窗口。
- **Windows 贴靠布局**：每个窗口的最大化按钮上方放一个不绘制的原生子窗口，让系统认出它是最大化按钮（`window/win.rs`）。

### 剪贴板与终端输入

- 读写剪贴板用 `tauri-plugin-clipboard-manager`，因为菜单里的粘贴触发不了浏览器的 paste 事件，WKWebView 的 `readText` 又会弹出系统确认；macOS 的 ⌘C / ⌘V 仍走原生菜单。
- 所有粘贴走 xterm.js 的 `term.paste()`。多行内容在远端没开 bracketed paste 时先确认，开了则 shell 只插入不执行，无需确认。
- 远端程序开启鼠标模式时右键交给程序，Shift+右键总是打开菜单；Option 作为 Meta 只在 macOS 上可选。
- **链接**：文本里的网址（`@xterm/addon-web-links`）和程序标记的 OSC 8 链接，⌘+点击（其他平台 Ctrl+点击）用系统浏览器打开，普通点击只是选择与聚焦；悬停时提示按法，OSC 8 链接的文字可以与地址无关，提示里还显示地址。在链接上打开的右键菜单多出"打开链接"和"复制链接地址"。只认 http 与 https。

### 程序的 OSC 请求

xterm.js 自己处理标题、调色板、OSC 8 链接与颜色查询（OSC 0–2、4、8、10–12、104、110–112）；其余由 `TerminalView` 注册处理函数，解析是 `lib/osc.ts` 里的纯函数。都只作用于发出序列的窗格，与同步输入无关；文本日志本来就去掉 OSC 序列。

- **文件夹（OSC 7）**：见「文件传输与端口转发」。
- **剪贴板（OSC 52）**：程序写入的文本经剪贴板插件放进系统剪贴板，并 `announce()`；`c`、`p`、`s` 等选区都当作系统剪贴板（插件没有 Linux 的 primary selection）。只写不读：查询（`?`）不回应。可关闭（`terminal.clipboardWrite`），超过上限（`terminal.clipboardLimit`，KB，最多 7168：xterm.js 丢弃超过 10 MB 的 OSC 负载）的、清空剪贴板的与不合法的都忽略。tmux 3.3 起在 `TERM` 为 `xterm*` 时默认把复制转发成 OSC 52。
- **进度（OSC 9;4）**：ConEmu 的定义（0 清除、1 进行中、2 错误、3 不确定、4 暂停），不带百分比时沿用上一次的。`PaneSession` 记住窗格的进度，会话结束、断开或 30 秒没有更新时清除（崩溃或被 Ctrl+C 打断的程序不会留下一直挂着的进度），经 `Pane.progress` 进入标签页 store；窗格移到其他窗口时不带过去。标签显示其窗格合并的进度环（`mergeProgress`：错误 > 暂停 > 进行中 > 不确定，同为进行中取最小的百分比），后台标签出错时 `announce()`。窗口把它所有标签合并的进度经 `window_set_progress` 显示在任务栏按钮上（`app/useTaskbarProgress.ts`）；macOS 的 Dock 与 Linux 的 libunity 整个应用只有一个，由后端以同样的规则合并各窗口的（`window/progress.rs`）。
- **通知（OSC 9、777、99）**：iTerm2 的 `9;正文`（以数字加分号开头的是 ConEmu 的子命令）、urxvt 的 `777;notify;标题;正文`、kitty 的 `99`（只取标题与正文，支持分块与 base64；按钮、图标、关闭回报与查询不实现、不回应）。可关闭（`terminal.notifications`）；每个窗格最多 3 秒传一条，其间来的只留最后一条，到时再传（常是更要紧的「失败」而不是「开始」）。窗格所在的标签在前台且窗口有焦点时不提醒；标签在后台时像响铃那样加提醒标记（`Tab.attention` 的 `notice`，与响铃同级，先来的保留），窗口有焦点时 `announce()`；窗口没有焦点时发系统通知（`tauri-plugin-notification`，首次发送时请求权限）；应用锁定时也发系统通知，但只说有程序发来通知，不带标题、正文与窗格的标题。插件在桌面上不回报点击，点通知只能激活应用。
- **鼠标指针（OSC 22）**：名称是 CSS 的 cursor 值，也接受 X11 光标字体的名称（`hand2`、`xterm` 等，按表换算），查询不回应。照 kitty 每个屏幕一个栈（`=` 替换、`>` 压入、`<` 弹出），全屏程序退出后恢复 shell 的；会话换了、终端重置（菜单或 RIS）时恢复默认。指针画在 `.xterm` 上，盖过文本光标与鼠标模式的箭头，链接的手形与列选择的十字仍优先。

### 标签页

- **分屏**：一个标签是一棵窗格树，每个窗格是独立的会话。窗格平铺渲染、按树算出的位置绝对定位，切分、关闭、调整大小都不会重新挂载其他窗格的终端（卸载即结束会话，`lib/layout.ts`）。新窗格默认复制当前窗格（SSH 复用连接）。布局不保存。
- **终端挂在哪里**：窗格的终端与文件 / 转发面板不由标签页渲染，而由 `App` 下按窗格 key 排成一列的 `PaneHostList`（`components/PaneHost.tsx`）经 portal 画进每个窗格自己的宿主元素（`components/paneHosts.ts`，`display: contents`，不产生盒子），窗格所在标签的页面把宿主放进窗格的位置和侧栏里。React 换了父节点的组件一定会重新挂载，这样窗格换标签只是宿主元素换了位置，xterm 实例、WebGL、选区、滚动位置和面板里进行中的传输都不受影响，也不需要像跨窗口那样序列化屏幕。portal 里的 React 事件沿 React 树冒泡到 `PaneHost`，不经过窗格的 `div`，所以窗格跟踪焦点用 DOM 事件。
- **移动窗格**：终端右键菜单的「移到新标签页」（分屏时）与「将窗格移到」子菜单（新窗口、其他窗口、本窗口的其他标签页），或拖动窗格顶部的抓手（指针靠近窗格上沿时出现）：放到标签栏成为新标签，停在某个标签上片刻切换过去，放到某个窗格上按离哪条边近（沿对角线分成四块）放在那一侧，拖出窗口与拖出标签相同。菜单移到其他标签时放在它当前窗格旁，较宽就放右边、较高就放下面（`autoSide`）。从标签栏把标签拖到当前标签的窗格上，整棵窗格树合并过去；拖着标签停在另一个标签的中间一半上不越过它，片刻后切到那个标签（`lib/tabRest.ts`，拖窗格时停在标签上也一样；停留中的标签高亮，底部的进度条填满时切换），再拖到它的窗格上合并。拖动途中切过去的标签只为找落点：没合并、也没拖出窗口时，切回拖动前显示的标签。每个落点都检查合并后的窗格不小于最小尺寸。抓手只给指针（`aria-hidden`），键盘与读屏用户用终端的右键菜单（Shift+F10），长菜单可以滚动并按首字母跳转。同一窗口里的移动是 `reduceTabs` 的 `movePane` / `mergeTab`，标签移出中（`Tab.moving`）时不动；移到其他窗口是只带一个窗格的「移动标签」（见「多窗口」），条件相同。
- **当前窗格**：标签的标题、状态、右键菜单与文件 / 转发面板都跟随有焦点的窗格；每个 SSH 窗格各有一份面板实例。
- **标题**：重命名的标题 > 远端标题（OSC 0 / 2，可关闭）> 会话名。
- **提醒**（可关闭）：后台标签里的窗格有输出时，标签的状态灯外加一圈，切到该标签后消失（`Tab.attention`，显示中的标签在 `reduceTabs` 里随时清掉）。窗格刚收到输入或改变大小后 300 ms 内的输出是回显与重绘，不算（`PaneSession` 的 `ECHO_MS`）；输出频繁，只有尚未标记的后台标签才写入 store。
- **响铃**（可关闭）：xterm.js 解析到 BEL 时经 `PaneSession.bell()` 上报，1 秒内的连续响铃算一次。提醒：后台标签显示铃铛（优先于新输出的圆环）并经 `announce()` 播报，窗口不在前台时请求系统提醒（`requestUserAttention`，Dock 跳动或任务栏闪烁）；紧跟在输入之后的响铃（补全失败）不提醒。提示音（默认开）由后端播放系统提示音（`app_bell`，`window/` 的各平台文件）。闪屏（默认关，关掉提示音时用）在该终端上叠一层前景色后淡出，减少动态效果时改为静止显示而不跳过。提示音与闪屏不区分是否由输入引起。
- **拖拽排序**：像浏览器那样，拖动中的标签浮起来跟着指针走，越过的标签滑到另一边让出位置（`dragSlot`，`lib/tabStrip.ts`），放下后滑进新位置。拖动中只改各标签的 transform，不改标签页 store，也不重新渲染，顺序在放下时才改一次。离开标签栏（拖到窗格上、拖出窗口）时标签回到原位，由合并预览或其他窗口标签栏里的蓝色竖线提示落点；从其他窗口拖来的标签和拖到标签栏的窗格还不是本窗口的标签，也用蓝色竖线。用鼠标事件而不是 HTML5 拖放，因为 Windows 上 Tauri 的文件拖入会拦截 HTML5 拖放。拖动窗格同样如此，拖出窗口的部分与拖动标签共用 `lib/windowDrag.ts`。
- **关闭确认**（可关闭）：已连接的远端窗格或有前台程序的本地终端先确认；文件面板有传输、在本地编辑器里打开的文件或待回答的问题时，不论是否连接也先确认，并说明会失去什么（断线时等待重连上传的保存也在内）。文件面板的这三项与「不能移动的标签」共用一个判断（`paneWork`，`lib/panes.ts`）。关闭窗口会结束它的全部会话，有这样的标签时总是确认（`app/useCloseFlow.tsx`）。
- **撰写栏与同步输入**：纯前端实现，经各窗格的 `PaneSession.send()` 发出，只发给 shell 已就绪的窗格，不会把口令发进别的窗格的认证提示。范围只在撰写栏打开时生效，关闭后只发当前窗格，避免范围停在"所有标签"而不自知；范围超出当前窗格时有醒目的标记（`lib/compose.ts`）。
- **快速命令**：按分组整体保存，默认分组总是存在；会话可以指定其标签默认显示的分组，与撰写栏共用发送逻辑。

### 多窗口

- **窗口从标签产生**：没有空白的新窗口。标签右键菜单"移到新窗口 / 移到某窗口"、把标签拖出窗口，或会话列表的"在新窗口中连接"，才会打开窗口。第一个窗口是 `main`，之后的是 `window-N`（capability 授权 `window-*`）；新窗口在来源窗口右下方错开，大小相同。窗口要显示的内容（新窗口打开时带的标签或会话，之后其他窗口移来的标签）由后端按窗口暂存，页面启动时和收到 `tab-received` 时取走（`window_take`），后端不解读它。暂存而不是直接用事件发，是为了让还在加载的页面也拿得到，并让"关空窗口"和"移入标签"互斥（见下文"关闭与退出"）。
- **只发给一个窗口的事件**：前端 `listen()` 不指定目标时，也会收到发给其他窗口的事件，所以一律用 `ipc/events.ts` 与 `hooks/windowEvents.ts` 的 `listenHere` / `useWindowEvent`（监听本窗口）。原生菜单项发给最近有焦点的窗口（`window::current`，也是 Windows 与 Linux 上再次启动时提到前台的窗口）。
- **跨窗口同步**：设置、会话与文件夹、代理、快速命令、保险库的保护方式、应用锁在一个窗口里保存后，后端通知其他窗口（`config-changed`），它们重新读取（见「前端」的外部 store；整体写入的设置与快速命令见「命令在哪个线程上运行」）。最近打开的会话在浏览器存储里，经 `storage` 事件同步。应用锁的状态本来在后端：一个窗口锁定或解锁，其他窗口跟着盖上或揭开，只有有焦点的窗口自动请求认证。
- **窗口标题**：窗口以当前标签的标题命名（macOS 的 Window 菜单、任务栏、其他窗口的"移到"菜单都用它），改变时通知其他窗口刷新列表。
- **移动标签**（`app/useWindows.ts`）：整个标签连同分屏布局与窗格状态（标题、状态、转发、日志、命令分组）作为数据交给目标窗口，目标窗口用自己的 key 插入。只移一个窗格时它作为单独的标签交出去，标签的其余窗格留下。
  - **接管会话**：来源窗口交出标签时同时把其中的会话提供给目标窗口（`window_send_tab` / `window_open` 的 `sessions`，`SessionManager::offer`）；目标窗口建好终端与新 Channel 后请后端换通道（`session_adopt`），只能接管提供给它的会话、各一次。后端在旧 Channel 上最后发一个 `moved` 事件再换，会话从这时起归目标窗格（输入直达，关闭窗格会结束它）。
  - **屏幕**：旧窗口收到 `moved` 时已写完之前的全部输出，这时序列化屏幕与回滚（`@xterm/addon-serialize`，加上它不保存的终端模式，`lib/terminalModes.ts`），经后端转交（`session-snapshot`）。目标窗口按原尺寸写入快照和在此期间缓存的输出，不丢不重，画完才适配窗格并把新尺寸告诉会话；画完之前不能再移走。每次移动有目标窗口取的 `moveId`，随 `moved` 和快照往返，迟到的旧快照不会被当成这次的。
  - **没有会话的窗格**直接带上快照，等待重连的到了目标窗口立即重连；移动期间它在旧窗口里不再连接，移动失败才恢复，以免两边各连一次。
  - **结束**：全部会话移走后旧窗口关掉这个标签。目标窗口接管失败（会话刚结束、ZMODEM 刚开始）时去掉这个窗格并告诉来源窗口（`pane-stayed`），目标窗口在接管完之前关闭时来源窗口从 `window-closed` 得知，目标窗口的页面在接管完之前重新加载（它已取走的标签随旧页面没了）时后端收回提供给它、还没接管的会话，并以 `offers-withdrawn` 告诉来源窗口；这两种情况下没移走的窗格都留在来源窗口。移出中的标签（`Tab.moving`）不能再移动，关闭它的请求也被忽略，否则会结束目标窗口就要接管的会话。这个过程是 `lib/tabMoves.ts` 里不依赖 React 的状态，有单元测试。
- **不能移动的标签**：文件面板有传输、本地编辑或待回答的问题，ZMODEM 进行中，窗格正在打开会话，或会话还在移入（快照还没画完）。这些状态属于旧窗口的页面（进度 Channel、文件对话框），菜单项此时禁用；后端也拒绝 ZMODEM 中的会话（`session.busy`）。文件面板在新窗口里重新打开，从终端所在的文件夹（找不到时从主目录）开始。
- **拖出标签**（`tab_drag/`）：在标签栏里拖动照常排序；拖出窗口后松手，落在另一个窗口的标签栏上就并入那里的对应位置，落在其他地方（包括另一个窗口的终端区）就在光标处开新窗口（保持在屏幕的工作区内）；窗口唯一的标签拖到其他地方什么也不做；Esc 取消。仍用鼠标事件：WKWebView 在按住按键移出窗口后照样收到 `mousemove` / `mouseup`，松手没被页面收到时以 `buttons === 0` 兜底。拖动中页面一次一个请求地问后端会落在哪里：落点窗口由系统判断（macOS `windowNumberAtPoint`、Windows `WindowFromPoint`），被其他应用的窗口挡住的 ZShell 窗口不算；光标用后端读的屏幕坐标，不用网页的 `screenX`（多显示器、缩放不同时不可靠，坐标换算见 `tab_drag/mod.rs`）。各窗口报告自己标签栏的矩形；落点窗口收到 `tab-drag-over` 显示插入位置，并入时按松手的横坐标算出位置。
- **关闭与退出**：窗口的最后一个标签关闭或移走时，如果还有其他窗口，这个窗口随之关闭；只剩一个窗口时留下空窗口。关空窗口由后端判断（`window_close_empty`）：有待取的标签就不关，关了就不再接收标签（`window_send_tab` 报 `window.notFound`），前端正在取标签时也不去关。关掉最后一个窗口即退出。⌘Q 由有焦点的窗口处理：后端向每个窗口询问需要确认的标签数（`busy-query`），合计后确认一次再退出；超时未答的窗口（页面还在加载）按它拥有的会话数计，宁可多问。

### 状态栏

- **内容**：`StatusBar` 显示当前窗格的连接、上下行指示灯、终端类型、尺寸与光标，可在设置里关闭。
- **数据来源**：变化少的（终端类型、重连时间）在 tab store；变化频繁的（收发计数、光标）不进 store，状态栏直接订阅并按帧更新（`lib/traffic.ts`、`TerminalView`）。计数只算终端数据，不含 SFTP 与转发。
- **无障碍**：状态栏不是 live region（光标一动就会播报），各项的读屏名称是完整句子。

## 平台注意事项

- macOS 上 CJK 输入法要在真机上测。处理 Enter / Esc 前先用 `lib/platform.ts` 的 `isComposing` 判断是否在组字。
- macOS 的"按住按键显示重音字符"被关掉（与 Terminal.app 一致），长按会重复；用户可用 `defaults write` 恢复（`lib.rs`）。
- 中文 / emoji 宽度依赖 unicode11 插件。
- Windows 的 S 模式不允许启动命令行程序，这时隐藏本地终端的入口，其他功能不受影响。
- ConPTY 需要 Windows 10 1809 及以上。Windows PowerShell 5.1 调用的原生程序可能按 OEM 代码页输出导致中文乱码，暂不修改用户的 `[Console]::OutputEncoding`（已知限制）。
- ssh-agent：macOS 与 Linux 用 `SSH_AUTH_SOCK`；Windows 用 OpenSSH 命名管道或 Pageant。
- Linux 第一阶段只针对 GNOME + Wayland（Debian、Fedora、Ubuntu 的默认桌面）：闲置检测、窗口按钮的样式和窗口菜单按 GNOME 做，其他桌面可能缺少其中一些。Wayland 不让应用知道窗口位置和全局指针坐标，按住按键时别的窗口也收不到鼠标事件，所以标签拖出窗口（由页面判断指针已在窗口外）后改为 GTK 拖放（只有本应用接受的类型），各窗口的 webview 接收它：经过另一个窗口的标签栏时显示插入位置，放下即并入；没有窗口接收时开新窗口，位置由合成器决定（`tab_drag/linux.rs`）。
- 终端的默认字体：Linux 上先用桌面设置的等宽字体（GNOME 的 `monospace-font-name`，经设置 portal 读取，和 GNOME Terminal 一样），再用内置列表；内置列表里 macOS、Windows 的字体在 Linux 上都没有。
- Linux 上串口设备通常属于 `dialout`（Fedora 是 `dialout` 或 `uucp`）组，用户要加入该组才能打开。

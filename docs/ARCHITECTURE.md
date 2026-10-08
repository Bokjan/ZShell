# ZShell 架构

个人使用的跨平台（macOS / Windows）SSH 客户端。本文只记录跨模块的设计决策；模块内的细节（边界情况、库的坑）写在代码注释里。

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
| 前端 | Vite + React + TypeScript |
| 终端渲染 | `@xterm/xterm` + fit / webgl / unicode11 / web-links / search 插件 |

## 总体结构

```
┌──────────── 前端 (React + xterm.js) ────────────┐
│ 会话列表 │ 终端标签页 │ SFTP 面板 │ 端口转发面板 │
└───────────────┬──────────────▲─────────────────┘
          invoke (输入/命令)    │ Channel (输出字节流/事件/进度)
┌───────────────▼──────────────┴─────────────────┐
│ SessionManager: 会话任务（SSH shell / 本地 PTY） │
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
| `ssh/` | 连接与 shell（`mod.rs`）、russh 回调（`handler.rs`）、主机密钥确认（`host_key.rs`）、认证（`auth.rs`）、连接注册表（`connections.rs`） |
| `sftp/` | 目录浏览与文件操作（`mod.rs`）、递归上传下载与进度（`transfer.rs`） |
| `forward/` | 转发规则运行管理（`mod.rs`）、`-L` / `-R` / `-D`（`local.rs` / `remote.rs` / `dynamic.rs`）、SOCKS（`socks.rs`） |
| `pty/` | 本地终端（`mod.rs`）、默认 shell 与环境变量（`shell.rs`） |
| `zmodem/` | rz / sz：检测与会话接管（`mod.rs`）、帧格式与 CRC（`frame.rs`）、收发字节流（`link.rs`）、接收（`receive.rs`）、发送（`send.rs`） |
| `config.rs` / `settings.rs` / `secrets.rs` | 会话配置 `profiles.json`、应用设置 `settings.json`、钥匙串 |
| `import.rs` / `backup.rs` | ssh_config 导入；会话导出与导入 |
| `i18n.rs` / `error.rs` | 后端消息目录、结构化错误（见 [I18N.md](I18N.md)） |
| `commands.rs` | Tauri 命令入口 |

前端（`src/`）：`App.tsx` 持有标签页状态；`components/` 下每个面板一个组件；`lib/api.ts` 封装全部后端命令。

## 设计决策

### 会话与数据流

- **会话是一个后台任务**：`SessionManager` 为每个会话创建 `TermIo`（输出 Channel、事件 Channel、输入队列），后端任务（SSH shell 或本地 PTY）持有它运行；关闭会话即中止任务。新的会话后端只需实现同样的任务形态。
- **终端输出不走 event**：经 Tauri 2 `Channel` 以 `InvokeResponseBody::Raw` 推送，前端直接 `term.write(Uint8Array)`，避免 JSON 序列化开销。输入走 `invoke`，尺寸变化发 `window-change`。
- **输出流控**：前端在 `term.write` 回调里累计已处理字节，每 64 KiB 调一次 `session_ack`；后端未确认超过 2 MiB 时暂停读取，降到 512 KiB 以下再继续。目前只有本地 PTY 据此暂停（SSH 有网络限速，只计数）。
- **SSH 读写并行**：写入等待服务器窗口时仍持续读取输出。russh 在连接任务里把输出送入有界队列，队列满会卡住连接任务，窗口调整也就收不到，形成死锁（大段粘贴、ZMODEM 上传时远端还在输出）。写入未完成时不取新的输入，以此形成背压。本地终端的输入队列同样有界（写线程按 shell 的读取速度消费）。
- **会话事件**：`Connected`、`Forward`（转发规则状态）、`Zmodem`（见下文）、`Closed { reason, status, error }`。`reason` 为 `exited`（shell 退出）、`lost`（已建立的连接断开）、`failed`（连接、认证或启动阶段失败），前端据此决定重连、自动关闭标签或提示。

### SSH

- **一个连接多路复用**：认证完成后连接登记到 `Connections`；SFTP 首次使用时在同一连接上开 subsystem channel 并缓存，端口转发也开在同一连接上。
- **复制标签**：已连接的 SSH 标签复制时，新标签用 `ssh_open_shared` 在源标签的连接上开一个新的 shell channel，不重新连接和认证（类似 OpenSSH 的 ControlMaster），也不重复自动启动转发。`Connections` 按会话 id 登记，多个会话可以指向同一连接，最后一个使用它的会话结束时才断开。转发状态推送给连接上所有会话的标签，后加入的标签先收到各规则的当前状态。源连接已断开时按正常流程新建连接；复制出的标签断线重连也总是新建连接。
- **提示都在终端里**：主机指纹确认、密码、私钥口令、keyboard-interactive 问题都通过 `TermIo::read_line` 在终端里询问，与 OpenSSH 一致，不弹窗。russh 在自己的任务里回调 `check_server_key`，handler 通过 mpsc + oneshot 把问题转交给会话任务。
- **认证**："自动"（默认）按 OpenSSH 的顺序尝试 agent 中的密钥 → `~/.ssh/id_ed25519`、`id_ecdsa`、`id_rsa` → keyboard-interactive / 密码（可用钥匙串中的密码）。加密的私钥只在服务器接受其公钥后才询问口令（细节见 `ssh/auth.rs`）。
- **主机校验**：读写 `~/.ssh/known_hosts`，首次连接确认指纹，指纹变化时显式告警。
- **ProxyJump**：会话的 `jumpHosts` 按顺序引用其他会话，每一跳用被引用会话的地址和认证，但不展开它自己的跳板机。在上一跳连接上开 `direct-tcpip` channel，以其 `ChannelStream` 作为下一跳的传输层；主机密钥按每一跳自己的 host:port 校验，各跳的提示都在同一个终端里。被引用的会话不能删除。
- **keepalive 与重连**：会话配置 `keepaliveInterval`（默认 30 秒，连续 3 次无响应判定断线）与 `autoReconnect`（默认开）。`lost` 时前端按 2、4、8、16、30 秒退避重连，一直重试到标签页关闭；认证错误和主机密钥被拒绝时停止。Enter 立即重连，Ctrl+C 取消，`online` 事件立即重试。重连是新会话：SFTP 面板回到原目录，自动启动的转发规则重新启动。

### 文件传输与端口转发

- **SFTP 传输**：先扫描生成计划（目录、文件、总字节数），再逐个文件复制；进度经 Channel 每 100 ms 推送一次；取消通过共享的 `AtomicBool`，未完成的文件会被删除。
- **端口转发**：规则保存在会话配置的 `forwards` 中，只能通过 `profile_set_forwards` 修改（`profile_save` 保留原有规则）。每条运行中的规则是一个任务，持有自己的监听器和承载的连接，停止规则即中止任务。状态（Starting / Active / Failed / Stopped）经会话事件推送；每次启动分配一个代号（generation），被替换的旧任务不能覆盖新状态。`-R` 的 `forwarded-tcpip` channel 由 `ClientHandler` 按路由表转交给规则任务，不阻塞 russh 的连接任务。

### ZMODEM

- **位置**：在会话层而不是某个后端：SSH 与本地终端（包括在本地终端里 ssh 登录后）都适用。协议自己实现（帧、CRC、转义、收发状态机），以 lrzsz 为对照测试。
- **检测与接管**：后端把远端输出交给 `SessionSink::output`，在其中查找 `sz` 的 ZRQINIT / `rz` 的 ZRINIT 十六进制头（`**\x18B00` / `**\x18B01`，可跨两次输出）。识别后到传输结束：输出交给传输任务而不再写入终端；用户按键被丢弃，Ctrl+C 取消；传输任务要发送的数据经 `TermIo::recv` 注入，与键盘输入走同一条路到后端，并有背压。开头的头部在 2 秒内没有完整到达（例如 `cat` 了一个二进制文件），即视为误判，回到普通输出。结束后剩余的输出（shell 提示符）交还终端。
- **与前端交互**：经 `Zmodem { phase }` 会话事件。`sz` 时先问保存位置（`zmodem_save_to`，默认"下载"文件夹，可在设置中改为每次询问）；`rz` 时问要发送的文件（`zmodem_send_files`）：终端上方显示提示条并立即弹出文件选择框，选择框取消后提示条仍在，可以拖入文件、重新选择或取消；`zmodem_cancel` 随时取消。进度（文件名、已传 / 总量、速度）和每个文件的结果由后端写在终端里。
- **兼容性**：发送时转义全部控制字符（相当于 `rz -e`），接收时也在 ZRINIT 里要求对方这样做，以通过会过滤控制字符的堡垒机；使用对方支持的 32 位 CRC；对方不支持全双工时逐包确认。下载的同名文件按 SFTP 的规则改名，保留修改时间。
- **取消与出错**：向对方发送取消序列（连续 CAN），删除未完成的本地文件，在终端里提示。下载被取消时，对方已发出的数据还在路上，先丢弃一段时间，只保留最后一个 CAN 之后的输出（对方退出后的 shell 提示符）。对方超时无响应时按 lrzsz 的习惯重发（每次 10 秒，最多 5 次）后放弃。

### 本地终端

- 作为 `SessionManager` 的另一种后端（`local_open`），输入、resize、关闭沿用 SSH 会话的命令。
- **前台进程**：`session_foreground` 报告 shell 之外正在前台运行的程序名，用于关闭标签前的确认。unix 比较终端的前台进程组（`tcgetpgrp`）与 shell 的 pid，后台作业不算；Windows 没有前台进程组，以 shell 是否有子进程判断。
- **默认 shell**：macOS 以登录 shell 方式启动用户的 shell，让 `/etc/zprofile`、`~/.zprofile` 补全从 Finder 启动时缺失的 PATH；Windows 优先 PowerShell 7，否则用系统自带的 Windows PowerShell。
- **环境变量**：`TERM=xterm-256color`（仅 unix）、`COLORTERM=truecolor`、`TERM_PROGRAM=ZShell`；macOS 上没有区域设置时按系统语言生成 UTF-8 的 `LANG`。
- **结束**：以子进程退出为准，而非输出 EOF（后台作业可能一直占着终端）。shell 以 0 退出时自动关闭标签页（同 Terminal.app），否则保留，按 Enter 重启。关闭标签页时 unix 发 SIGHUP、Windows 关闭 pseudoconsole，2 秒后仍未退出则强制结束。

### 配置与设置

- **存储**：配置目录（macOS `~/Library/Application Support/org.boyin.zshell/`，Windows `%APPDATA%\org.boyin.zshell\`）下的 `profiles.json`、`folders.json` 与 `settings.json`；密码与口令只存系统钥匙串，服务名为 bundle identifier `org.boyin.zshell`。
- **设置**：分为 `appearance`、`terminal`、`tabs` 三组；后端校验并夹取数值，文件损坏时回退默认值。前端 `SettingsProvider` 启动时读取，修改即时生效并保存，较旧的保存结果不会覆盖较新的修改。
- **主题**：`<html data-theme>` 选择 CSS 变量组，样式中不写死颜色；原生窗口用 `setTheme` 同步原生菜单与对话框（Windows 上还决定 WebView2 的 `prefers-color-scheme`），用 `setBackgroundColor` 同步调整大小时露出的背景；首帧背景由 `index.html` 的内联样式按系统外观给出，避免闪烁。终端配色、字体等通过 `term.options` 应用到所有已打开的终端。
- **会话与文件夹**：会话的 `folder` 字段指向所在文件夹，文件夹存在 `folders.json`（`parent` 可嵌套）。`profiles.json` 仍是数组，旧版本照常读取。文件中的顺序即显示顺序，每个文件夹里先列子文件夹、再列会话；拖拽只有一个后端操作 `tree_move`（放进某文件夹、排在某项之前或末尾）。删除文件夹时其中的内容移到上一级，不删除会话。加载时修正指向不存在文件夹的引用和循环。文件夹折叠状态与最近连接是本机的界面状态，与侧栏宽度一样存在 localStorage。
- **导出 / 导入**：导出为 JSON（`format: "zshell-sessions"`，含文件夹与会话，不含密码）。导入时同名、否则同地址（用户、主机、端口）的会话视为已存在，不再导入；被选中会话的跳板机一并导入，已存在的则引用现有会话；文件夹按名称路径合并；导入的会话一律分配新 id。
- **快速连接**：标签目标 `quick`（`ssh_quick_open`）不需要已保存的会话，用"自动"认证，未写用户名时用本机用户名（同 `ssh host`）。"另存为会话"把标签目标改为新会话但不重连（`TerminalView` 只在本地与 SSH 之间切换时重建终端），下次连接起使用会话的设置。
- **ssh_config 导入**：一次性复制，导入后与 config 文件无关联。与已有会话同名或同地址的主机不再导入，不支持的选项（ProxyCommand、ForwardAgent 等）在列表中标出。

## 交互约定

- **快捷键**：应用快捷键在 window 的捕获阶段拦截，终端收不到。Windows 上与 shell 冲突的快捷键加 Shift（Ctrl+F、Ctrl+W 等留给 shell）。剪贴板快捷键只在终端获得焦点时生效（xterm.js 的 `attachCustomKeyEventHandler`），输入框里仍是普通的复制粘贴。

  | 功能 | macOS | Windows |
  |---|---|---|
  | 复制 | ⌘C（原生菜单项） | Ctrl+Shift+C、Ctrl+Insert；有选区时 Ctrl+C（复制后清除选区，无选区时仍发 `^C`） |
  | 粘贴 | ⌘V（原生菜单项） | Ctrl+Shift+V、Ctrl+V、Shift+Insert |
  | 切换标签 | Ctrl+Tab / Ctrl+Shift+Tab | 同左 |
  | 跳到第 1–8 个 / 最后一个标签 | ⌘1–8 / ⌘9 | Alt+1–8 / Alt+9 |
  | 新建本地终端 | ⌘T | Ctrl+Shift+T |
  | 关闭标签 | ⌘W（原生菜单项） | Ctrl+Shift+W |
  | 关闭窗口 | ⇧⌘W（原生菜单项） | — |
  | 搜索会话 | ⌘K | Ctrl+Shift+K |
  | 设置 | ⌘,（原生菜单项） | Ctrl+, |
  | 终端搜索 | ⌘F | Ctrl+Shift+F |
  | 撰写栏 | ⇧⌘I | Ctrl+Shift+I |
  | SFTP 面板 | ⇧⌘E | Ctrl+Shift+E |
  | 端口转发面板 | ⇧⌘P | Ctrl+Shift+P |

- **菜单**：macOS 保留原生菜单栏（WKWebView 的 ⌘C / ⌘V / ⌘A / ⌘Q、⌘, 都依赖原生菜单项），由 `lib.rs` 显式构建：去掉预置的 "Close Window"（它占用 ⌘W），File 菜单改为 Close Tab（⌘W，交给前端，可能先确认；没有标签时关闭窗口）、Close Window（⇧⌘W），与 Terminal.app 一致；应用菜单的 Quit（⌘Q）也换成自定义菜单项，关闭窗口而不是直接退出，以便前端先确认。Windows 不显示菜单栏。
- **右键菜单**：终端和标签的右键菜单是页面内绘制的 `ContextMenu`（跟随主题，两个平台一致），不用原生菜单。WebView 自带的右键菜单（重新载入、检查元素等）全局屏蔽，只有文本输入框保留系统的编辑菜单。
- **滚动条**：终端的滚动条由 xterm.js 自绘，颜色取配色的前景色。其余区域用系统滚动条，`color-scheme` 让它们跟随深浅色：macOS 是系统的悬浮滚动条；Windows 通过窗口配置 `scrollBarStyle: fluentOverlay` 使用 WebView2 的 Fluent 悬浮滚动条（需要 WebView2 Runtime 125 及以上，否则为默认的经典样式）。不用 `::-webkit-scrollbar` 统一自绘，以免 macOS 失去原生悬浮滚动条。标签栏放不下时横向滚动但不显示滚动条（滚轮、触控板，当前标签自动滚入视野），"文件 / 转发"切换按钮固定在右侧。
- **入口**：本地终端在标签栏末尾的 "+"；设置在侧栏底部和 macOS 应用菜单中；新建会话、新建文件夹、导入导出在侧栏顶行。
- **会话列表**（Xshell 风格）：单击选中，双击或 Enter 在新标签中打开；单击文件夹折叠 / 展开，双击打开其中全部会话（超过 5 个先确认）。右键"连接"在该会话已有标签时切换过去，"在新标签中连接"总是新开。键盘：↑↓ 移动，←→ 折叠展开（← 在会话上回到所在文件夹）。新建会话 / 文件夹放进当前选中的文件夹。搜索框按名称、地址、用户名过滤（多个词同时满足），↑↓ 与 Enter 打开；没有匹配且输入像地址（`[user@]host[:port]`，含 `@`、`.` 或端口）时提供快速连接。

### 标题栏

- **布局**：没有原生标题栏，窗口顶部一行（`--titlebar-height`，34px）在侧栏右边界处分为侧栏顶行与标签栏，侧栏从窗口顶部延伸到底部。标签栏始终显示，没有标签页时也有 "+" 和拖动区域。
- **窗口**：主窗口在配置中 `create: false`，由 `window.rs` 按平台创建：macOS 用 `titleBarStyle: Overlay` 并隐藏标题，红绿灯（`trafficLightPosition`）垂直居中于顶行，侧栏顶行左侧为其留白（全屏时取消）；Windows `decorations: false`，阴影与圆角由 Tauri 的无边框阴影提供，顶边缩放由 Tauri 盖在 WebView2 上的子窗口处理。其他平台保留原生标题栏。
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

- **标题**：重命名的标题 > 远端标题（OSC 0 / 2，可在设置中关闭）> 会话名。重命名为空则恢复自动标题；重连时清除旧的远端标题，由新 shell 重新设置。
- **拖拽排序**：用鼠标事件而不是 HTML5 拖放（Windows 上 Tauri 的文件拖入会拦截 HTML5 拖放）；指针越过相邻标签的中点即交换位置。按下标签不夺走终端的焦点。
- **关闭确认**（可关闭）：已连接的 SSH 标签，或本地终端里有前台程序时确认；关闭多个标签时合并为一次确认。对话框里可勾选"不再询问"，即关闭该设置。关闭窗口（即退出：关闭按钮、⌘Q、Alt+F4）会一次结束全部会话，有这样的标签时总是确认，不受该设置影响，也没有"不再询问"；由前端的 `onCloseRequested` 拦截。macOS 上从程序坞退出、注销或关机由系统直接终止应用，不经过确认（已知限制）。
- **重新连接**：标签右键菜单中，关闭当前会话后立即新建连接；旧会话迟到的输出和事件按连接代号忽略。
- **撰写栏与同步输入**：纯前端实现，按会话 id 直接 `session_write`。发送范围只在撰写栏打开时生效（`lib/compose.ts` 的 `scopeTabs`），关闭后一律只发当前标签，关闭时同步也随之关闭，避免范围停在"所有标签"而不自知。同步转发的是终端 `onData` 中用户的输入（键入与粘贴），过滤掉鼠标与焦点报告，只从当前标签转发。范围超出当前标签时标签顶部有标记（同步时为警示色），收到内容的标签闪烁；同步时当前终端四周有警示色边框（画在终端画布之上的一层，`outline` 会被 WebGL 画布遮住）。撰写栏历史只保存在内存中。
- **终端内操作**：连接结束后按 Enter 重连 / 重试 / 重启 shell，提示以暗色文字写在终端里。

## 平台注意事项

- macOS 上 xterm.js 运行在 WKWebView 里，CJK 输入法（候选框位置、组字过程）需在真机上测试。
- macOS 默认的"按住按键显示重音字符"会让 WKWebView 里长按字母键不重复。应用启动时以注册默认值把 `ApplePressAndHoldEnabled` 设为关闭（与 Terminal.app / iTerm2 一致），用户仍可用 `defaults write org.boyin.zshell ApplePressAndHoldEnabled -bool true` 恢复。
- 中文 / emoji 宽度依赖 unicode11 插件，远端 `LANG` 需为 UTF-8。
- ConPTY 需要 Windows 10 1809 及以上。Windows PowerShell 5.1 调用的原生程序可能按 OEM 代码页输出导致中文乱码，暂不修改用户的 `[Console]::OutputEncoding`（已知限制）。
- ssh-agent：macOS 用 `SSH_AUTH_SOCK`；Windows 用 OpenSSH 命名管道或 Pageant。

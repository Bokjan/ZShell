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
| `config.rs` / `settings.rs` / `secrets.rs` | 会话配置 `profiles.json`、应用设置 `settings.json`、钥匙串 |
| `import.rs` | ssh_config 导入 |
| `i18n.rs` / `error.rs` | 后端消息目录、结构化错误（见 [I18N.md](I18N.md)） |
| `commands.rs` | Tauri 命令入口 |

前端（`src/`）：`App.tsx` 持有标签页状态；`components/` 下每个面板一个组件；`lib/api.ts` 封装全部后端命令。

## 设计决策

### 会话与数据流

- **会话是一个后台任务**：`SessionManager` 为每个会话创建 `TermIo`（输出 Channel、事件 Channel、输入队列），后端任务（SSH shell 或本地 PTY）持有它运行；关闭会话即中止任务。新的会话后端只需实现同样的任务形态。
- **终端输出不走 event**：经 Tauri 2 `Channel` 以 `InvokeResponseBody::Raw` 推送，前端直接 `term.write(Uint8Array)`，避免 JSON 序列化开销。输入走 `invoke`，尺寸变化发 `window-change`。
- **输出流控**：前端在 `term.write` 回调里累计已处理字节，每 64 KiB 调一次 `session_ack`；后端未确认超过 2 MiB 时暂停读取，降到 512 KiB 以下再继续。目前只有本地 PTY 据此暂停（SSH 有网络限速，只计数）。
- **会话事件**：`Connected`、`Forward`（转发规则状态）、`Closed { reason, status, error }`。`reason` 为 `exited`（shell 退出）、`lost`（已建立的连接断开）、`failed`（连接、认证或启动阶段失败），前端据此决定重连、自动关闭标签或提示。

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

### 本地终端

- 作为 `SessionManager` 的另一种后端（`local_open`），输入、resize、关闭沿用 SSH 会话的命令。
- **前台进程**：`session_foreground` 报告 shell 之外正在前台运行的程序名，用于关闭标签前的确认。unix 比较终端的前台进程组（`tcgetpgrp`）与 shell 的 pid，后台作业不算；Windows 没有前台进程组，以 shell 是否有子进程判断。
- **默认 shell**：macOS 以登录 shell 方式启动用户的 shell，让 `/etc/zprofile`、`~/.zprofile` 补全从 Finder 启动时缺失的 PATH；Windows 优先 PowerShell 7，否则用系统自带的 Windows PowerShell。
- **环境变量**：`TERM=xterm-256color`（仅 unix）、`COLORTERM=truecolor`、`TERM_PROGRAM=ZShell`；macOS 上没有区域设置时按系统语言生成 UTF-8 的 `LANG`。
- **结束**：以子进程退出为准，而非输出 EOF（后台作业可能一直占着终端）。shell 以 0 退出时自动关闭标签页（同 Terminal.app），否则保留，按 Enter 重启。关闭标签页时 unix 发 SIGHUP、Windows 关闭 pseudoconsole，2 秒后仍未退出则强制结束。

### 配置与设置

- **存储**：配置目录（macOS `~/Library/Application Support/org.boyin.zshell/`，Windows `%APPDATA%\org.boyin.zshell\`）下的 `profiles.json` 与 `settings.json`；密码与口令只存系统钥匙串，服务名为 bundle identifier `org.boyin.zshell`。
- **设置**：分为 `appearance`、`terminal`、`tabs` 三组；后端校验并夹取数值，文件损坏时回退默认值。前端 `SettingsProvider` 启动时读取，修改即时生效并保存，较旧的保存结果不会覆盖较新的修改。
- **主题**：`<html data-theme>` 选择 CSS 变量组，样式中不写死颜色；原生窗口用 `setTheme` 同步标题栏，用 `setBackgroundColor` 同步调整大小时露出的背景；首帧背景由 `index.html` 的内联样式按系统外观给出，避免闪烁。终端配色、字体等通过 `term.options` 应用到所有已打开的终端。
- **ssh_config 导入**：一次性复制，导入后与 config 文件无关联。与已有会话同名或同地址的主机不再导入，不支持的选项（ProxyCommand、ForwardAgent 等）在列表中标出。

## 交互约定

- **快捷键**：应用快捷键在 window 的捕获阶段拦截，终端收不到。Windows 上与 shell 冲突的快捷键加 Shift（Ctrl+F、Ctrl+W 等留给 shell）。剪贴板快捷键只在终端获得焦点时生效（xterm.js 的 `attachCustomKeyEventHandler`），输入框里仍是普通的复制粘贴。

  | 功能 | macOS | Windows |
  |---|---|---|
  | 复制 | ⌘C（原生菜单项） | Ctrl+Shift+C、Ctrl+Insert；有选区时 Ctrl+C（复制后清除选区，无选区时仍发 `^C`） |
  | 粘贴 | ⌘V（原生菜单项） | Ctrl+Shift+V、Ctrl+V、Shift+Insert |
  | 切换标签 | Ctrl+Tab / Ctrl+Shift+Tab | 同左 |
  | 跳到第 1–8 个 / 最后一个标签 | ⌘1–8 / ⌘9 | Alt+1–8 / Alt+9 |
  | 关闭标签 | ⌘W（原生菜单项） | Ctrl+Shift+W |
  | 关闭窗口 | ⇧⌘W（原生菜单项） | — |
  | 设置 | ⌘,（原生菜单项） | Ctrl+, |
  | 终端搜索 | ⌘F | Ctrl+Shift+F |
  | SFTP 面板 | ⇧⌘E | Ctrl+Shift+E |
  | 端口转发面板 | ⇧⌘P | Ctrl+Shift+P |

- **菜单**：macOS 保留原生菜单栏（WKWebView 的 ⌘C / ⌘V / ⌘A / ⌘Q、⌘, 都依赖原生菜单项），由 `lib.rs` 显式构建：去掉预置的 "Close Window"（它占用 ⌘W），File 菜单改为 New Local Terminal、Close Tab（⌘W，交给前端，可能先确认；没有标签时关闭窗口）、Close Window（⇧⌘W），与 Terminal.app 一致。Windows 不显示菜单栏。
- **右键菜单**：终端和标签的右键菜单是页面内绘制的 `ContextMenu`（跟随主题，两个平台一致），不用原生菜单。
- **入口**：本地终端在侧栏标题栏的按钮和 macOS File 菜单中；设置在侧栏底部和 macOS 应用菜单中。

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
- **关闭确认**（可关闭）：已连接的 SSH 标签，或本地终端里有前台程序时确认；关闭多个标签时合并为一次确认。对话框里可勾选"不再询问"，即关闭该设置。
- **重新连接**：标签右键菜单中，关闭当前会话后立即新建连接；旧会话迟到的输出和事件按连接代号忽略。
- **终端内操作**：连接结束后按 Enter 重连 / 重试 / 重启 shell，提示以暗色文字写在终端里。

## 平台注意事项

- macOS 上 xterm.js 运行在 WKWebView 里，CJK 输入法（候选框位置、组字过程）需在真机上测试。
- macOS 默认的"按住按键显示重音字符"会让 WKWebView 里长按字母键不重复。应用启动时以注册默认值把 `ApplePressAndHoldEnabled` 设为关闭（与 Terminal.app / iTerm2 一致），用户仍可用 `defaults write org.boyin.zshell ApplePressAndHoldEnabled -bool true` 恢复。
- 中文 / emoji 宽度依赖 unicode11 插件，远端 `LANG` 需为 UTF-8。
- ConPTY 需要 Windows 10 1809 及以上。Windows PowerShell 5.1 调用的原生程序可能按 OEM 代码页输出导致中文乱码，暂不修改用户的 `[Console]::OutputEncoding`（已知限制）。
- ssh-agent：macOS 用 `SSH_AUTH_SOCK`；Windows 用 OpenSSH 命名管道或 Pageant。

# ZShell 架构

个人使用的跨平台（macOS / Windows）SSH 客户端。

## 语言约定

- `docs/` 目录下的文档使用中文；除此之外的一切（代码、注释、界面文案、终端提示、错误信息、README、提交信息）使用英语。
- 应用的默认语言（源语言）为英语，其他语言通过 i18n 提供，见下文「国际化」。

## 技术栈

| 层 | 选型 |
|---|---|
| 应用框架 | Tauri 2 |
| 后端 | Rust + tokio |
| SSH / SFTP | `russh` / `russh-sftp` |
| 凭据 | `keyring`（macOS Keychain / Windows Credential Manager） |
| ssh_config 导入 | `ssh2-config` |
| 本地终端 | `portable-pty`（Windows 走 ConPTY） |
| 前端 | Vite + React + TypeScript |
| 终端渲染 | `@xterm/xterm` + fit / webgl / unicode11 / web-links 插件 |

## 总体结构

```
┌──────────── 前端 (React + xterm.js) ────────────┐
│ 会话列表 │ 终端标签页 │ SFTP 面板 │ 端口转发面板 │
└───────────────┬──────────────▲─────────────────┘
          invoke (输入/命令)    │ Channel (输出字节流/进度)
┌───────────────▼──────────────┴─────────────────┐
│ ConnectionManager: HashMap<ConnId, Connection>  │
│   一个 SSH 连接 ──┬─ shell channel × N (标签页)  │
│                   ├─ sftp subsystem channel     │
│                   └─ direct-tcpip / forwarded   │
│ auth / known_hosts / secrets / config / pty     │
└─────────────────────────────────────────────────┘
```

### 设计原则

- **一个连接多路复用**：同一主机的终端、SFTP、端口转发共用一条 SSH 连接，各开 channel。
- **终端数据流**：远端输出经 Tauri 2 `Channel` 以 `InvokeResponseBody::Raw` 推送，前端收到
  `ArrayBuffer` 后直接 `term.write(Uint8Array)`；不走 event 系统（JSON 序列化开销大）。
  键盘输入走 `invoke`，尺寸变化发 `window-change`。
- **会话抽象**：`session::SessionManager` 为每个会话创建 `TermIo`（输出 Channel、事件 Channel、
  输入队列），后端任务（SSH shell / 本地 PTY）持有它运行；关闭会话 = 中止该任务。
- **终端内提示**：主机指纹确认、密码、私钥口令、keyboard-interactive 问题都直接在终端里询问
  （`TermIo::read_line`），与 OpenSSH 体验一致，无需额外弹窗。russh 在自己的任务里回调
  `check_server_key`，因此 handler 通过 mpsc + oneshot 把问题转交给会话任务。
- **认证**：自动（默认）、密码、私钥文件（含口令）、ssh-agent（macOS `SSH_AUTH_SOCK`；Windows OpenSSH 命名管道 / Pageant）、keyboard-interactive（2FA）。
  - "自动"按 OpenSSH 的默认顺序尝试：agent 中的密钥 → `~/.ssh/id_ed25519`、`id_ecdsa`、`id_rsa` → keyboard-interactive / 密码（可使用钥匙串中保存的密码）。
  - 加密的默认私钥只有在服务器接受其公钥后才询问口令：用 russh 的 `authenticate_publickey_with` 先发探测请求，收到 PK_OK 后才调用自定义 `Signer`，此时在终端询问口令、解密并签名。签名器不能返回错误（russh 会一直等待签名，连接卡死），口令错误或取消时返回格式正确但无效的签名，服务器拒绝该密钥后继续尝试下一种方式；取消则随后中止认证。
- **主机校验**：读写 `~/.ssh/known_hosts`，首次连接确认指纹，指纹变化时显式告警。
- **连接注册表**：`ssh::Connections` 以会话 id 登记认证完成的连接（`Arc<Handle>`），SFTP 首次使用时在该连接上开 `sftp` subsystem channel 并缓存；shell 结束或标签页关闭时统一断开。
- **传输**：上传/下载先扫描生成计划（目录 + 文件 + 总字节数），再逐个文件复制（256 KiB 缓冲，russh-sftp 内部并发读写请求），进度经 Channel 每 100 ms 推送一次；取消通过共享的 `AtomicBool`，未完成的文件会被删除。下载到"下载"文件夹时顶层重名自动改为 `name (1).ext`。
- **端口转发**：规则（`-L` / `-R` / `-D`）保存在会话配置的 `forwards` 中，只能通过 `profile_set_forwards` 修改，`profile_save` 会保留原有规则。认证完成后自动启动勾选了"自动启动"的规则，失败时在终端打印一行黄色提示。每条运行中的规则是一个任务，持有自己的监听器（`-R` 则是 `RemoteRoutes` 中的路由）和一个 `JoinSet`（承载的所有连接），停止规则即中止任务，已建立的连接随之断开。
  - `-L`：本地监听，每个连接开一个 `direct-tcpip` channel 后双向拷贝。
  - `-D`：本地 SOCKS 服务端（SOCKS5 无认证 CONNECT，兼容 SOCKS4/4a），目标地址交给服务器解析；channel 打开成功后才回复成功，失败时按原因回复。
  - `-R`：`tcpip-forward` 请求服务器监听（端口 0 由服务器分配）。服务器为每个连接开 `forwarded-tcpip` channel，`ClientHandler::server_channel_open_forwarded_tcpip` 按 (地址, 端口) 查路由（找不到再只按端口匹配），通过 mpsc 交给规则任务，不阻塞 russh 的连接任务；规则任务先连本地目标，连上才 accept，否则以 `ConnectFailed` 拒绝（与 OpenSSH 一致）。任务结束时注销路由并发送 `cancel-tcpip-forward`。
  - 状态（Starting / Active{实际监听地址, 连接数, 最近一次连接错误} / Failed / Stopped）通过会话事件 Channel 以 `SessionEvent::Forward` 推送，前端按标签页累积，会话关闭时清空。每次启动分配一个代号（generation），只有当前代号的任务能上报状态，避免被替换的旧任务覆盖新状态。
  - 重启规则（编辑运行中的规则）时，新任务先等待旧任务结束、并等待旧 `-R` 的 cancel 请求完成，再重新监听，避免同一端口重启时"地址已被占用"。
  - 前端：标签栏右侧的分段选择器（Files | Forwards）切换侧面板，再次点击当前段关闭面板；快捷键 ⇧⌘E / ⇧⌘P（Windows 为 Ctrl+Shift+E / P），在捕获阶段拦截，终端收不到。Forwards 段上的徽标显示运行中的规则数，有失败时显示红点。新增规则在已连接时立即启动；编辑运行中的规则会以新定义重启。
- **keepalive 与断线重连**：每个会话配置有 `keepaliveInterval`（秒，默认 30，0 关闭；连续 3 次无响应判定断线）和 `autoReconnect`（默认开）。会话结束时 `SessionEvent::Closed` 带 `reason`：`exited`（远端发了 exit-status 或关闭 channel）、`lost`（已建立的连接断开）、`failed`（连接、认证或启动 shell 阶段失败），以及结构化错误。断线原因由 `ClientHandler::disconnected` 记录（如 `Keepalive timeout`、`early eof`），作为错误的技术细节显示。前端在 `lost` 时按 2、4、8、16、30 秒退避自动重连（30 秒封顶，一直重试到标签页关闭）；重试期间的 `failed` 继续退避，但认证类错误（`auth.*`）和主机密钥被拒绝时停止；Enter 立即重连，Ctrl+C 取消，`online` 事件（如睡眠唤醒后网络恢复）立即重试。重连是新会话：SFTP 面板回到原目录，自动启动的转发规则重新启动。
- **终端搜索**：`@xterm/addon-search`，终端右上角的浮动搜索条（区分大小写 / 整词 / 正则、上一个 / 下一个、"3 of 17" 计数，高亮上限 1000 条）。快捷键 macOS ⌘F，Windows Ctrl+Shift+F（Ctrl+F 留给 shell）。注意 addon-search 0.16 只切换选项时不会重新高亮（先保存新选项再比较），搜索前先调用 `clearDecorations()` 清掉缓存的搜索词来绕过。WebKit 点击按钮不会让按钮获得焦点，搜索条在 mousedown 时阻止默认行为，让焦点留在输入框。
- **ProxyJump**：会话配置的 `jumpHosts` 按顺序引用其他会话（相当于 `ProxyJump a,b`），每一跳使用被引用会话的地址和认证方式，但不展开它自己的跳板机。连接时逐跳建立：在上一跳的连接上开 `direct-tcpip` channel 到下一跳，以其 `ChannelStream` 作为下一跳 SSH 握手的传输层；主机密钥按每一跳自己的 host:port 校验，各跳的提示都在同一个终端里。`Connection` 持有各跳板机连接，关闭时从目标往回逐个断开。保存时校验引用存在、不引用自身、不重复；被其他会话用作跳板机的会话不能删除。
- **ssh_config 导入**：一次性复制（导入后与 config 文件无关联）。`ssh2-config` 解析（支持 `Include`），列出不含通配符的 `Host` 别名，用 `query(alias)` 得到合并 `Host *` 等默认值后的参数（与 OpenSSH 一样先出现的值生效）。映射：HostName（支持 `%h`）、Port、User（缺省为本机用户名）、第一个 IdentityFile → 私钥认证（没有则为"自动"）、ProxyJump → 跳板机（config 中的别名一并导入，`[user@]host[:port]` 写法复用同地址的已有会话或新建）、ServerAliveInterval → keepalive、LocalForward / RemoteForward / DynamicForward → 转发规则（不自动启动；受 `ssh2-config` 限制，LocalForward 和 DynamicForward 每个 Host 块只保留最后一条）。与已有会话同名或同地址的主机标为"已存在"、不再导入。ProxyCommand、ForwardAgent、CertificateFile 等未导入的选项在列表中标出。
- **会话配置**：本地 JSON/TOML 文件；密码与口令只存系统钥匙串。

## 国际化（i18n）

i18n 架构已接入，目前只有英语（`en`）一种语言；其他语言的翻译和语言设置界面排期在 MVP 之后（里程碑 M6）。

### 文案来源与处理方式

| 来源 | 示例 | 处理 |
|---|---|---|
| 前端界面 | 按钮、对话框、提示、传输状态 | 前端 `i18next` 按 key 翻译 |
| 后端命令错误 | `Cannot list {path}`、`Session profile not found` | 后端消息目录按当前语言渲染，连同 `code` / `params` 返回 |
| 后端写入终端的提示 | 主机指纹确认、连接关闭、口令错误 | 后端消息目录按当前语言渲染 |
| 远端/系统产生的文本 | 服务器 banner、keyboard-interactive 问题、SFTP 状态、OS 错误 | 原样显示，不翻译（作为错误的技术细节附在译文之后） |

每条文案只在一处维护：界面文案在前端语言包，后端产生的文案在后端语言包。

### 目录与格式

- 前端：`src/locales/<lang>.json`；后端：`src-tauri/locales/<lang>.json`（编译期嵌入）。
- 两端都是嵌套 JSON，key 为点分路径（`sftp.uploadFiles`、`errors.transfer.readFailed`），占位符统一为 `{name}`。
- 前端复数用 i18next 的后缀约定（`transfer.progress_one` / `_other`，参数 `count`），按 `Intl.PluralRules` 选择。
- 英语为源语言，也是缺失 key 时的兜底。

### 前端

- `src/i18n/index.ts`：初始化 i18next（插值前后缀改为 `{` `}`，与后端一致），按 `navigator.languages` 选择最匹配的已支持语言（`zh-Hans-CN` → `zh-Hans` → `zh` → `en`），并调用后端 `set_locale` 保持两端一致；`changeLanguage()` 供将来的语言设置使用。
- `src/i18n/i18next.d.ts`：以 `en.json` 为类型源，`t()` 的 key 写错会在 `tsc` 阶段报错。
- 组件中用 `useTranslation()` 的 `t()`；写入 xterm 的提示同样走 `t()`。
- 日期与数字用 `Intl.DateTimeFormat` / `Intl.NumberFormat` 按当前语言格式化（`src/lib/format.ts`）。
- 后端错误用 `errorMessage()` 显示，逻辑判断用 `errorCode()`（例如传输取消判断 `transfer.cancelled`），不匹配文案内容。

### 后端

- `src-tauri/src/i18n.rs`：消息目录、语言协商（同上的回退规则）、`set_locale` 命令背后的全局语言；`t!("key", name = value)` 宏取译文。
- `error::Error` 为 `{ code, params, detail }`：消息由 `errors.<code>` 渲染，底层原因（OS / 库 / 服务器文本）作为不翻译的 `detail` 追加；序列化给前端为 `{ code, params, message }`。后端内部通过 anyhow 的 `.context(Error::new(..))` 附加，转换回 `Error` 时取最外层的 code。
- 单元测试 `catalog_covers_all_keys_in_sources` 扫描源码中的 `t!("…")` 与 `Error::new("…")`，确保英语语言包包含所有 key。
- OpenSSH 风格、用户可能依赖其字面形式的提示保持英文，不进语言包：`user@host's password:`、`Enter passphrase for key '…':`、`Permission denied, please try again.`，以及 `(yes/no)` 的回答词。

### 新增一种语言（M6 起）

1. 新建 `src/locales/<lang>.json` 和 `src-tauri/locales/<lang>.json`，按英语文件逐 key 翻译（可只翻译部分，缺失项回退英语）。
2. 在 `src/i18n/index.ts` 的 `resources` 和 `src-tauri/src/i18n.rs` 的 `SOURCES` 中登记该语言。
3. 首次加入第二种语言时，同时加入语言设置界面（调用 `changeLanguage()`，并持久化用户选择）。

### 开发约束

- 新文案一律加入语言包，不在组件或后端代码里写死用户可见的文本。
- 用完整句子加参数，不拼接句子片段（便于不同语序的语言翻译）。
- 不根据文案内容做逻辑判断，用错误码或状态。
- 界面布局不假设文案长度，CJK 字体保留回退；大小写等样式用 CSS（`text-transform`）实现。

## Rust 模块规划（`src-tauri/src/`）

| 模块 | 职责 | 阶段 |
|---|---|---|
| `session/` | 终端会话抽象与管理（`TermIo`、`SessionManager`） | M0 ✅ |
| `commands.rs` | Tauri 命令入口 | M0 ✅ |
| `ssh/` | 连接与 shell（`mod.rs`）、russh 回调（`handler.rs`：主机密钥、`forwarded-tcpip`、断线原因）、主机密钥确认（`host_key.rs`）、认证（`auth.rs`） | M1 ✅ |
| `config.rs` | 会话配置存储（`profiles.json`） | M1 ✅ |
| `import.rs` | 从 OpenSSH client config 导入会话 | M4 ✅ |
| `secrets.rs` | keyring 封装 | M1 ✅ |
| `ssh/connections.rs` | 连接注册表，供 SFTP / 端口转发复用连接 | M2 ✅ |
| `sftp/` | 目录浏览与文件操作（`mod.rs`）、递归上传下载与进度（`transfer.rs`） | M2 ✅ |
| `forward/` | 规则与运行管理（`mod.rs`）、`-L`（`local.rs`）、`-R`（`remote.rs`）、`-D`（`dynamic.rs`）、SOCKS 协议（`socks.rs`） | M3 ✅ |
| `pty/` | 本地终端（portable-pty），作为 session 的另一种后端 | M5 |
| `i18n.rs` | 后端消息目录、语言协商、`t!` 宏 | ✅ |
| `error.rs` | 结构化错误（code / params / detail），消息经消息目录渲染 | ✅ |

## 里程碑

| 阶段 | 内容 |
|---|---|
| **M0 骨架** ✅ | Tauri + React + xterm.js；Channel 二进制输出链路；loopback 会话 |
| **M1 MVP** ✅ | 密码/私钥/agent 登录；交互式 shell；多标签；resize；known_hosts；会话保存 + 钥匙串 |
| **M2 SFTP** ✅ | 浏览、上传下载（进度）、拖拽、重命名/删除/新建/chmod |
| **M3 端口转发** ✅ | `-L` / `-R` / `-D`，规则随会话保存，可自动启动 |
| **M4 增强** | ssh_config 导入、ProxyJump、keepalive 与断线重连、终端搜索、主题 |
| **M5 本地终端** | portable-pty（macOS zsh / Windows PowerShell） |
| **i18n 架构** ✅ | 前端 i18next（类型检查 key）与后端消息目录；结构化错误；语言协商与 `set_locale`；仅英语 |
| **M6 翻译** | 语言设置界面；首批翻译简体中文（zh-CN），之后按需增加其他语言 |

## 注意事项

- macOS 上 xterm.js 运行在 WKWebView 里，需重点测试 CJK 输入法（候选框位置、组字过程）。
- 中文 / emoji 宽度依赖 unicode11 插件，远端 `LANG` 需为 UTF-8。
- russh 使用 `ring` 加密后端（而非默认的 aws-lc-rs），避免 Windows 上依赖 CMake/NASM。
- Windows 构建计划用 GitHub Actions（`tauri-apps/tauri-action`）完成。

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
- **认证**：密码、私钥文件（含口令）、ssh-agent（macOS `SSH_AUTH_SOCK`；Windows OpenSSH
  命名管道 / Pageant）、keyboard-interactive（2FA）。
- **主机校验**：读写 `~/.ssh/known_hosts`，首次连接确认指纹，指纹变化时显式告警。
- **连接注册表**：`ssh::Connections` 以会话 id 登记认证完成的连接（`Arc<Handle>`），SFTP 首次使用时在该连接上开 `sftp` subsystem channel 并缓存；shell 结束或标签页关闭时统一断开。
- **传输**：上传/下载先扫描生成计划（目录 + 文件 + 总字节数），再逐个文件复制（256 KiB 缓冲，russh-sftp 内部并发读写请求），进度经 Channel 每 100 ms 推送一次；取消通过共享的 `AtomicBool`，未完成的文件会被删除。下载到"下载"文件夹时顶层重名自动改为 `name (1).ext`。
- **ProxyJump**：在跳板机连接上开 `direct-tcpip` channel，作为下一跳 SSH 的传输层。
- **会话配置**：本地 JSON/TOML 文件；密码与口令只存系统钥匙串。

## 国际化（i18n）

当前所有文案以英语硬编码；翻译接入排期在 MVP 之后（见里程碑 M6）。架构上按以下方式设计，现阶段新写的代码需遵守「开发约束」，以降低日后接入成本。

### 文案来源与处理方式

| 来源 | 示例 | 处理 |
|---|---|---|
| 前端界面 | 按钮、对话框、提示、传输状态 | 前端 i18n 库按 key 翻译 |
| 后端命令错误 | `cannot read {path}`、`profile not found` | 改为结构化错误，由前端翻译 |
| 后端写入终端的提示 | 主机指纹确认、密码提示、连接关闭 | Rust 端消息目录，按当前语言渲染 |
| 远端/系统产生的文本 | 服务器 banner、keyboard-interactive 问题、SFTP 状态消息、系统错误 | 原样显示，不翻译 |

### 前端

- 选用 `i18next` + `react-i18next`：成熟、支持插值与复数（如 `{{count}} files`）、按需加载语言包。
- 语言包放在 `src/locales/<lang>.json`，英语 `en.json` 为源文件和兜底；key 按模块分组（`sftp.uploadFiles`、`profile.authPassword` 等）。
- 语言选择：首次启动跟随系统语言（`tauri-plugin-os` 的 locale，或 `navigator.language`），设置中可手动切换并持久化；不支持的语言回退到英语。
- 日期、数字、文件大小用 `Intl.DateTimeFormat` / `Intl.NumberFormat` 按当前语言格式化（替换 `src/lib/format.ts` 中的手写格式）。
- 原生文件对话框标题等经插件传入的字符串同样走翻译。

### 后端

- **命令错误结构化**：`error::Error` 序列化为 `{ code, params, message }`，`code` 为稳定的错误标识（如 `sftp.readFailed`），`params` 为插值参数，`message` 为英文兜底（含底层原因）。前端按 `code` 翻译，无对应翻译时显示 `message`。
- **终端提示**：Rust 端引入消息目录（候选：`rust-i18n` 或 Fluent `fluent-bundle`），语言包随应用打包；前端在启动和切换语言时调用 `set_locale` 命令，`TermIo` 输出提示时按当前语言取文案。
- OpenSSH 风格、用户可能依赖其字面形式的提示（如 `user@host's password:`、`(yes/no)` 的回答词）保持英文，避免脚本或习惯被打破。

### 开发约束（现在起遵守）

- 文案用完整句子加参数，不拼接句子片段（便于不同语序的语言翻译）。
- 不根据文案内容做逻辑判断（例如传输取消状态用标志位，而非匹配错误字符串）。
- 界面布局不假设文案长度（按钮、列宽留足余量，长文本省略或换行），CJK 字体保留回退。
- 大写等样式用 CSS（`text-transform`）实现，不写进文案本身。

## Rust 模块规划（`src-tauri/src/`）

| 模块 | 职责 | 阶段 |
|---|---|---|
| `session/` | 终端会话抽象与管理（`TermIo`、`SessionManager`） | M0 ✅ |
| `commands.rs` | Tauri 命令入口 | M0 ✅ |
| `ssh/` | 连接与 shell（`mod.rs`）、主机密钥校验（`host_key.rs`）、认证（`auth.rs`） | M1 ✅ |
| `config.rs` | 会话配置存储（`profiles.json`）；ssh_config 导入（M4） | M1 ✅ |
| `secrets.rs` | keyring 封装 | M1 ✅ |
| `ssh/connections.rs` | 连接注册表，供 SFTP / 端口转发复用连接 | M2 ✅ |
| `sftp/` | 目录浏览与文件操作（`mod.rs`）、递归上传下载与进度（`transfer.rs`） | M2 ✅ |
| `forward/` | `-L` / `-R` / `-D`（SOCKS5） | M3 |
| `pty/` | 本地终端（portable-pty），作为 session 的另一种后端 | M5 |
| `i18n/` | 终端提示的消息目录、`set_locale`；错误码定义 | M6 |

## 里程碑

| 阶段 | 内容 |
|---|---|
| **M0 骨架** ✅ | Tauri + React + xterm.js；Channel 二进制输出链路；loopback 会话 |
| **M1 MVP** ✅ | 密码/私钥/agent 登录；交互式 shell；多标签；resize；known_hosts；会话保存 + 钥匙串 |
| **M2 SFTP** ✅ | 浏览、上传下载（进度）、拖拽、重命名/删除/新建/chmod |
| **M3 端口转发** | `-L` / `-R` / `-D`，规则随会话保存，可自动启动 |
| **M4 增强** | ssh_config 导入、ProxyJump、keepalive 与断线重连、终端搜索、主题 |
| **M5 本地终端** | portable-pty（macOS zsh / Windows PowerShell） |
| **M6 国际化** | 接入前端 i18next 与后端消息目录；结构化错误；语言设置；首批翻译简体中文（zh-CN），之后按需增加其他语言 |

## 注意事项

- macOS 上 xterm.js 运行在 WKWebView 里，需重点测试 CJK 输入法（候选框位置、组字过程）。
- 中文 / emoji 宽度依赖 unicode11 插件，远端 `LANG` 需为 UTF-8。
- russh 使用 `ring` 加密后端（而非默认的 aws-lc-rs），避免 Windows 上依赖 CMake/NASM。
- Windows 构建计划用 GitHub Actions（`tauri-apps/tauri-action`）完成。

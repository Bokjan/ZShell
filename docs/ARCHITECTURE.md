# ZShell 架构

个人使用的跨平台（macOS / Windows）SSH 客户端。

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
- **会话抽象**：`session::SessionManager` 持有每个会话的输入队列（`SessionInput`），后端任务
  （loopback / SSH shell / 本地 PTY）消费队列、向 Channel 写输出。关闭会话 = 丢弃发送端。
- **认证**：密码、私钥文件（含口令）、ssh-agent（macOS `SSH_AUTH_SOCK`；Windows OpenSSH
  命名管道 / Pageant）、keyboard-interactive（2FA）。
- **主机校验**：读写 `~/.ssh/known_hosts`，首次连接确认指纹，指纹变化时显式告警。
- **ProxyJump**：在跳板机连接上开 `direct-tcpip` channel，作为下一跳 SSH 的传输层。
- **会话配置**：本地 JSON/TOML 文件；密码与口令只存系统钥匙串。

## Rust 模块规划（`src-tauri/src/`）

| 模块 | 职责 | 阶段 |
|---|---|---|
| `session/` | 终端会话抽象与管理；`loopback` 本地回显后端 | M0 ✅ |
| `commands.rs` | Tauri 命令入口 | M0 ✅ |
| `ssh/` | russh 客户端、认证、known_hosts、ConnectionManager | M1 |
| `config/` | 会话存储、ssh_config 导入 | M1 / M4 |
| `secrets/` | keyring 封装 | M1 |
| `sftp/` | 目录浏览、传输任务与进度 | M2 |
| `forward/` | `-L` / `-R` / `-D`（SOCKS5） | M3 |
| `pty/` | 本地终端（portable-pty），作为 session 的另一种后端 | M5 |

## 里程碑

| 阶段 | 内容 |
|---|---|
| **M0 骨架** ✅ | Tauri + React + xterm.js；Channel 二进制输出链路；loopback 会话 |
| **M1 MVP** | 密码/私钥/agent 登录；交互式 shell；多标签；resize；known_hosts；会话保存 + 钥匙串 |
| **M2 SFTP** | 浏览、上传下载（进度）、拖拽、重命名/删除/新建/chmod |
| **M3 端口转发** | `-L` / `-R` / `-D`，规则随会话保存，可自动启动 |
| **M4 增强** | ssh_config 导入、ProxyJump、keepalive 与断线重连、终端搜索、主题 |
| **M5 本地终端** | portable-pty（macOS zsh / Windows PowerShell） |

## 注意事项

- macOS 上 xterm.js 运行在 WKWebView 里，需重点测试中文输入法（候选框位置、组字过程）。
- 中文 / emoji 宽度依赖 unicode11 插件，远端 `LANG` 需为 UTF-8。
- Windows 构建计划用 GitHub Actions（`tauri-apps/tauri-action`）完成。

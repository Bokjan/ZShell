# ZShell 产品规划

本文记录做什么、为什么、先做哪个，以及尚未实现的功能的设计要点。功能实现后，跨模块的设计写进 [ARCHITECTURE.md](ARCHITECTURE.md)，模块内的细节写在代码注释里，并在下方里程碑表中标记 ✅。

## 定位

- 作为 Xshell 的替代，个人使用，macOS 与 Windows 体验一致。
- 优先补齐每天高频使用的操作（剪贴板、标签页、会话列表），再补 Xshell 的效率功能；不追求逐项对齐 Xshell。
- 行为尽量与 OpenSSH 一致（认证顺序、known_hosts、ssh_config 语义），减少"在 ZShell 里能连、命令行不能连"或反过来的情况。

## 现状

已实现的功能见下方里程碑表（README 的 Features 是面向用户的英语概述），这里只记录取舍。

### 相对 Xshell 的优势（保持）

- 一条连接复用：终端、SFTP、端口转发共用一条 SSH 连接，不像 Xshell + Xftp 那样各连一次、各认证一次。
- 原生支持 macOS（Xshell 没有 Mac 版）。
- ssh_config 导入、自动重连、转发规则实时状态、"自动"认证。

### 主要差距

| 方面 | 差距 | 优先级 |
|---|---|---|
| 数据保护 | 会话等配置是明文，密码逐条存在系统凭据存储里，导出不能带密码 | P1 |
| 杂项 | 没有应用锁 | P2 |

## 里程碑

按计划顺序排列。翻译、上架 Store 与其他里程碑没有依赖，可以随时提前。M17、M19、M21、M22 来自 2026-10 代码审查的设计层面观察，属于内部结构调整，用户可见的变化很少。

| 阶段 | 内容 | 优先级 |
|---|---|---|
| **M0 骨架** ✅ | Tauri + React + xterm.js；Channel 二进制输出链路；loopback 会话 | |
| **M1 MVP** ✅ | 密码 / 私钥 / agent 登录；交互式 shell；多标签；resize；known_hosts；会话保存 + 钥匙串 | |
| **M2 SFTP** ✅ | 浏览、上传下载（进度）、拖拽、重命名 / 删除 / 新建 / chmod | |
| **M3 端口转发** ✅ | `-L` / `-R` / `-D`，规则随会话保存，可自动启动 | |
| **M4 增强** ✅ | ssh_config 导入、ProxyJump、keepalive 与断线重连、终端搜索、主题；"自动"认证与设置界面 | |
| **M5 本地终端** ✅ | portable-pty（macOS 登录 shell / Windows PowerShell，ConPTY）；输出流控；Windows CI 编译检查 | |
| **i18n 架构** ✅ | 前端 i18next 与后端消息目录；结构化错误；语言协商；仅英语 | |
| **M6 终端与标签页** ✅ | 剪贴板与右键菜单、Option 作为 Meta、标签页操作 | |
| **M7 ZMODEM** ✅ | rz/sz 上传下载 | |
| **M8 会话管理** ✅ | 分组、搜索、快速连接、复制会话、右键菜单、导出导入 | |
| **M9 标题栏集成** ✅ | 标签栏画进窗口标题栏，"+" 新建本地终端 | |
| **M10 批量与效率** ✅ | 撰写栏与同步输入、快速命令、会话日志 | |
| **M11 SFTP 增强** ✅ | 多选、另存为、拖出、用本地编辑器编辑 | |
| **M12 会话配置** ✅ | agent 转发、字符编码、单会话外观、登录后命令、TERM 与环境变量 | |
| **M13 Telnet 与串口** ✅ | 会话类型增加 Telnet 与串口 | |
| **M14 代理** ✅ | 出口代理（SOCKS5 / HTTP）与 ProxyCommand | |
| **M15 分屏** ✅ | 标签页内左右 / 上下分屏 | |
| **M16 known_hosts 管理** ✅ | 查看、搜索、删除主机密钥记录 | |
| **M17 前端结构** ✅ | 对话框栈、集中的快捷键分发、`App.tsx` 拆出状态与会话注册 | |
| **M18 无障碍** ✅ | 终端读屏模式；对话框语义与焦点管理；标签栏、会话列表、文件列表的角色与键盘操作；状态播报；焦点可见、减少动态效果、对比度 | |
| **M19 前后端类型与配置结构** ✅ | 从 Rust 生成 TypeScript 类型与错误码联合类型；打开会话的命令合并；`Profile` 按协议拆分（破坏性变更，2.0.0） | |
| **M20 多因素认证** ✅ | 按服务器返回的剩余方法继续认证（`AuthenticationMethods publickey,keyboard-interactive` 等） | |
| **M21 会话生命周期** ✅ | 会话的全部资源由 guard 持有，`SessionManager::remove` 是唯一的清理入口；页面重载或窗口销毁时关闭所属会话；转发规则由单一所有者持有 | |
| **M22 大流量与背压** ✅ | 所有后端的输出都有上限：`Flow` 覆盖 SSH / Telnet / 串口，ZMODEM 接收有界，日志写入移出会话线程；批量传输使用单独的 SFTP channel | |
| **M23 加密保险库与带密码导出** | 会话与密码可选加密（主密码 / 系统凭据存储 / 不加密），首次引导；导出 / 导入可带密码（口令加密） | P1 |
| **M24 应用锁** | 用 Touch ID / Windows Hello 锁定应用 | P2 |
| **M25 翻译** | 语言设置界面，首批简体中文 | — |
| **M26 Microsoft Store** | 打包 MSIX 上架 Microsoft Store | — |

### M23 加密保险库与带密码导出

**为什么**：现在会话、文件夹、代理、快速命令是明文 JSON，密码逐条存在系统凭据存储里（macOS 钥匙串的应用程序密码，服务 `org.boyin.zshell`、帐户为会话 id 或 `proxy:<id>`；Windows 凭据管理器的普通凭据 `<帐户>.org.boyin.zshell`）。配置目录被拷走（备份、同步盘）时主机、用户、跳板机链、代理命令都会外泄；ad-hoc 签名每版不同，macOS 升级后可能逐条要求重新授权；导出也带不了密码。改为可选的加密保险库，由用户选择保护方式，导出时可用口令加密带上密码。

**保护方式**（首次启动引导选择，之后在 设置 › 安全 中随时更改）
- **主密码**：每次启动输入主密码解锁。可勾选"在此设备上记住"（默认不勾），把数据密钥存进系统凭据存储，启动时自动解锁；M24 可改为 Touch ID / Windows Hello。忘记主密码无法找回，只能"重置"（删除保险库，会话与密码全部丢失，需输入确认），引导页与设置中明确说明并建议先导出。
- **系统凭据存储**：数据密钥存在钥匙串 / 凭据管理器里，启动时自动解锁。只防"只有文件被拿走"，能以该用户身份运行程序的人同样能取到密钥，界面上说明。
- **不加密**：会话等仍是现在的明文 JSON，密码也以明文存在配置目录的 `passwords.json`（unix 上权限 0600），不用凭据存储。界面上明确提示风险。
- **未设置**（引导时选"以后再说"、或从旧版本升级后尚未选择）：行为与 1.5 完全相同（明文 JSON，密码逐条在凭据存储里），设置里提示可以开启加密。不会在用户不知情时降低或改变保护方式。

**加密范围**：会话、文件夹、代理、密码、快速命令。不加密的：`settings.json`（外观、语言在解锁界面之前就要生效）、日志索引 `logs.json`、会话日志本身、`~/.ssh/known_hosts`（属于 OpenSSH）。隐私政策写明。

**存储格式**
- 两层密钥：32 字节随机的数据密钥（`ring::rand::SystemRandom`）加密全部内容；保护方式只决定数据密钥由谁保护。切换主密码与凭据存储、修改主密码、开关"记住"都只重新包装数据密钥。
- 加密时合为一个文件 `vault.json`（原子写入，unix 上 0600）：明文头 `{ format: "zshell-vault", version: 1, protection: "system" | "password", keyId, wrappedKey?, nonce, data }`。`keyId` 为数据密钥 SHA-256 的前 8 字节，用来区分"密钥不对"与"文件损坏"。`data` 是 `{ folders, profiles, proxies, commands, passwords }` 的 JSON 经 AES-256-GCM（`ring::aead`）加密，每次写入换新的 96 位随机 nonce，AAD 为明文头（防止篡改头部）。
- 主密码：PBKDF2-HMAC-SHA256（ring 的 `pbkdf2`，60 万次，16 字节随机盐，不引入新依赖）派生包装密钥，以 AES-256-GCM 包装数据密钥写进 `wrappedKey`；派生在阻塞线程上进行。凭据存储（系统模式，或主密码 + 记住）里只有一条：服务 `org.boyin.zshell`、帐户 `vault-key`，值为 base64 的数据密钥。
- `passwords` 的键沿用现在的帐户命名（会话 id、`proxy:<id>`）。复制会话复制密码、删除会话 / 代理删除密码、Telnet 与 SSH 共用会话那一条，这些行为不变。
- 后端的会话、代理、快速命令存储改为经保险库读写：未解锁时涉及它们的命令返回 `vault.locked`；凭据存储的访问抽象为接口，单元测试用内存实现。

**解锁**：主密码模式（未记住，或记住的密钥读不出、`keyId` 不符）启动时整个窗口被解锁界面盖住，解锁前不显示会话、不能打开标签。主密码错误（GCM 校验失败）提示重试。系统模式读不出密钥时同样显示解锁界面，说明原因，并提供"重置"。该界面供 M24 应用锁复用。

**切换与迁移**（都先写好新的、读回校验，再删除旧的）
- 未设置 / 不加密 → 加密：生成数据密钥，写 `vault.json`，读回校验后删除明文的 `profiles.json`、`folders.json`、`proxies.json`、`commands.json`、`passwords.json`；未设置时还要把逐条的凭据迁进保险库并删除旧条目（macOS 上旧条目的授权提示最多出现这一次）。
- 加密 → 不加密：解密后写回明文文件与 `passwords.json`，删除 `vault.json` 与 `vault-key`。
- 未设置 → 不加密：逐条凭据迁进 `passwords.json` 并删除旧条目。
- 降级：加密后旧版本（≤ 1.5）读不到会话；不加密模式的明文文件旧版本照常读取，但看不到 `passwords.json` 里的密码。引导页与设置中提示。新版本照常读取旧版本的全部文件。

**引导**：首次启动（或从旧版本升级后第一次启动）弹出"保护会话与密码"，列出三种方式与"以后再说"，说明各自防什么、不防什么；选主密码时输入两次。只弹一次，之后在设置中更改。

**带密码导出 / 导入**
- 导出对话框增加"包含保存的密码"，勾选后输入两次导出口令（不保存、无法找回）。文件增加可选的 `passwords` 一节：`{ kdf: { name: "pbkdf2-sha256", iterations: 600000, salt }, cipher: "aes-256-gcm", nonce, data }`，`data` 解密后为 `{ <文件中的帐户>: 密码 }`，AAD 为 `zshell-sessions-passwords-v1`。导出口令与本机的保护方式无关。
- 文件仍为 `format: "zshell-sessions"`、`version: 1`：旧版本忽略 `passwords`，照常导入（不带密码）。
- 导入时文件带密码则多一个口令输入框（留空则不导入密码）；口令错误时提示并可重试（`backup.wrongPassphrase`）。导入的会话与代理分配新 id，密码按旧 id → 新 id 存入当前的保护方式；已存在而未导入的会话、复用的已有代理不改动其密码。

### M24 应用锁

- 启动时、闲置一段时间后、系统锁屏或睡眠后，盖上锁定界面，用系统认证解锁：macOS 用 LocalAuthentication（Touch ID，不可用时退回登录密码），Windows 用 Windows Hello（PIN、指纹、人脸）。会话在锁定期间照常运行。
- 系统没有可用的认证方式时不能开启。
- 锁定界面复用 M23 的解锁界面。只锁界面，不重新加密内存中的数据；要让保险库真正受生物识别保护，需要把数据密钥放进要求生物识别的钥匙串项（M23 的"在此设备上记住"），macOS 上需要开发者证书签名。
- 开工前先用原型确认 ad-hoc 签名下 LocalAuthentication 可用。

### M25 翻译

语言设置界面，首批简体中文；做法见 [I18N.md](I18N.md)「新增一种语言」。

### M26 Microsoft Store

已完成：release workflow 打出 x64 与 ARM64 的 `.msixbundle`（见 [DEVELOPMENT.md](DEVELOPMENT.md)「发布」）、S 模式下隐藏本地终端、随包附上第三方许可证声明。还剩：

- 打包后实测：本地终端（ConPTY 启动 PowerShell）、OpenSSH agent 命名管道与 Pageant、凭据管理器里的密码、用本地编辑器编辑与拖出、会话日志写入「文档」、自绘标题栏与贴靠布局。本地测试用 `Add-AppxPackage -Register AppxManifest.xml` 注册解包后的目录（需开启开发者模式），不用签名。
- 临时文件："用本地编辑器编辑"与拖出到资源管理器的临时文件写在 `%TEMP%`，它在 `AppData\Local` 下，被文件系统虚拟化重定向后外部编辑器和资源管理器可能看不到。实测若如此，有包身份时（`GetCurrentPackageFullName` 成功）改用包的临时目录 `ApplicationData::Current().TemporaryFolder()`（`…\Packages\<PFN>\TempState`），那是真实路径，外部进程可以访问。
- 上架前提：仓库设为公开（隐私政策与 Issues 的链接要能打开）。ZShell 本身不开源，条款见根目录的 `LICENSE.md`：发布的安装包可免费用于任何用途，源码可以自己研究、构建和修改，但不能分发；原则上不接受 PR，改动先在 issue 里讨论（`CONTRIBUTING.md`）。
- 提交：先在 Partner Center 手动上传每个版本的 `.msixbundle`；流程稳定后再考虑用 Store 提交 API（`msstore` CLI，需要把 Entra ID 应用的凭据配成 secret）自动提交。
- 商店页面：描述、截图、年龄分级在 Partner Center 填写；提交时说明使用 `runFullTrust` 的理由（完整的桌面应用，需要启动本地 shell、访问 SSH agent 等）。隐私政策写明应用不收集数据、配置只保存在本机，放在仓库里，用 GitHub 链接。

## 暂不排期（P2）

有实际需要时再排期：

- 关键字高亮（如 `ERROR` 标红）：需在 xterm.js 的渲染层用 decoration 实现，不能往输出流里插入颜色序列（会破坏全屏程序）。
- X11 转发（macOS 上还要依赖 XQuartz）。
- 主机证书（`@cert-authority`）：用 CA 管理主机的环境不必逐台确认主机密钥。现在主机证书被当作普通公钥；要先确认 russh 是否支持证书形式的主机密钥，不支持则需要上游补丁。
- macOS 上 VoiceOver 开启时自动打开终端读屏模式：WebView 拿不到这个状态，需要后端查询（`NSWorkspace.isVoiceOverEnabled`）；现在是设置里的手动开关。

## 不做

- Xshell 的脚本（VBS / JScript / Python）：快速命令和登录后命令能覆盖大多数场景。
- 独立的密钥代理（Xagent）：使用系统的 ssh-agent / Pageant / Windows OpenSSH agent。
- 云同步服务：用导出文件配合用户自己的同步盘。
- 在标签里临时切换字符编码：编码在会话配置中设置。

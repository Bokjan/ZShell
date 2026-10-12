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
| **M23 终端全屏** ✅ | 窗口全屏且只显示当前标签的终端（⌘↩ / F11）；快捷键集中声明在一张表里（`lib/keymap.ts`） | |
| **M24 状态栏** ✅ | 窗口底部显示当前窗格的连接、上下行活动、终端类型、尺寸与光标位置；调整大小时显示终端尺寸 | |
| **M25 加密保险库与带密码导出** ✅ | 会话与密码可选加密（主密码 / 系统凭据存储 / 不加密），首次引导；导出 / 导入可带密码（口令加密） | |
| **M26 应用锁** | 用 Touch ID / Windows Hello 锁定应用 | P2 |
| **M27 翻译** | 语言设置界面，首批简体中文 | — |
| **M28 Microsoft Store** | 打包 MSIX 上架 Microsoft Store | — |
| **M29 自定义快捷键** | 在设置里修改快捷键 | P2 |
| **M30 Linux** | 支持 Linux 桌面 | P2 |

### M26 应用锁

- 启动时、闲置一段时间后、系统锁屏或睡眠后，盖上锁定界面，用系统认证解锁：macOS 用 LocalAuthentication（Touch ID，不可用时退回登录密码），Windows 用 Windows Hello（PIN、指纹、人脸）。会话在锁定期间照常运行。
- 系统没有可用的认证方式时不能开启。
- 锁定界面复用 M25 的解锁界面（`UnlockScreen`，见 [ARCHITECTURE.md](ARCHITECTURE.md)「加密保险库」）。只锁界面，不重新加密内存中的数据；要让保险库真正受生物识别保护，需要把数据密钥放进要求生物识别的钥匙串项（M25 的"在此设备上记住"），macOS 上需要开发者证书签名。
- 开工前先用原型确认 ad-hoc 签名下 LocalAuthentication 可用。

### M27 翻译

语言设置界面，首批简体中文；做法见 [I18N.md](I18N.md)「新增一种语言」。

### M28 Microsoft Store

已完成：release workflow 打出 x64 与 ARM64 的 `.msixbundle`（见 [DEVELOPMENT.md](DEVELOPMENT.md)「发布」）、S 模式下隐藏本地终端、随包附上第三方许可证声明。还剩：

- 打包后实测：本地终端（ConPTY 启动 PowerShell）、OpenSSH agent 命名管道与 Pageant、凭据管理器里的密码、用本地编辑器编辑与拖出、会话日志写入「文档」、自绘标题栏与贴靠布局。本地测试用 `Add-AppxPackage -Register AppxManifest.xml` 注册解包后的目录（需开启开发者模式），不用签名。
- 临时文件："用本地编辑器编辑"与拖出到资源管理器的临时文件写在 `%TEMP%`，它在 `AppData\Local` 下，被文件系统虚拟化重定向后外部编辑器和资源管理器可能看不到。实测若如此，有包身份时（`GetCurrentPackageFullName` 成功）改用包的临时目录 `ApplicationData::Current().TemporaryFolder()`（`…\Packages\<PFN>\TempState`），那是真实路径，外部进程可以访问。
- 上架前提：仓库设为公开（隐私政策与 Issues 的链接要能打开）。ZShell 本身不开源，条款见根目录的 `LICENSE.md`：发布的安装包可免费用于任何用途，源码可以自己研究、构建和修改，但不能分发；原则上不接受 PR，改动先在 issue 里讨论（`CONTRIBUTING.md`）。
- 提交：先在 Partner Center 手动上传每个版本的 `.msixbundle`；流程稳定后再考虑用 Store 提交 API（`msstore` CLI，需要把 Entra ID 应用的凭据配成 secret）自动提交。
- 商店页面：描述、截图、年龄分级在 Partner Center 填写；提交时说明使用 `runFullTrust` 的理由（完整的桌面应用，需要启动本地 shell、访问 SSH agent 等）。隐私政策写明应用不收集数据、配置只保存在本机，放在仓库里，用 GitHub 链接。

### M29 自定义快捷键

快捷键已集中在 `lib/keymap.ts` 的一张表里（见 [ARCHITECTURE.md](ARCHITECTURE.md)「快捷键、菜单与侧栏」），自定义只是在表上叠加用户的覆盖项：

- `settings.json` 只保存改动过的动作（`"shortcuts": { "find": ["Ctrl+Shift+F"] }`），旧配置照常读取，"恢复默认"即删除覆盖项。
- 设置里 Keyboard Shortcuts 一节的每行可以录制新按键：显示与其他动作的冲突；在 macOS 以外的平台上占用 shell 需要的键（不带 Shift 的 Ctrl+字母）时提示终端将收不到它。
- 成组的动作（⌘1–⌘8、方向键切换窗格）只允许改修饰键。
- macOS 原生菜单项（关闭标签 / 窗口、设置）的按键在 `lib.rs` 里，改它们需要后端按新按键重建菜单。

### M30 Linux

- 快捷键沿用 Windows 的一套（`keymap.ts` 的 `other`），按需单独覆盖。
- 平台判断改为 `macos` / `windows` / `linux` 三种，检查现有只区分两种的地方：窗口按钮（Linux 用系统标题栏还是自绘）、"在文件管理器中显示"等文案、密码存储（Secret Service）的提示。
- 本地终端用 `$SHELL` 登录 shell；SSH agent 用 `SSH_AUTH_SOCK`，与 macOS 相同。
- 打包 deb / AppImage（图标用 `bundle.icon` 里的 PNG），CI 的 matrix 加上 `ubuntu-latest`。
- 只允许一个实例，与 Windows 相同：再次启动时把已有的窗口提到前台，第二个进程立即退出。保险库每次写入是整个文件，两个实例会互相覆盖对方的修改（明文的配置文件同样如此）。single-instance 插件支持 Linux（经 D-Bus），把 `Cargo.toml` 里它的 target 与 `lib.rs` 注册处的 `cfg(windows)` 改为 Windows 与 Linux。
- 密码与保险库：Secret Service 已经在 keyring 的依赖里，保险库按三个平台设计（见 [ARCHITECTURE.md](ARCHITECTURE.md)「加密保险库」）。没有 Secret Service 时"系统凭据存储"与"记住"不可选、主密码与不加密照常可用；可用性探测与界面文案已按 `secretService` 处理，需要在真机上验证（GNOME Keyring、KWallet、无桌面环境）。Flatpak 沙箱里要经 portal 访问，不在计划内。
- 许可证：`about.toml` 的 `targets` 加上 Linux；Linux 独有的依赖（GTK / WebKitGTK 绑定、zbus 等）目前都是 MIT / Apache-2.0 / Zlib / ISC，`target-lexicon`（Apache-2.0 WITH LLVM-exception）只是构建依赖。

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
- 探测远端支持的终端类型：SSH 申请 pty 时只发出 TERM 字符串，协议里没有确认；要探测只能先开 exec channel 在远端运行 `infocmp`，每次连接多一次往返，网络设备、Windows OpenSSH 上也没有这类工具。ZShell 模拟的是 xterm，`xterm-256color` 几乎处处可用。

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

盖上锁定界面，用系统认证解锁，会话在锁定期间照常运行。只锁界面，不重新加密内存中的数据；要让保险库真正受生物识别保护，需要把数据密钥放进要求生物识别的钥匙串项（M25 的"在此设备上记住"），macOS 上需要开发者证书签名，不在本阶段。威胁模型是离开座位时别人动已打开的 ZShell，以及改配置文件后重启绕过。

- **范围**：只实现 macOS 与 Windows，接口按三个平台设计（Linux 见 M30）。
- **系统认证**（新模块 `applock.rs`，按平台分文件）：macOS 用 LocalAuthentication 的 `DeviceOwnerAuthentication`（Touch ID，没有时退回登录密码；已用原型确认 ad-hoc 签名下可用，不需要 entitlement），依赖 `objc2-local-authentication`；Windows 用 Windows Hello，经 `IUserConsentVerifierInterop::RequestVerificationForWindowAsync` 把认证框挂在主窗口上（不用 `RequestVerificationAsync`，它在 Win32 应用里可能出现在窗口后面）。后端报告三项能力：认证方式（`touchId` / `password` / `windowsHello` / 不可用）、能否检测闲置、能否检测锁屏与睡眠；前端按它选择文案、隐藏不支持的选项，不猜平台。
- **锁定状态在后端**：后端持有是否锁定，前端启动时查询，所以页面重载后仍然锁着；解锁由后端调用系统认证，通过后才改状态。后端锁定时发事件通知前端。
- **何时锁定**：
  - 启动时：主密码保护且未"记住"时，输入主密码即算认证，不再锁一次；其他情况先读出保险库（系统凭据存储照常静默读取），设置为开启则先显示锁定界面，再显示应用。
  - 闲置：按整个系统的闲置时间（macOS `CGEventSourceSecondsSinceLastEventType`，Windows `GetLastInputInfo`），不只看 ZShell 窗口，终端输出不算活动。可选从不 / 1 / 5 / 15 / 30 / 60 分钟，默认 15。
  - 系统锁屏、睡眠、切换用户（可关，默认开）：事件发生时就锁，醒来时画面已被盖住。macOS 监听 `com.apple.screenIsLocked`、`NSWorkspaceWillSleepNotification` 与会话切换；Windows 用 `WTSRegisterSessionNotification`（`WTS_SESSION_LOCK`）与 `PBT_APMSUSPEND`。
  - 手动：`keymap.ts` 加"锁定"动作（⌘⇧L / Ctrl+Shift+L，与现有快捷键不冲突），macOS 的 App 菜单与命令面板各有一项。
- **设置存放**：应用锁的设置存进保险库，与会话、密码在一起，不放 `settings.json`，这样加密时改不了也删不掉；旧文件读出为关闭，重置保险库时一并清除。不加密时与会话一样是明文，设置里提示加密才能防止被绕过；未设置保护方式时不能开启，先选一种。
- **设置界面**：在"安全"一节保护方式之下，与"更改保护方式"一样走后端命令，不经 `SettingsProvider` 的即时保存。开启时先认证一次，确认可用；系统没有可用的认证方式时开关禁用并说明原因。"恢复默认设置"不影响保护方式与应用锁，确认文字里写明。
- **认证方式不可用**：开启后认证方式没了（如删除了 Windows Hello PIN），启动时不锁定，并在设置里提示。删除 PIN、修改 macOS 登录密码都要先输入账户密码，做得到的人已控制这个账户。
- **锁定界面**：把 `UnlockScreen` 的外壳（标题栏、面板、图标）拆成共用的 `LockScreen`，保险库解锁与应用锁各填内容；内容同时支持"系统认证按钮"与"输入密码"两种，后者供 Linux 以后的 PAM 后备复用。应用锁是覆盖层，应用保持挂载：其下的应用根元素加 `inert` 并以 `visibility: hidden` 隐藏，不能用 `display: none`（xterm 会被 fit 成 0×0，向远端发 resize）；覆盖层有自己的标题栏与窗口按钮，所以这里给根元素加 `inert` 不影响 Windows 的窗口按钮。只在窗口有焦点时自动弹出系统认证（闲置锁定时用户可能在别的应用里），取消后不再自动弹出，等用户点"解锁"。
- **锁定期间要拦住的**：
  - ZMODEM：远端 `rz`（以及"选择文件夹"设置下的 `sz`）会自动打开原生文件对话框，它会出现在锁定界面之上，所以等解锁后再打开。
  - 拖放：Tauri 的 `onDragDropEvent` 不受 `inert` 影响，终端与 SFTP 的处理里判断是否锁定。
  - 快捷键与原生菜单事件（⌘W、⌘, 等）不处理；⌘Q 与关闭窗口照常可用，确认放在锁定界面上，只说会结束几个会话。
  - `announce()` 在锁定期间丢弃，读屏软件不播报后台会话的变化。
  - 已知限制：锁定时已经打开的原生文件对话框盖不住。
- **验证**：锁定的判断（启动时是否锁、闲置、认证方式不可用）用假的认证实现做单元测试；Touch ID 在真机上试，Windows Hello 只有 CI 编译检查，需在 Windows 真机上试。

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

第一阶段的目标是 Debian 与 Fedora 的默认桌面，即 GNOME + Wayland；其他桌面与发行版按需再做。

- 快捷键沿用 Windows 的一套（`keymap.ts` 的 `other`），按需单独覆盖。
- 平台判断改为 `macos` / `windows` / `linux` 三种，检查现有只区分两种的地方：窗口按钮（Linux 用系统标题栏还是自绘）、"在文件管理器中显示"等文案、密码存储（Secret Service）的提示。
- 本地终端用 `$SHELL` 登录 shell；SSH agent 用 `SSH_AUTH_SOCK`，与 macOS 相同。
- 打包 deb 与 rpm（图标用 `bundle.icon` 里的 PNG），CI 的 matrix 加上 `ubuntu-latest`。不打 AppImage：应用锁的 polkit 策略文件要由包管理器装进系统目录。
- 应用锁（M26 的接口已按三个平台设计）：认证用 polkit，自定义一个 `auth_self` 的 action，弹出 GNOME Shell 自带的认证框（登录密码，启用了 fprintd 时也可用指纹），策略文件随 deb / rpm 装进 `/usr/share/polkit-1/actions/`；PAM（在锁定界面输入登录密码，经 `unix_chkpwd` 校验，各发行版的 service 名不同）作为以后没有 polkit 代理时的后备。闲置用 Mutter 的 `org.gnome.Mutter.IdleMonitor`（Wayland 没有通用接口；X11 有 XScreenSaver 扩展，KDE 有 `org.freedesktop.ScreenSaver.GetSessionIdleTime`）。锁屏与睡眠用 logind 的 `PrepareForSleep` 与会话的 `Lock` / `LockedHint`。这些都经 D-Bus，需要 zbus（MIT），它目前不在依赖树里。
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

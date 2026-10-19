# ZShell 产品规划

本文记录做什么、为什么、先做哪个，以及尚未实现的功能的设计要点。功能实现后，跨模块的设计写进 [ARCHITECTURE.md](ARCHITECTURE.md)，模块内的细节写在代码注释里，并在下方里程碑表中标记 ✅。

## 定位

- 作为 Xshell 的替代，个人使用，macOS、Windows 与 Linux 体验一致。
- 优先补齐每天高频使用的操作（剪贴板、标签页、会话列表），再补 Xshell 的效率功能；不追求逐项对齐 Xshell。
- 行为尽量与 OpenSSH 一致（认证顺序、known_hosts、ssh_config 语义），减少"在 ZShell 里能连、命令行不能连"或反过来的情况。

## 现状

已实现的功能见下方里程碑表（README 的 Features 是面向用户的英语概述），这里只记录取舍。

### 相对 Xshell 的优势（保持）

- 一条连接复用：终端、SFTP、端口转发共用一条 SSH 连接，不像 Xshell + Xftp 那样各连一次、各认证一次。
- 原生支持 macOS（Xshell 没有 Mac 版）。
- ssh_config 导入、自动重连、转发规则实时状态、"自动"认证。

## 里程碑

按计划顺序排列。M17、M19、M21、M22 来自 2026-10 代码审查的设计层面观察，M38 来自 2026-10-16 的架构评审，M39 来自 2026-10-17 的渲染进程内存排查，都属于内部结构调整，用户可见的变化很少。原定的 M33 自定义快捷键已移到下文「暂不排期」，编号空缺。

| 阶段 | 内容 |
|---|---|
| **M0 骨架** ✅ | Tauri + React + xterm.js；Channel 二进制输出链路；loopback 会话 |
| **M1 MVP** ✅ | 密码 / 私钥 / agent 登录；交互式 shell；多标签；resize；known_hosts；会话保存 + 钥匙串 |
| **M2 SFTP** ✅ | 浏览、上传下载（进度）、拖拽、重命名 / 删除 / 新建 / chmod |
| **M3 端口转发** ✅ | `-L` / `-R` / `-D`，规则随会话保存，可自动启动 |
| **M4 增强** ✅ | ssh_config 导入、ProxyJump、keepalive 与断线重连、终端搜索、主题；"自动"认证与设置界面 |
| **M5 本地终端** ✅ | portable-pty（macOS 登录 shell / Windows PowerShell，ConPTY）；输出流控；Windows CI 编译检查 |
| **i18n 架构** ✅ | 前端 i18next 与后端消息目录；结构化错误；语言协商；当时仅英语（翻译见 M31） |
| **M6 终端与标签页** ✅ | 剪贴板与右键菜单、Option 作为 Meta、标签页操作 |
| **M7 ZMODEM** ✅ | rz/sz 上传下载 |
| **M8 会话管理** ✅ | 分组、搜索、快速连接、复制会话、右键菜单、导出导入 |
| **M9 标题栏集成** ✅ | 标签栏画进窗口标题栏，"+" 新建本地终端 |
| **M10 批量与效率** ✅ | 撰写栏与同步输入、快速命令、会话日志 |
| **M11 SFTP 增强** ✅ | 多选、另存为、拖出、用本地编辑器编辑 |
| **M12 会话配置** ✅ | agent 转发、字符编码、单会话外观、登录后命令、TERM 与环境变量 |
| **M13 Telnet 与串口** ✅ | 会话类型增加 Telnet 与串口 |
| **M14 代理** ✅ | 出口代理（SOCKS5 / HTTP）与 ProxyCommand |
| **M15 分屏** ✅ | 标签页内左右 / 上下分屏 |
| **M16 known_hosts 管理** ✅ | 查看、搜索、删除主机密钥记录 |
| **M17 前端结构** ✅ | 对话框栈、集中的快捷键分发、`App.tsx` 拆出状态与会话注册 |
| **M18 无障碍** ✅ | 终端读屏模式；对话框语义与焦点管理；标签栏、会话列表、文件列表的角色与键盘操作；状态播报；焦点可见、减少动态效果、对比度 |
| **M19 前后端类型与配置结构** ✅ | 从 Rust 生成 TypeScript 类型与错误码联合类型；打开会话的命令合并；`Profile` 按协议拆分（破坏性变更，2.0.0） |
| **M20 多因素认证** ✅ | 按服务器返回的剩余方法继续认证（`AuthenticationMethods publickey,keyboard-interactive` 等） |
| **M21 会话生命周期** ✅ | 会话的全部资源由 guard 持有，`SessionManager::remove` 是唯一的清理入口；页面重载或窗口销毁时关闭所属会话；转发规则由单一所有者持有 |
| **M22 大流量与背压** ✅ | 所有后端的输出都有上限：`Flow` 覆盖 SSH / Telnet / 串口，ZMODEM 接收有界，日志写入移出会话线程；批量传输使用单独的 SFTP channel |
| **M23 终端全屏** ✅ | 窗口全屏且只显示当前标签的终端（⌘↩ / F11）；快捷键集中声明在一张表里（`lib/keymap.ts`） |
| **M24 状态栏** ✅ | 窗口底部显示当前窗格的连接、上下行活动、终端类型、尺寸与光标位置；调整大小时显示终端尺寸 |
| **M25 加密保险库与带密码导出** ✅ | 会话与密码可选加密（主密码 / 系统凭据存储 / 不加密），首次引导；导出 / 导入可带密码（口令加密） |
| **M26 应用锁** ✅ | 用 Touch ID / Windows Hello 锁定应用：启动、闲置、系统锁屏或睡眠时，以及手动 |
| **M27 多窗口** ✅ | 多个窗口各有自己的标签；标签可以从菜单或拖拽移到新窗口或其他窗口，连接不断开 |
| **M28 应用菜单** ✅ | Windows 上由标题栏的图标与名称打开应用菜单；菜单与 macOS 原生菜单由同一份定义生成 |
| **M29 检查更新** ✅ | 定期查询 GitHub Release，有新版本时提示（Store 版不检查） |
| **M30 Linux** ✅ | GNOME + Wayland：自绘标题栏与跟随系统的主题、拖出文件、polkit 应用锁、单实例；deb 与 rpm |
| **M31 翻译** ✅ | 语言设置；简体中文、日语、德语、法语；CJK 字体按语言选择 |
| **M32 Microsoft Store** ✅ | x64 与 ARM64 的 MSIX 上架 [Microsoft Store](https://apps.microsoft.com/detail/9n502tzg8hk8)；S 模式下隐藏本地终端 |
| **M34 文件面板跟随终端** ✅ | 文件面板在终端所在的文件夹打开，可跟随 shell 切换文件夹；路径栏补全；会话备注 |
| **M35 终端与会话细节** ✅ | 响铃（提示音、后台标签与窗口提醒、可选闪屏）；后台标签的新输出标记；⌘ / Ctrl+点击打开链接与 OSC 8；用户退出 shell 后关闭窗格；断开的会话在原窗格重连；界面字体；会话列表中会话与文件夹混排（3.2.0–3.3.0） |
| **M36 自动读屏模式** ✅ | 系统的读屏软件开启时自动打开终端读屏模式（macOS、Windows、Linux） |
| **M37 主机证书** ✅ | 按 known_hosts 的 `@cert-authority` 接受 CA 签发的主机证书，不必逐台确认主机密钥 |
| **M38 架构调整** ✅ | 保存的数据类型独立为 `model/`；会话的已存密码随路由传入，不经全局变量；存储在保险库解锁后自己重读；连接注册表移出 `ssh/`；命令集中在 `commands/`；命令名与事件名由后端生成并检查；前端分为 `lib/`、`ipc/`、`hooks/`；样式按区域有序拆分（3.5.0） |
| **M39 React Compiler** ✅ | 组件按 Rules of React 改写，由 React Compiler 自动 memo，测试检查每个组件都能编译；侧栏不再随标签页变化重新渲染，标签栏、文件列表、传输列表与快速命令栏只重新渲染变了的那一项（3.6.0） |
| **M40 移动窗格** ✅ | 窗格经终端右键菜单或拖动抓手移到新标签页、其他标签页（合并成分屏）或其他窗口；标签可拖到另一标签的窗格旁合并；终端与面板不随标签页重新挂载（3.7.0） |
| **M41 终端连字** ✅ | 终端字体的编程连字：关闭、常用序列，或按字体自己的规则（后端读出已安装字体的 GSUB，各终端共用）（3.8.0） |
| **M42 终端与系统集成** | 远端程序经 OSC 序列写入剪贴板（52）、报告进度（9;4）、发出通知（9、777、99）、设置鼠标指针（22）（3.9.0） |

### M42 终端与系统集成的设计要点

xterm.js 自己处理标题、调色板、链接与颜色查询（OSC 0–2、4、8、10–12、104、110–112），其余由我们在 `TerminalView` 里用 `registerOscHandler` 接上，解析是 `lib/osc.ts` 里的纯函数（单元测试），不经后端。都只作用于发出序列的窗格，与同步输入无关；文本日志本来就去掉 OSC 序列。

- **剪贴板（OSC 52）**：`52;Pc;Pd`，`Pd` 为 base64，按 UTF-8 解码（`atob` 得到的是字节）后经 `writeText` 写入系统剪贴板，并 `announce()`。`Pc` 不论是 `c`、`s`、`p` 还是空都写系统剪贴板（剪贴板插件没有 Linux 的 primary selection）。只写不读：`Pd` 为 `?` 的查询不回应，远端程序不能读到本地剪贴板；空的 `Pd`（清空）、不合法的 base64 或 UTF-8、解码后超过 1 MB 的都忽略。设置 `terminal.clipboardWrite`（默认开）。tmux 3.3 起在 `TERM` 为 `xterm*` 时默认把复制转发成 OSC 52，无需配置。
- **进度（OSC 9;4）**：`9;4;状态;百分比`，状态 0 清除、1 进行中、2 错误、3 不确定、4 暂停（ConEmu 的定义，Windows Terminal 等沿用）。窗格记住最近一次的状态，会话结束、断开或 15 秒没有更新时清除，程序崩溃或被 Ctrl+C 打断不会留下一直挂着的进度。
  - 标签：状态灯换成环形进度，错误红、暂停黄，不确定时转圈（减少动态效果时静止）；分屏时取优先级最高的窗格（错误 > 暂停 > 进行中 > 不确定），同为进行中取最小的百分比。状态灯的无障碍名称带上进度，后台标签进入错误状态时 `announce()`。
  - 任务栏与 Dock：Tauri 的 `setProgressBar`，Windows 每个窗口各自显示，取窗口里各标签合并的状态；macOS 的 Dock 与 Linux 的 libunity（GNOME 的 Dash to Dock、Ubuntu Dock）整个应用只有一个，由后端合并各窗口上报的状态。tao 在 macOS 上把 Dock 图标换成画着进度条的 `NSImageView`，要检查清除后图标（深色与着色图标）能否恢复。
- **通知（OSC 9、777、99）**：
  - 三种格式：`9;正文`（iTerm2；负载以数字加分号开头的是 ConEmu 的子命令，如 `9;4`，不当作通知）、`777;notify;标题;正文`（urxvt、Ghostty）、`99;元数据;负载`（kitty，只取 `p=title` / `p=body`，支持 base64 的 `e=1` 与分块的 `d=0`；按钮、图标、查询 `p=?` 与关闭回报都不实现，不回应）。
  - 呈现：标题为空时用窗格标题，正文截断到合理长度。窗格所在的标签在前台且窗口有焦点时不提醒（用户正看着）；标签在后台而窗口有焦点时，像响铃那样给标签加提醒标记并 `announce()`；窗口没有焦点时发系统通知（`tauri-plugin-notification`，MIT / Apache-2.0）。每个窗格限频，远端程序刷不了屏。设置 `terminal.notifications`（默认开）。
  - 插件在桌面上不回报点击，点通知只能激活应用，不能切到对应标签。macOS 首次发送时系统询问权限；Windows 的 dev 构建下通知显示为 PowerShell 发出，安装版与 Store 版用各自的 AppUserModelID。
- **鼠标指针（OSC 22）**：`22;名称`，名称是 CSS 的 cursor 值，kitty 还接受 X11 的名称（`hand2`、`xterm` 等），按表换成 CSS 值，不认识的忽略，空名称恢复默认；查询（`?`）不回应。主屏与备用屏各记一份（与 kitty 相同），全屏程序退出后恢复。按住 ⌘ / Ctrl 悬停链接时的手形与鼠标模式下的指针优先于程序的设置。

两个新设置都有默认值，旧的 `settings.json` 照常读取。不涉及存储与网络连接，`PRIVACY.md` 不变。

## 暂不排期（P2）

有实际需要时再排期：

- 自定义快捷键：快捷键已集中在 `lib/keymap.ts` 的一张表里（见 [ARCHITECTURE.md](ARCHITECTURE.md)「快捷键、菜单与侧栏」），自定义只是在表上叠加用户的覆盖项。`settings.json` 只保存改动过的动作（`"shortcuts": { "find": ["Ctrl+Shift+F"] }`），旧配置照常读取，"恢复默认"即删除覆盖项；设置里 Keyboard Shortcuts 一节的每行可以录制新按键，显示与其他动作的冲突，在 macOS 以外的平台上占用 shell 需要的键（不带 Shift 的 Ctrl+字母）时提示终端将收不到它；成组的动作（⌘1–⌘8、方向键切换窗格）只允许改修饰键；菜单的文字与按键由 M28 的统一定义得出，改快捷键后 Windows 的菜单与 macOS 的原生菜单都要随之重建。
- Microsoft Store 自动提交：现在每个版本在 Partner Center 手动上传（见 [DEVELOPMENT.md](DEVELOPMENT.md)「发布」）。可以用 Store 提交 API（`msstore` CLI）在 release workflow 里提交，需要把 Entra ID 应用的凭据配成 secret。
- 关键字高亮（如 `ERROR` 标红）：需在 xterm.js 的渲染层用 decoration 实现，不能往输出流里插入颜色序列（会破坏全屏程序）。
- X11 转发（macOS 上还要依赖 XQuartz）。
- Linux 上 GNOME 以外的桌面：KDE 的闲置时间（`org.freedesktop.ScreenSaver.GetSessionIdleTime`）、X11 的闲置时间（XScreenSaver 扩展）、窗口按钮按各自主题的样式；没有 polkit 认证代理时用 PAM 在锁定界面输入密码。Flatpak 不在计划内（Secret Service 与 polkit 都要经 portal）。

## 不做

- Xshell 的脚本（VBS / JScript / Python）：快速命令和登录后命令能覆盖大多数场景。
- 独立的密钥代理（Xagent）：使用系统的 ssh-agent / Pageant / Windows OpenSSH agent。
- 云同步服务：用导出文件配合用户自己的同步盘。
- 在标签里临时切换字符编码：编码在会话配置中设置。
- 探测远端支持的终端类型：SSH 申请 pty 时只发出 TERM 字符串，协议里没有确认；要探测只能先开 exec channel 在远端运行 `infocmp`，每次连接多一次往返，网络设备、Windows OpenSSH 上也没有这类工具。ZShell 模拟的是 xterm，`xterm-256color` 几乎处处可用。
- iTerm2 的 OSC 1337：一组私有命令，报告当前文件夹已有 OSC 7，其余依赖 iTerm2 自己的概念（profile、badge、用户变量），内联图片 `File=` 还会往下载文件夹写文件，远端程序抢焦点的 `StealFocus` 不应支持。
- kitty 的剪贴板协议（OSC 5522）：多种 MIME 类型与读取授权，几乎只有 kitty 自己的工具在用，剪贴板插件也只有纯文本与图片。
- 层级上下文标记（OSC 3008）：systemd 257 起 `run0`、容器等发出的上下文开始 / 结束标记（用户、主机、容器），发出它的程序还很少，服务器发行版的 systemd 大多不到 257。

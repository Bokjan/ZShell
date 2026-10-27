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
| **M42 终端与系统集成** ✅ | 远端程序经 OSC 序列写入剪贴板（52）、报告进度（9;4）、发出通知（9、777、99）、设置鼠标指针（22）（3.9.0） |
| **M43 kitty 键盘协议** | 随 xterm.js 升级打开 kitty 键盘协议，远端的 Claude Code、Cursor CLI、Codex 等能分清 Shift+Enter 与 Enter（等 xterm.js 带上它的正式版） |
| **M44 终端图片** | 加载 xterm.js 的图片插件，远端程序可在终端里显示图片（kitty 图形协议、Sixel、iTerm2 内联图片）；与 M43 同在下一个 xterm.js 正式版之后，重点是性能 |
| **M45 连接期间阻止睡眠** ✅ | 有远程会话连着、正在连接或等待重连时阻止系统闲置睡眠：关、仅接通电源时、始终；熄屏、锁屏、合盖与手动睡眠照旧（3.12.0） |
| **M46 收起会话列表** ✅ | 收起左侧的会话列表，标签栏、面板与状态栏不变：开关按钮在标题栏左侧，两种状态下位置不变；应用菜单「显示」里一项，⌃⌘S / Ctrl+Shift+B；收起时搜索会话与新建会话先展开列表（3.13.0） |
| **M47 面板收发指示** ✅ | 文件面板的浏览通道与传输、端口转发面板的全部规则与每条规则显示上下行指示灯，传输与转发另有实时速度；状态栏与面板的悬停提示给出连接编号（含本机端口）与通道号。收发改由后端按通道统计，终端的计数含 ZMODEM（3.14.0） |
| **M48 文件面板后退与前进** ✅ | 文件面板记住去过的文件夹（含跟随终端时进入的），用鼠标侧键、⌘[ / ⌘]（macOS）或 Alt+← / Alt+→ 后退与前进；工具栏不加按钮（3.16.0） |
| **M49 快捷键随键盘布局** ✅ | 字母与标点快捷键按键帽上的字符匹配（AZERTY、Dvorak、日文键盘等），非拉丁布局退回美式位置；macOS 菜单栏不再自动改写快捷键。Windows / Linux 的拆分窗格改为 Ctrl+Shift+S / Ctrl+Shift+D（Tabby 的键），原来的 Alt+Shift+= / - 照样可用（3.16.0） |

### M43 kitty 键盘协议的设计要点

传统编码里 Shift+Enter、Ctrl+Enter 与 Enter 都是 `\r`，Ctrl+I 与 Tab 也分不开，远端的 agent 只能靠 Option / Alt+Enter（`ESC \r`）、Ctrl+J 或 `\` 加 Enter 换行。[kitty 键盘协议](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)让程序用 `CSI > flags u` 声明自己能理解增强编码，之后 Shift+Enter 发 `CSI 13;2u`；没有声明的程序照旧收到传统编码。Ghostty、kitty、foot、iTerm2、Alacritty、Windows Terminal 1.25 与 VS Code 都已支持。

- **时机**：xterm.js 的实现（[xtermjs/xterm.js#5600](https://github.com/xtermjs/xterm.js/pull/5600)，2026-01-10 合入，选项 `vtExtensions.kittyKeyboard`，默认关）只在 6.1.0-beta 里，PR 挂在 7.0.0 里程碑下，没有日期。不提前用 beta：等正式版发布后，和 [TODO.md](TODO.md) 里几条等 xterm.js 的补丁、绕过一起升级，重做或去掉 `patches/` 里的补丁。升级时还要看输入法的问题：[xtermjs/xterm.js#6112](https://github.com/xtermjs/xterm.js/issues/6112)（程序打开协议后，输入法一次只提交一个字时丢字，在 agent 里打中文正好碰到；修复 [#6186](https://github.com/xtermjs/xterm.js/pull/6186) 未合入）没修好就照 #6186 打补丁，Linux 上还有 fcitx5 的 [#6036](https://github.com/xtermjs/xterm.js/issues/6036) 要实测。
- **程序怎么用**：Claude Code 2.1.269 起先发 `CSI ? u` 查询，有回应才 push flags 5，所以经 SSH、没有 `TERM_PROGRAM` 时也能用；Cursor CLI 不查询，启动时直接 push flags 1，挂起和退出时 pop；Codex 直接 push flags 7，含按键松开事件。xterm.js 实现了查询、push / pop / set、主屏与备用屏各一个栈，编码按协议的各级 flags。
- **ZShell 要接的地方**：
  - 设置：终端设置里加开关，默认开（只在程序请求时生效），输入法等问题出现时用户能关掉；旧的 `settings.json` 照常读取。
  - `lib/terminalReports.ts`：查询的回应 `CSI ? flags u` 要算作终端回报，否则会同步到其他窗格，也会被当成输入了命令。
  - 同步输入：现在把源窗格编码好的字节原样转发，源窗格的程序打开了协议而目标窗格没有时，目标的 shell 会收到 `CSI 99;5u` 而不是 Ctrl+C。目标窗格不在协议模式时，在 `lib/` 里把 CSI u 解回传统编码再发（纯函数，单元测试），松开事件丢掉。每个窗格的协议状态公开 API 不给，从 xterm.js 内部可选链读取，或注册不拦截的 CSI 处理器自己跟踪，升级时确认。
  - 不用改的：会话结束时后端发的软复位（`CSI ! p`，`SessionSink::reset_modes`）在新版 xterm.js 里也清掉协议状态，重连后 shell 不会收到 CSI u；程序崩溃没来得及 pop 时，右键菜单的"重置"能恢复。撰写栏发送的是文本，不经按键编码。
- **测试**：临时 sshd 上跑 Cursor CLI 与 Claude Code，Shift+Enter 换行、Enter 发送；macOS 上用拼音输入法往里打中文（含 Shift 打的标点）；Option 作 Meta 开与关；Windows 的 AltGr；Linux 的 fcitx5；同步输入时一个窗格跑 agent、一个是普通 shell，Ctrl+C 与方向键都正常。
- **不含**：远端 tmux 里的程序（见下文「暂不排期」的 modifyOtherKeys）；Windows 本地终端的 win32-input-mode（同一批 PR 里的 `vtExtensions.win32InputMode`，让 ConPTY 拿到完整按键，VS Code 那边还是实验选项）。

不涉及存储与网络连接，`PRIVACY.md` 不变。

### M44 终端图片的设计要点

yazi 的预览、`timg` / `chafa`、Neovim 的图片插件、matplotlib 的终端后端等经这三种协议显示图片。`@xterm/addon-image`（MIT）的 0.10.0-beta 在 Sixel 与 iTerm2 内联图片之外加上了 kitty 图形协议，与 xterm.js 的下一个正式版配套发布；现在 ZShell 没有加载它，图片序列被丢掉。

- **插件的现状**（0.10.0-beta.310）：kitty 部分只支持把数据直接放在序列里（`t=d`，经 SSH 本来也只能这样），不支持文件、共享内存、动画，一张图片只能放在一处；iTerm2 的非内联文件（`inline=0`）直接丢掉，不会写文件，与下文「不做」里 OSC 1337 的理由不冲突。
- **性能是这个里程碑的重点**，每一项都要实测，不合格就收紧限制或不做：
  - 内存：图片以 RGBA 存放（每像素 4 字节），插件默认每个终端缓存 128 MB（`storageLimit`，满了按先进先出丢掉旧图、留占位块），单张上限约 1600 万像素（`pixelLimit`），Sixel 解码器还可能常驻 `pixelLimit` × 4 字节。默认值乘上 ZShell 常开的十几个窗格是不可接受的：每个终端的上限要小得多（或按全部终端合计一个预算，后台终端先丢），离开备用屏、清屏、重置时随文本一起释放。用 M39 那次排查的方法看渲染进程内存：开 10 个窗格各显示一批图片、来回切换，再关掉，内存应回落。
  - CPU：base64 与 Sixel 解码在主线程（wasm），大图或连续输出（预览器快速翻页、`timg` 播放动画）时测按键延迟与输出吞吐，不能卡住打字；序列大小有上限（Sixel 25 MB、iTerm2 与 kitty 20 MB），可以再调小。
  - GPU 与绘制：图片画在终端上方单独的 2D 画布上（`drawImage`），和 WebGL 渲染器的画布叠在一起；后台终端交出 WebGL、改用 DOM 渲染器（`lib/webgl.ts`）时图片层也要跟着暂停，不能在隐藏页里重画。测滚动带图片的长输出时的帧率与 GPU 占用，以及切换标签时有没有空白帧或残影（TODO 里切换标签空白一帧那一条的录屏方法）。
- **ZShell 要接的地方**：设置里一个开关（默认开），可以再给一个内存上限；插件默认打开 `CSI 14 t` / `16 t` 等像素尺寸回报（程序据此算图片大小），回报已经在 `lib/terminalReports.ts` 的范围内，不会同步到其他窗格；窗格移到其他窗口时靠 serialize 插件重画，图片不在快照里，到新窗口后丢失，要看是否可以接受；文本日志用 vte 解析，APC 与 DCS 里的图片数据不会写进去。
- **测试**：临时 sshd 上用 `timg`、`chafa`、`kitten icat`、yazi 显示图片，Sixel 与 kitty 各一遍；窗口缩放与改字号后图片位置；分屏、同步输入、读屏模式下不出错；macOS、Windows、Linux 都看一遍内存与 GPU。

插件与它带的 `xterm-wasm-parts` 都是 MIT，加入后照常跑 `pnpm licenses:generate`。不涉及存储与网络连接，`PRIVACY.md` 不变。

## 暂不排期（P2）

有实际需要时再排期：

- 自定义快捷键：快捷键已集中在 `lib/keymap.ts` 的一张表里（见 [ARCHITECTURE.md](ARCHITECTURE.md)「快捷键与菜单」），自定义只是在表上叠加用户的覆盖项。`settings.json` 只保存改动过的动作（`"shortcuts": { "find": ["Ctrl+Shift+F"] }`），旧配置照常读取，"恢复默认"即删除覆盖项；设置里 Keyboard Shortcuts 一节的每行可以录制新按键，显示与其他动作的冲突，在 macOS 以外的平台上占用 shell 需要的键（不带 Shift 的 Ctrl+字母）时提示终端将收不到它；成组的动作（⌘1–⌘8、方向键切换窗格）只允许改修饰键；菜单的文字与按键由 M28 的统一定义得出，改快捷键后 Windows 的菜单与 macOS 的原生菜单都要随之重建。
- Microsoft Store 自动提交：现在每个版本在 Partner Center 手动上传（见 [DEVELOPMENT.md](DEVELOPMENT.md)「发布」）。可以用 Store 提交 API（`msstore` CLI）在 release workflow 里提交，需要把 Entra ID 应用的凭据配成 secret。
- 关键字高亮（如 `ERROR` 标红）：需在 xterm.js 的渲染层用 decoration 实现，不能往输出流里插入颜色序列（会破坏全屏程序）。
- X11 转发（macOS 上还要依赖 XQuartz）。
- modifyOtherKeys（`CSI > 4;2 m`）：tmux 不转发 kitty 键盘协议，只向外层终端要 modifyOtherKeys（终端名不在它认得的表里时，用户还要在 tmux 配置里加 `terminal-features 'xterm*:extkeys'`），所以 M43 之后远端 tmux 里的 agent 仍分不清 Shift+Enter。xterm.js 的 master 也没有实现，要自己在 `attachCustomKeyEventHandler` 里编码（`CSI 27;2;13~`）。tmux 自己的 kitty 支持（tmux#5615）合入后可能就不需要了。
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

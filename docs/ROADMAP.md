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
| **M26 应用锁** ✅ | 用 Touch ID / Windows Hello 锁定应用：启动、闲置、系统锁屏或睡眠时，以及手动 | |
| **M27 多窗口** ✅ | 多个窗口各有自己的标签；标签可以从菜单或拖拽移到新窗口或其他窗口，连接不断开 | |
| **M28 应用菜单** ✅ | Windows 上由标题栏的图标与名称打开应用菜单；菜单与 macOS 原生菜单由同一份定义生成 | |
| **M29 检查更新** ✅ | 定期查询 GitHub Release，有新版本时提示（Store 版不检查） | |
| **M30 Linux** ✅ | GNOME + Wayland：自绘标题栏与跟随系统的主题、拖出文件、polkit 应用锁、单实例；deb 与 rpm | |
| **M31 翻译** | 语言设置；首批简体中文、日语、德语、法语 | — |
| **M32 Microsoft Store** | 打包 MSIX 上架 Microsoft Store | — |
| **M33 自定义快捷键** | 在设置里修改快捷键 | P2 |

### M31 翻译

首批四种语言：简体中文（`zh-Hans`）、日语（`ja`）、德语（`de`）、法语（`fr`）。架构已在 i18n 阶段接好（见 [I18N.md](I18N.md)），这里要补的是语言的选择与协商、两端语言包的完整性检查、系统自带文本，以及各语言的排版。

**语言设置**

- `settings.json` 增加 `language`：`"system"`（默认）或一个语言代码。用字符串而不是 Rust 枚举：降级到旧版本或手改出未知值时，枚举会让整个 `settings.json` 解析失败并被移到一旁，字符串则在 `normalize` 里回到 `"system"`。
- 设置的 General 一节加 Language 下拉框：各语言用自己的名字（English、简体中文、日本語、Deutsch、Français），不随界面语言翻译；"跟随系统"一项附上它当前对应的语言（"跟随系统（日本語）"）。
- 立即生效，不重启：i18next 切换语言后组件重新渲染；macOS 的菜单栏因 `useNativeMenu` 依赖 `t` 而重建；其他窗口经 `config-changed` 收到设置变化后各自切换。已经写进终端的提示、已写入的日志保持原来的语言。
- 支持的语言列表只在后端 `i18n.rs` 的 `SOURCES` 里维护，经 `constants()` 生成到 `bindings.ts`，前端的下拉框和 `resources` 都以它为准（`resources` 的 key 与它不一致时测试失败）。

**语言协商移到后端**

现在前端按 `navigator.languages` 协商，再用 `set_locale` 告诉后端。改为后端唯一决定：

- 后端启动时读 `settings.json` 的 `language`；`"system"` 时用 `sys-locale` 的 `get_locales()`（系统的首选语言列表，按顺序找第一个支持的），`sys-locale` 从 macOS 专用依赖改为全平台依赖。设置保存时 `SettingsStore` 顺带更新当前语言，`set_locale` 命令删去。
- 前端启动时先取后端解析出的语言（新命令返回当前语言、"跟随系统"对应的语言，以及下文"排版与字体"的 CJK 偏好）再初始化 i18next，`detectLanguage()` 删去。
- 好处：后端在任何页面加载之前就是对的语言（应用锁的 Touch ID / Windows Hello 提示、菜单栏）；不依赖 WebView 的 `navigator.languages`（WKWebView 在应用未声明本地化时可能只报 `en-US`，WebView2 报的是浏览器界面语言）。
- 中文按文字而非地区匹配：`zh`、`zh-CN`、`zh-SG`、`zh-Hans-*` → `zh-Hans`；`zh-TW`、`zh-HK`、`zh-MO`、`zh-Hant-*` 不回退到简体，继续看列表里的下一个语言，最后是英语（繁体用户可以在设置里手动选简体中文）。其余语言按现有规则去掉子标签匹配（`de-AT` → `de`，`fr-CA` → `fr`）。

**语言包的完整性**

- 每种语言必须完整翻译，缺 key 让测试失败。缺失时虽然会回退英语，但界面会中英混杂；新增英语文案时在同一个提交里补齐所有语言（`CLAUDE.md` 的 Language 一节随之写明）。
- 前端测试（`locales.test.ts`）对每种语言检查：key 与 `en.json` 一致（复数 key 去掉后缀后比较）；每个值的 `{name}` 占位符集合与英语相同；复数 key 齐全 `Intl.PluralRules(lang)` 的所有类别。最后一条是必须的：法语有 `one` / `many` / `other`，缺 `_many` 时 i18next 回退到英语的 `_other`（已验证：`1000000` 显示成 "1000000 files"）；中文、日语只有 `_other`，德语 `_one` / `_other`。
- 后端测试同样检查每种语言的 key 集合与占位符（后端没有复数）。
- i18next 的类型只以 `en.json` 为准，其他语言不进类型检查，靠上面的测试。

**系统与平台自带的文本**

- macOS 菜单栏：`PredefinedMenuItem` 不传文字时用的是 muda 写死的英语（About、Hide、Undo、Copy……），改为全部传 `t!` 的译文。AppKit 自己加的项（服务菜单的内容、"开始听写"、"表情与符号"）、帮助菜单的搜索框，以及文件对话框、Touch ID 对话框的按钮，跟随系统语言与应用声明的本地化取交集。
- macOS 加 `src-tauri/Info.plist`（Tauri 会合并进包里）声明 `CFBundleLocalizations`（en、zh-Hans、ja、de、fr）。没有它 macOS 认为应用只有英语，上面那些系统文本永远是英语，系统设置里"按应用设置语言"也不会列出 ZShell。界面语言与系统语言不同时，系统文本跟随系统，不去改 `AppleLanguages`。
- Windows 与 Linux 的文件对话框、Windows Hello 对话框的按钮跟随系统语言；Windows 的应用菜单由页面绘制，随界面语言。
- Linux 的 polkit 策略文件（`org.boyin.zshell.policy`）给 `description` / `message` 加各语言的 `xml:lang` 版本（polkit 按认证代理所在会话的语言选择，与 ZShell 的设置无关）。
- 不在本里程碑：安装程序（NSIS / MSI，只出现一次，保持英语）、`.desktop` 文件的本地化名称（名称就是 ZShell）、MSIX 清单的 `<Resource Language>` 与 Store 页面的多语言（归 M32；清单声明的语言可能要求 Partner Center 提供对应语言的商店页面，上架时再定）。README、CHANGELOG 保持英语，README 的 Features 写一句支持的语言。

**排版与字体**

- 字体栈分两段：拉丁字体（`-apple-system`、`Segoe UI`、`system-ui`）各语言共用，德语、法语的变音字母它们都覆盖；CJK 回退字体的顺序按语言不同。汉字统一编码后同一个码位在中、日字体里字形不同（"直""骨""写"），现在的栈把 `PingFang SC`、`Microsoft YaHei` 写死在前面，日语界面会显示成中文字形。
- CJK 回退的顺序由"CJK 偏好"决定：界面是中文或日语时就是界面语言；界面是英语、德语、法语时，取系统首选语言列表里第一个 CJK 语言（后端协商时一并算出），都没有则用简体中文（现在的行为）。这样选了英语界面的日本用户，会话名、文件名里的汉字仍是日文字形。
  - 简体中文：`PingFang SC`、`Microsoft YaHei UI`、`Noto Sans CJK SC`
  - 日语：`Hiragino Sans`、`Yu Gothic UI`、`Meiryo UI`、`Noto Sans CJK JP`
- 实现：`:root` 上的 `--font-ui` 变量，按 CJK 偏好设在 `<html>` 的一个 data 属性上切换（不用 `:lang()`，因为英语界面也可能要日文顺序）。终端字体的 CJK 回退（`terminalSchemes.ts`）用同一个偏好，等宽的对应字体为 `Noto Sans Mono CJK SC` / `JP`。繁体中文与韩语也各有一组回退字体（系统首选语言里有它们时用）。西里尔、希腊等拼音文字由拉丁段的系统字体覆盖，不需要回退组；但拉丁段必须在 CJK 段之前，CJK 字体也带西里尔字母，排在前面会用它的字形。保加利亚语、塞尔维亚语的本地字形（б、д、г 等）由字体的 OpenType `locl` 特性提供，浏览器按 `<html lang>` 自动启用。
- 德语文案普遍比英语长 30% 左右、复合词长：逐一检查设置对话框的标签列、工具栏与面板按钮、状态栏、右键菜单、480px 最小窗口宽度下的布局；长词靠 `hyphens: auto`（依赖 `lang`）而不是截断。`text-transform: uppercase` 的几处对 CJK 无影响，德语的 `ß` 由浏览器正确处理。
- 快捷键的显示（`keymap.ts` 的 `Ctrl+Shift+E`）：德语 Windows 与键盘上写作 `Strg` / `Umschalt`，法语写作 `Ctrl` / `Maj`；修饰键与命名键（`Insert`、`Left`……）的显示名进语言包，匹配逻辑不变。macOS 的符号（⌃⌥⇧⌘）不受影响。

**翻译约定**

- 各语言的风格写进 I18N.md：
  - 简体中文：用"你"或省略人称；中文与英文、数字之间加空格（与 `docs/` 一致）；全角标点，引号用 “”。
  - 日语：消息用です・ます体，标签用名词；全角括号，引号用「」；中英之间不加空格。
  - 德语：用 "Sie"；按钮与菜单用不定式（"Datei hochladen"）；引号用 „…“。
  - 法语：用 "vous"；`:` `?` `!` `;` 前加不换行空格（U+00A0 / U+202F），引号用 « … »；按钮用不定式。
- 术语表（会话、标签页、窗格、端口转发、主机密钥、已知主机、私钥、口令、保险库、主密码、应用锁、快速命令、撰写栏、同步输入、会话日志……）写进 I18N.md，四种语言各一列，翻译与后续新增文案都按它；SSH、SFTP、Telnet、ZMODEM、ProxyJump、ProxyCommand、keyboard-interactive、TERM、Touch ID、Windows Hello 等协议与产品名不译。
- 与 OpenSSH 一致、用户可能依赖字面形式的提示保持英语（见 I18N.md「后端」），`(yes/no)` 的回答词不译。
- 简体中文由维护者审阅；日语、德语、法语没有母语审阅者，README 里说明译文问题可以在 issue 里报告。

**验证**

- 端到端：四种语言各走一遍主要界面（会话列表、新建会话对话框、设置各节、SFTP 面板与传输列表、端口转发、主机密钥确认、应用锁、导出导入），德语重点看布局，日语看字形；切换语言后其他窗口与 macOS 菜单栏随之变化。
- macOS 上分别把系统语言设为中文、日文，确认"跟随系统"的结果、服务菜单与文件对话框的语言，以及系统设置里按应用选语言后的效果。

### M32 Microsoft Store

已完成：release workflow 打出 x64 与 ARM64 的 `.msixbundle`（见 [DEVELOPMENT.md](DEVELOPMENT.md)「发布」）、S 模式下隐藏本地终端、随包附上第三方许可证声明。还剩：

- 打包后实测：本地终端（ConPTY 启动 PowerShell）、OpenSSH agent 命名管道与 Pageant、凭据管理器里的密码、用本地编辑器编辑与拖出、会话日志写入「文档」、自绘标题栏与贴靠布局。本地测试用 `Add-AppxPackage -Register AppxManifest.xml` 注册解包后的目录（需开启开发者模式），不用签名。
- 临时文件："用本地编辑器编辑"与拖出到资源管理器的临时文件写在 `%TEMP%`，它在 `AppData\Local` 下，被文件系统虚拟化重定向后外部编辑器和资源管理器可能看不到。实测若如此，有包身份时（`GetCurrentPackageFullName` 成功）改用包的临时目录 `ApplicationData::Current().TemporaryFolder()`（`…\Packages\<PFN>\TempState`），那是真实路径，外部进程可以访问。
- 上架前提：仓库设为公开（隐私政策与 Issues 的链接要能打开）。ZShell 本身不开源，条款见根目录的 `LICENSE.md`：发布的安装包可免费用于任何用途，源码可以自己研究、构建和修改，但不能分发；原则上不接受 PR，改动先在 issue 里讨论（`CONTRIBUTING.md`）。
- 提交：先在 Partner Center 手动上传每个版本的 `.msixbundle`；流程稳定后再考虑用 Store 提交 API（`msstore` CLI，需要把 Entra ID 应用的凭据配成 secret）自动提交。
- 商店页面：描述、截图、年龄分级在 Partner Center 填写；提交时说明使用 `runFullTrust` 的理由（完整的桌面应用，需要启动本地 shell、访问 SSH agent 等）。隐私政策写明应用不收集数据、配置只保存在本机，放在仓库里，用 GitHub 链接。

### M33 自定义快捷键

快捷键已集中在 `lib/keymap.ts` 的一张表里（见 [ARCHITECTURE.md](ARCHITECTURE.md)「快捷键、菜单与侧栏」），自定义只是在表上叠加用户的覆盖项：

- `settings.json` 只保存改动过的动作（`"shortcuts": { "find": ["Ctrl+Shift+F"] }`），旧配置照常读取，"恢复默认"即删除覆盖项。
- 设置里 Keyboard Shortcuts 一节的每行可以录制新按键：显示与其他动作的冲突；在 macOS 以外的平台上占用 shell 需要的键（不带 Shift 的 Ctrl+字母）时提示终端将收不到它。
- 成组的动作（⌘1–⌘8、方向键切换窗格）只允许改修饰键。
- 菜单的文字与按键由 M28 的统一定义得出，改快捷键后 Windows 的菜单与 macOS 的原生菜单都要随之重建。

## 暂不排期（P2）

有实际需要时再排期：

- 关键字高亮（如 `ERROR` 标红）：需在 xterm.js 的渲染层用 decoration 实现，不能往输出流里插入颜色序列（会破坏全屏程序）。
- X11 转发（macOS 上还要依赖 XQuartz）。
- 主机证书（`@cert-authority`）：用 CA 管理主机的环境不必逐台确认主机密钥。现在主机证书被当作普通公钥；要先确认 russh 是否支持证书形式的主机密钥，不支持则需要上游补丁。
- Linux 上 GNOME 以外的桌面：KDE 的闲置时间（`org.freedesktop.ScreenSaver.GetSessionIdleTime`）、X11 的闲置时间（XScreenSaver 扩展）、窗口按钮按各自主题的样式；没有 polkit 认证代理时用 PAM 在锁定界面输入密码。Flatpak 不在计划内（Secret Service 与 polkit 都要经 portal）。
- macOS 上 VoiceOver 开启时自动打开终端读屏模式：WebView 拿不到这个状态，需要后端查询（`NSWorkspace.isVoiceOverEnabled`）；现在是设置里的手动开关。

## 不做

- Xshell 的脚本（VBS / JScript / Python）：快速命令和登录后命令能覆盖大多数场景。
- 独立的密钥代理（Xagent）：使用系统的 ssh-agent / Pageant / Windows OpenSSH agent。
- 云同步服务：用导出文件配合用户自己的同步盘。
- 在标签里临时切换字符编码：编码在会话配置中设置。
- 探测远端支持的终端类型：SSH 申请 pty 时只发出 TERM 字符串，协议里没有确认；要探测只能先开 exec channel 在远端运行 `infocmp`，每次连接多一次往返，网络设备、Windows OpenSSH 上也没有这类工具。ZShell 模拟的是 xterm，`xterm-256color` 几乎处处可用。

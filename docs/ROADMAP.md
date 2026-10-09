# ZShell 产品规划

本文记录做什么、为什么、先做哪个，以及尚未实现的功能的设计要点。功能实现后，跨模块的设计写进 [ARCHITECTURE.md](ARCHITECTURE.md)，模块内的细节写在代码注释里，并在下方里程碑表中标记 ✅。

## 定位

- 作为 Xshell 的替代，个人使用，macOS 与 Windows 体验一致。
- 优先补齐每天高频使用的操作（剪贴板、标签页、会话列表），再补 Xshell 的效率功能；不追求逐项对齐 Xshell。
- 行为尽量与 OpenSSH 一致（认证顺序、known_hosts、ssh_config 语义），减少"在 ZShell 里能连、命令行不能连"或反过来的情况。

## 现状（2026-10-09，M14 之后）

### 已具备

- SSH：密码 / 私钥 / agent / keyboard-interactive / 自动认证，known_hosts，密码存钥匙串，多跳 ProxyJump，keepalive 与带退避的自动重连。
- 代理：SOCKS5、HTTP CONNECT（可带用户名和密码）与 ProxyCommand，按名称保存、由会话选用，SSH 与 Telnet 都可用；经跳板机时用第一台跳板机的代理；ssh_config 的 ProxyCommand 一并导入。
- Telnet 与串口：Telnet 选项协商（BINARY、ECHO、SGA、TTYPE、NAWS）、可经 SSH 跳板机、保存的用户名和密码在登录提示时自动填入；串口列出系统设备，可设波特率、数据位、校验、停止位、流控，拔出后插回自动重连；两者都可发送 Break。
- 终端：搜索、配色 / 字体 / 光标 / 回滚设置、本地终端（macOS 登录 shell、Windows PowerShell）；剪贴板快捷键、右键菜单、选中即复制、右键粘贴、多行粘贴确认、macOS Option 作为 Meta。
- 标签页：拖拽排序、重命名、复制（复用同一连接）、跟随远端标题、切换快捷键、关闭确认、右键菜单。
- SFTP：浏览、多选与右键菜单、排序 / 过滤 / 隐藏文件开关、上传下载（进度、取消、拖入上传、下载到指定位置）、拖出到 Finder / 资源管理器、面板内拖到文件夹移动、用本地编辑器编辑远端文件（保存即上传）、重命名 / 删除 / 新建 / chmod。
- ZMODEM：终端里的 rz / sz，SSH、Telnet、串口与本地终端都可用，适合没有 SFTP 的堡垒机环境。
- 会话管理：可嵌套的文件夹（拖拽整理）、搜索与快速连接（含 `telnet host`）、右键菜单、复制会话、最近连接、导出 / 导入。
- 会话配置：agent 转发、字符编码（GBK、Big5 等，终端、SFTP 文件名、ZMODEM 文件名都转换）、单会话配色 / 背景色 / 字体、登录后命令、终端类型与环境变量。
- 批量与效率：撰写栏（发送到多个标签、同步输入）、分组的快速命令与命令面板、会话日志（自动 / 手动记录、纯文本或原始格式、按天数清理）。
- 窗口：标签栏画在窗口标题栏里，macOS 保留红绿灯，Windows 自绘窗口按钮（支持 Windows 11 贴靠布局）。
- 端口转发：`-L` / `-R` / `-D`，规则随会话保存、可自动启动，实时状态。
- ssh_config 导入、i18n 架构、macOS / Windows 发布流程。

### 相对 Xshell 的优势（保持）

- 一条连接复用：终端、SFTP、端口转发共用一条 SSH 连接，不像 Xshell + Xftp 那样各连一次、各认证一次。
- 原生支持 macOS（Xshell 没有 Mac 版）。
- ssh_config 导入、自动重连、转发规则实时状态、"自动"认证。

### 主要差距

| 方面 | 差距 | 优先级 |
|---|---|---|
| 布局 | 没有分屏 | P1 |
| 杂项 | 没有 known_hosts 管理界面、应用锁 | P2 |

## 里程碑

按计划顺序排列。翻译、上架 Store 与其他里程碑没有依赖，可以随时提前。

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
| **M15 分屏** | 标签页内左右 / 上下分屏 | P1 |
| **M16 known_hosts 管理** | 查看、搜索、删除主机密钥记录 | P2 |
| **M17 应用锁** | 用 Touch ID / Windows Hello 锁定应用 | P2 |
| **M18 翻译** | 语言设置界面，首批简体中文 | — |
| **M19 Microsoft Store** | 打包 MSIX 上架 Microsoft Store | — |

### M15 分屏

- 标签页内左右 / 上下分屏，每个窗格是独立会话；分屏中的 SSH 窗格可以复用同一条连接。
- 拖动分隔条调整大小，快捷键在窗格间移动焦点。
- 与 M10 的"发送到所有会话"配合：范围增加"当前标签的所有窗格"。
- 需要把标签页模型从"一个标签一个会话"改为"一个标签一棵窗格树"，这也是排在 M10–M12 之后的原因。

### M16 known_hosts 管理

- 设置中列出 `~/.ssh/known_hosts` 的条目（主机、算法、指纹），可搜索、删除；哈希过的主机名只显示为哈希。
- 主机密钥变化时，终端里的提示同时给出删除旧记录的方法（OpenSSH 提示 `ssh-keygen -R`），连接仍然拒绝。

### M17 应用锁

- 启动时、闲置一段时间后、系统锁屏或睡眠后，盖上锁定界面，用系统认证解锁：macOS 用 LocalAuthentication（Touch ID，不可用时退回登录密码），Windows 用 Windows Hello（PIN、指纹、人脸）。会话在锁定期间照常运行。
- 系统没有可用的认证方式时不能开启。
- 只是应用锁，不加密配置文件：密码与口令本来就只在钥匙串里；真正的加密需要把密钥放进要求生物识别的钥匙串项，macOS 上需要开发者证书签名。
- 开工前先用原型确认 ad-hoc 签名下 LocalAuthentication 可用。

### M18 翻译

语言设置界面，首批简体中文；做法见 [I18N.md](I18N.md)「新增一种语言」。

### M19 Microsoft Store

- 形式：MSIX，作为 full trust 桌面应用打包（`runFullTrust`，不进 AppContainer）。提交后由 Store 用微软的证书重新签名，不需要自己的代码签名证书；安装、更新、卸载由 Store 负责，应用本身没有自动更新，不用改。不选 EXE / MSI 上架：那条路要求安装包用受信任 CA 的证书签名，且 Store 不负责更新。GitHub Release 的 NSIS / MSI / 单独 exe 照旧，与 Store 版可以同时安装。
- 打包：Tauri 不产出 MSIX。release workflow 增加一个 Store job，`tauri build --no-bundle` 之后由 `scripts/package-msix.ps1` 用 Windows SDK 的 `makeappx` 把 `zshell.exe`、图标与 `AppxManifest.xml` 打成 `.msix`，上传到 release（不签名，只用于提交 Store）。清单模板放在 `src-tauri/msix/`，版本号由 `Cargo.toml` 的 `X.Y.Z` 填成 `X.Y.Z.0`（Store 要求第四段为 0，且每次提交的版本必须更高）。
- 架构：x64 与 ARM64（`aarch64-pc-windows-msvc`，在 x64 runner 上交叉编译），用 `makeappx bundle` 合成一个 `.msixbundle` 提交。
- 身份：清单中 `Identity` 的 `Name`、`Publisher` 与 `PublisherDisplayName` 照抄 Partner Center 的「产品标识」页，必须完全一致，否则上传会被拒。这些不是机密，直接提交到仓库。
- 图标：`tauri icon` 已生成 `Square44x44Logo`、`Square150x150Logo`、`StoreLogo`，清单直接引用。任务栏用的 `targetsize-*` 无底板变体以后按需补。
- 文件系统虚拟化：保持 MSIX 的默认行为。应用在 `AppData` 下新建的文件写到包的私有目录（`%LOCALAPPDATA%\Packages\BoyinChen.ZShell_0mrn21pkbrd8j\LocalCache`），卸载时删除；已有的文件原地读写。因此机器上已有 GitHub 版的配置时，Store 版直接沿用它；全新安装的配置只有 Store 版看得到。关闭虚拟化需要受限能力 `unvirtualizedResources`，微软说明它只给特定的游戏使用，不考虑。
- 临时文件："用本地编辑器编辑"与拖出到资源管理器的临时文件写在 `%TEMP%`，它也在 `AppData\Local` 下，被重定向后外部编辑器和资源管理器可能看不到。实测若如此，有包身份时（`GetCurrentPackageFullName` 成功）改用包的临时目录 `ApplicationData::Current().TemporaryFolder()`（`…\Packages\<PFN>\TempState`），那是真实路径，外部进程可以访问。
- 系统要求：最低 Windows 10 1809（与本地终端的 ConPTY 要求一致）。WebView2 使用系统自带的运行时（Windows 11 与更新过的 Windows 10 都有），包里不带安装器。
- S 模式：Windows 的 S 模式只能运行 Store 应用，也不允许启动 PowerShell 等命令行程序。已处理：后端用 `WindowsIntegrityPolicy::IsEnabled` 检测，S 模式下前端隐藏新建本地终端的入口（"+" 按钮、快捷键、设置里的自动记录本地终端）；SSH 等其他功能不受影响。
- 打包后实测：本地终端（ConPTY 启动 PowerShell）、OpenSSH agent 命名管道与 Pageant、凭据管理器里的密码、用本地编辑器编辑与拖出、会话日志写入「文档」、自绘标题栏与贴靠布局。本地测试用 `Add-AppxPackage -Register AppxManifest.xml` 注册解包后的目录（需开启开发者模式），不用签名。
- 上架前提：仓库设为公开（隐私政策与 Issues 的链接要能打开）；第三方许可证声明已补上（见下条）。ZShell 本身不开源，README 中声明保留所有权利，安装包免费使用。
- 第三方许可证声明：已完成。构建时生成一份合并的声明（Rust 依赖用 `cargo-about`，前端只收运行时依赖），随前端产物内嵌进 exe，单独 exe 与 MSIX 都带着；设置的「关于」一节显示版本号并能查看这份声明。做法见 [DEVELOPMENT.md](DEVELOPMENT.md)「发布」。
- 提交：先在 Partner Center 手动上传每个版本的 `.msixbundle`；流程稳定后再考虑用 Store 提交 API（`msstore` CLI，需要把 Entra ID 应用的凭据配成 secret）自动提交。
- 商店页面：描述、截图、年龄分级在 Partner Center 填写；提交时说明使用 `runFullTrust` 的理由（完整的桌面应用，需要启动本地 shell、访问 SSH agent 等）。隐私政策写明应用不收集数据、配置只保存在本机，放在仓库里，用 GitHub 链接。

## 暂不排期（P2）

有实际需要时再排期：

- 关键字高亮（如 `ERROR` 标红）：需在 xterm.js 的渲染层用 decoration 实现，不能往输出流里插入颜色序列（会破坏全屏程序）。
- X11 转发（macOS 上还要依赖 XQuartz）。

## 不做

- Xshell 的脚本（VBS / JScript / Python）：快速命令和登录后命令能覆盖大多数场景。
- 独立的密钥代理（Xagent）：使用系统的 ssh-agent / Pageant / Windows OpenSSH agent。
- 云同步服务：用导出文件配合用户自己的同步盘。
- 在标签里临时切换字符编码：编码在会话配置中设置。

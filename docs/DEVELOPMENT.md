# ZShell 开发指南

## 环境

- Rust stable、Node.js、pnpm（CI 使用 Node 26、pnpm 12）。
- macOS 上开发；Windows 专用代码（ConPTY、OpenSSH agent 命名管道 / Pageant）本地无法编译，依赖 CI 检查与 Windows 上的手动测试。

## 常用命令

```bash
pnpm install
pnpm tauri dev                          # 开发模式运行
pnpm build                              # 前端类型检查（tsc）并构建
pnpm test                               # 前端单元测试（vitest，测试文件与被测模块放在一起：*.test.ts）
pnpm tauri build --debug --bundles app  # 打包 debug 版 .app，用于端到端测试
pnpm tauri build                        # 打包 release 版
```

后端检查在 `src-tauri/` 下运行：

```bash
cargo clippy --all-targets -- -D warnings
cargo test
```

ZMODEM 的测试除了自己的收发互测，还与 lrzsz 的 `lsz` / `lrz` 对传（含经由伪终端），未安装 lrzsz 时跳过（`brew install lrzsz`；CI 上没有）。

`cargo test` 中的 `catalog_covers_all_keys_in_sources` 会检查后端源码用到的消息 key 都在英语语言包中（见 [I18N.md](I18N.md)）。

`cargo test` 中的 `bindings_are_up_to_date` 检查 `src/lib/bindings.ts` 与 Rust 类型一致；改了与前端交换的类型或错误码后，在 `src-tauri/` 下重新生成并提交：

```bash
UPDATE_BINDINGS=1 cargo test bindings
```

## 约定

- **语言**：`docs/` 用中文，其他一切（代码、注释、界面文案、README、提交信息）用英语，见 [I18N.md](I18N.md)「语言约定」。
- **提交信息**：Conventional Commits（`feat:`、`fix:`、`docs:`、`chore:`、`refactor:`、`ci:`，可带 scope，如 `feat(sftp): …`）；正文不手动折行，每段或每个列表项一行。
- **版本号**：只在 `src-tauri/Cargo.toml` 维护，由 `scripts/release.sh` 修改（规则见下文「发布」）。`tauri.conf.json` 不写 `version`，Tauri 取 Cargo 包版本；`TERM_PROGRAM_VERSION` 用 `CARGO_PKG_VERSION`；`package.json` 为 private 包，不写版本。
- **文档分工**：做什么、先后顺序与未实现功能的设计写在 [ROADMAP.md](ROADMAP.md)；跨模块的设计写在 [ARCHITECTURE.md](ARCHITECTURE.md)；模块内的细节（边界情况、库的坑）写在代码注释里。根目录的 `CLAUDE.md`（英语）是给 AI 助手的项目说明，汇总上述约定；约定变化时同步更新。

## 端到端测试

需要真实 SSH 服务器的功能，用隔离的 HOME 和临时 sshd 测试，不碰自己的 `profiles.json`、`settings.json` 和 `~/.ssh`（known_hosts、config、默认私钥）。

**隔离的 HOME**：在一个临时目录 `$T` 下运行 debug 包里的可执行文件：

```bash
HOME=$T/home src-tauri/target/debug/bundle/macos/ZShell.app/Contents/MacOS/zshell
```

会话配置写在 `$T/home/Library/Application Support/org.boyin.zshell/profiles.json`。注意 WKWebView 的 localStorage（如侧栏宽度）可能不跟随 HOME，会写进正式应用的 WebKit 数据。

**临时 sshd**：用 `ssh-keygen -t ed25519 -N '' -f $T/host_key` 生成主机密钥，配置文件示例：

```
Port 2222
ListenAddress 127.0.0.1
HostKey /path/to/T/host_key
AuthorizedKeysFile /path/to/T/authorized_keys
PidFile /path/to/T/sshd.pid
UsePAM no
StrictModes no
AllowTcpForwarding yes
Subsystem sftp /usr/libexec/sftp-server
LogLevel VERBOSE
```

以 `/usr/sbin/sshd -D -e -f sshd_config` 前台运行，日志直接输出到终端（`LogLevel VERBOSE` 能看到客户端提供了哪些密钥）。测试 ProxyJump 时起两个 sshd（如 2222 作跳板机、2223 作目标）。

**ssh-agent**：Unix socket 路径有长度限制，临时目录太深时用相对路径启动：在 `$T` 下运行 `ssh-agent -a ./a.sock`，并以 `SSH_AUTH_SOCK=./a.sock` 从同一目录启动应用。

**Telnet**：macOS 没有自带 telnetd，用 `scripts/e2e/telnetd.py` 起一个测试服务器（只用 Python 标准库）：

```bash
python3 scripts/e2e/telnetd.py 2323 --home $T/work            # NVT 模式，服务器回显
python3 scripts/e2e/telnetd.py 2324 --binary --no-echo        # 接受 BINARY；不协商回显
```

它在标准输出里记录选项协商（`DO BINARY`、`NAWS (cols, rows)`、`TTYPE IS …`），登录账号是 `alice` / `secret`，登录后在伪终端里运行 `/bin/sh`；收到 Break 时 shell 打印 `BREAK-RECEIVED`。`--no-echo` 用来测登录提示处的本地回显：登录后 shell 的伪终端自己还会回显，所以输入显示两遍，这是服务器的问题。会话配置里填上用户名可以测自动填入；隔离的 HOME 里没有钥匙串，保存的密码只有单元测试覆盖。经跳板机的 Telnet 用临时 sshd 作跳板；要模拟跳板机断线，结束 sshd 为该连接派生的 `sshd-session` 进程（会被收养到 PID 1，按 `ps` 里的 PID 结束，不要用 `pkill -P`）。服务器进程退出时 TCP 正常关闭，ZShell 视为 `exited`，测不出 `lost`。

**代理**：`scripts/e2e/proxy.py` 起一个 SOCKS5 或 HTTP CONNECT 代理（只用 Python 标准库），在标准输出里记录每个请求的目标（按客户端发来的原样，可以看出主机名是否交给了代理解析）和用户名：

```bash
python3 scripts/e2e/proxy.py socks5 1081
python3 scripts/e2e/proxy.py http 8081 --user alice --password secret
```

ProxyCommand 用 macOS 自带的 `nc %h %p`，或一个不存在的命令看 stderr 是否显示在终端里。隔离的 HOME 里没有钥匙串，带用户名的代理每次连接都会在终端里问密码，正好测内联询问与重试。会话与代理可以直接写进 `profiles.json` 与 `proxies.json`；预先写好 `$T/home/.ssh/known_hosts`（`[localhost]:2222 <主机公钥>`）可以省去主机密钥确认。

**串口**：`scripts/e2e/fakeserial.py $T/vserial --home $T/work` 创建一对伪终端，另一端运行 `/bin/sh`（像开发板的串口控制台），设备路径是指向 `/dev/ttysNNN` 的符号链接；会话的设备填 `$T/vserial`。结束脚本相当于拔出设备，再次运行（同一路径）相当于插回，用来测断线重连。伪终端没有波特率，ZShell 对它们不设速率；真实 USB 串口适配器只能在真机上测。

**模拟从 Finder 启动**（没有 `LANG`、只有最小 PATH），用于测试本地终端的登录 shell 与区域设置：

```bash
env -i HOME=$T/home USER=$USER LOGNAME=$USER TMPDIR=$TMPDIR PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  src-tauri/target/debug/bundle/macos/ZShell.app/Contents/MacOS/zshell
```

测试 HOME 中放一个空的 `.zshrc`，否则 zsh 会显示新用户向导。

**ZMODEM**：在临时 sshd 的会话、测试 telnetd 或伪串口的 shell、本地终端里运行 `sz` / `rz`（Homebrew 装在 `/opt/homebrew/bin`）。SSH 会话以真实 HOME 登录，`rz` 收到的文件写在当前目录，先 `cd` 到临时目录。

**非 UTF-8 文件名**：APFS 不接受非 UTF-8 的文件名，在本机的临时 sshd 上造不出 GBK 文件名，SFTP 与 ZMODEM 的文件名转码只有单元测试覆盖；GBK 会话里新建中文名的文件夹会被服务器拒绝，这恰好说明名字已按 GBK 发出。终端的输出和输入可以用 `printf` 写出 GBK 字节的文件、`head -c 4 | xxd` 查看键入的字节来验证。

**只能在真机键盘上验证的**：按键长按重复（合成的按键事件不会自动重复）、输入法组字与候选框位置。

## 图标

贝壳螺旋起笔的"Z"，以块状光标收尾（沙色底、珊瑚色线条）。母版在 `src-tauri/icons/source/`：

- `icon.svg`：满幅，用于 Windows 与各尺寸 PNG。
- `icon-macos.svg`：同一图形按 Apple 网格放置（1024 画布中央 824 的圆角方形，四周留 100 边距）。

两者需同步修改，然后运行 `scripts/generate-icons.sh`：它用 `tauri icon` 重新生成全部图标，再用 macOS 母版覆盖 `icon.icns`，并删掉 CLI 总会生成的移动端图标。

## CI

- `.github/workflows/windows.yml`：推送到 main 和 PR 时在 `windows-latest` 上跑 `pnpm build`、`pnpm test`、`cargo clippy --all-targets -D warnings` 和 `cargo test`，保证 Windows 专用代码能通过编译。
- `.github/workflows/release.yml`：只能手动运行。在版本 tag 上运行时创建草稿 release（见下文「发布」）；在分支上运行时只打包，产物作为 workflow artifacts。勾选 "Microsoft Store only" 时只打 Store 包，省去 macOS 与其他 Windows 包。
- `.github/dependabot.yml`：每周检查 npm、Cargo 和 GitHub Actions 的依赖更新。各生态的 minor / patch 更新合并为一个 PR，Tauri（及 xterm.js、russh）的相关包各自成组；major 更新单独成 PR。提交前缀为 `chore(deps)` / `ci`，不进更新日志。Dependabot 的 PR 会触发 Windows CI。

## 发布

**版本号**：语义化版本 `X.Y.Z`，从 1.0.0 开始。
- 按语义化版本选择升哪一位：功能里程碑或新功能升 minor（1.1.0、1.2.0…），修复和小改进升 patch（1.3.1、1.3.2…）。
- 不按提交升版本。规划涉及 `src/`（前端）或 `src-tauri/`（后端）代码的修改时，先定版本号方案（升到哪个版本、包含哪些修改），确定后再动手；一个版本可以包含多个提交。只改文档、CI 或脚本不需要新版本。
- `profiles.json`、`settings.json` 等配置保持向后兼容：新字段加默认值，旧文件照常读取。放弃兼容要先与维护者商定，升 major，并在更新日志里标为破坏性变更。
- 不用预发布后缀（`-beta.1` 之类）：Windows 的 MSI 只接受数字版本号。

**升版本号与发布分开**：每个版本都打 tag，但不一定发布。git-cliff 以 tag 划分版本，所以没发布的版本也要打 tag 并推送，否则下一版的更新日志会把这一版的提交再算一遍；推送 tag 不会触发构建，要发布时再单独运行 release workflow，之前任何一个已推送的版本都可以补发。

**步骤**（`scripts/release.sh`，需在 main 上且工作区干净）：
1. `scripts/release.sh prepare X.Y.Z`：修改 `src-tauri/Cargo.toml` 中的版本号并更新 `Cargo.lock`，用 git-cliff 在 `CHANGELOG.md` 顶部加上该版本一节（`--unreleased --prepend`，之前各版本已润色的内容不变）。git-cliff 的配置在 `cliff.toml`，收录 `feat` / `fix` / `perf`、`refactor`（列在 "Internal" 下，排在面向用户的变化之后）和破坏性变更，其他类型（`docs`、`ci`、`chore` 等）不出现在更新日志里，因此提交类型要选准。
2. 润色该版本一节：合并同一功能的多条、去掉内部里程碑编号，内容保持英语。来自 issue 的修改在条目末尾署名（`(thanks @name, #12)`），并把此人加进 `THANKS.md`（每人一行，已有的补上 issue 编号）。只有 `docs` / `chore` 等提交的版本，git-cliff 生成的一节可能是空的，手写一句面向用户的说明（`tag` 要求有该版本一节）；"Internal" 下的条目同样润色成读者看得懂的说明。
3. `scripts/release.sh tag`：运行 `pnpm build`、`pnpm test`、clippy 与测试，提交 `chore: release vX.Y.Z` 并打 `vX.Y.Z` tag（之后的参数原样传给 `git commit`，如 `--trailer`）。
4. `git push origin main vX.Y.Z`。只升版本号的话到此为止。
5. 要发布时：`scripts/release.sh publish [X.Y.Z]`（默认当前版本），用 `gh workflow run release.yml --ref vX.Y.Z` 在该 tag 上运行 release workflow。workflow 先校验 tag 与 `Cargo.toml` 版本一致、`CHANGELOG.md` 中有 `## [X.Y.Z]` 一节，再由一个 job 统一创建草稿 release（避免并行 job 重复创建），然后并行打包并上传。发布说明由 `scripts/release-notes.sh` 从 `CHANGELOG.md` 取：不是每个版本都发布，所以包含上一个已发布的 release（不算草稿）之后的全部版本；只有这一个版本时就是该节原文，有多个时每个版本一节、以版本号和日期为标题，新的在前。本地运行 `scripts/release-notes.sh X.Y.Z` 可以预览。各个包：
   - macOS 通用包（`universal-apple-darwin`，`.app` 与 `.dmg`）。
   - Windows NSIS 安装包与 MSI。
   - Windows 免安装的单独 exe（`--no-bundle` + `uploadPlainBinary`，文件名 `ZShell_<版本>_x64_standalone.exe`）。需要系统有 WebView2 运行时；配置仍写在 `%APPDATA%`，不是便携模式。
   - Microsoft Store 用的 `ZShell_<版本>.msixbundle`（x64 与 ARM64，由 `scripts/package-msix.ps1` 用 Windows SDK 的 `makeappx` 打包，清单模板在 `src-tauri/msix/`，不签名）。
6. 检查草稿后手动发布，作为正式版本发布（不勾选 pre-release）。
7. 在 Partner Center 新建提交，上传该版本的 `.msixbundle`。Store 会重新签名并负责用户的更新。

**Microsoft Store**：以 full trust 桌面应用（`runFullTrust`，不进 AppContainer）的 MSIX 上架，由 Store 用微软的证书重新签名，不需要自己的代码签名证书，安装与更新由 Store 负责（应用本身没有自动更新）。不走 EXE / MSI 上架：那条路要求安装包用受信任 CA 的证书签名，且 Store 不负责更新。GitHub Release 的各个包照旧，可以与 Store 版同时安装。文件系统虚拟化保持 MSIX 的默认行为：应用在 `AppData` 下新建的文件写到包的私有目录（`%LOCALAPPDATA%\Packages\BoyinChen.ZShell_0mrn21pkbrd8j\LocalCache`），卸载时删除，已有的文件原地读写；所以机器上已有 GitHub 版的配置时 Store 版直接沿用，全新安装的配置只有 Store 版看得到（关闭虚拟化要用只给特定游戏的受限能力 `unvirtualizedResources`，不考虑）。清单的身份、版本号与最低系统版本见 `src-tauri/msix/AppxManifest.xml` 的注释。

**依赖的许可证**：ZShell 不开源、免费分发，依赖的许可证必须允许闭源分发，只要求附上声明。可以接受的列在 `src-tauri/about.toml` 的 `accepted` 里（MIT、Apache-2.0、BSD、ISC、Zlib、BSL-1.0、Unicode-3.0、MPL-2.0），`pnpm licenses:generate` 对 crate 与 npm 包都按这份列表检查，不在列表里的会让生成失败（release workflow 也随之失败）。引入新依赖前先看它（及其新带来的间接依赖）的许可证：
- GPL、LGPL、AGPL 等 copyleft 许可证不接受：GPL / AGPL 要求整个程序以同样的许可证开源；LGPL 要求用户能替换该库，Rust 静态链接做不到。只有在多许可证（`MIT OR GPL-3.0`）里能选宽松的一个时才可用。
- MPL-2.0 是文件级 copyleft：原样使用没有问题（声明里附有主页，说明源码在哪里），但修改了 MPL 文件（`[patch]`、vendoring）就要以 MPL 公开修改后的这些文件。
- 声明里没有的条件（如要求在界面或文档里致谢的 BSD-4-Clause、限制商用的 CC BY-NC、"Commons Clause"）一律不接受；拿不准的先问。确认能遵守后再加进 `accepted`。

**第三方许可证声明**：依赖的许可证要求随二进制附上其文本。`pnpm licenses:generate`（`scripts/generate-licenses.mjs`）生成 `public/third-party-licenses.json`（不进仓库），Vite 把它复制进前端产物、随 exe 内嵌，设置「关于」里的许可证对话框读取显示。Rust 依赖用 `cargo-about`（配置 `src-tauri/about.toml`，覆盖四个发布目标，不含 build / dev 依赖），前端只收运行时依赖（`pnpm licenses list --prod`）。本地需 `cargo install --locked cargo-about --features cli`，未安装时只生成前端部分，设置里注明；CI 中未安装则报错，release workflow 打包前生成。cargo-about 认不出许可证文件的 crate 在 `about.toml` 里按校验和 `clarify`，Dependabot 升级后文件变了会生成失败，更新校验和即可。

**签名**：尚未签名。macOS 只做 ad-hoc 签名（`APPLE_SIGNING_IDENTITY=-`，保证 Apple Silicon 能运行），下载后需在"隐私与安全性"中放行或去掉 quarantine；Windows 未签名，SmartScreen 会提示。以后接入证书只需给 workflow 配 secret。

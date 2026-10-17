# ZShell 开发指南

## 环境

- Rust stable、Node.js、pnpm（CI 使用 Node 26、pnpm 12）。
- macOS 或 Linux 上开发；Windows 专用代码（ConPTY、OpenSSH agent 命名管道 / Pageant）本地无法编译，依赖 CI 检查与 Windows 上的手动测试，macOS 与 Linux 的专用代码也只能在各自的平台上编译。
- Linux（Debian / Ubuntu）需要 Tauri 的系统库，以及 Tao 与 keyring 用的 libdbus：

  ```bash
  sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libssl-dev libxdo-dev librsvg2-dev libdbus-1-dev
  ```

  Fedora 上对应 `webkit2gtk4.1-devel gtk3-devel libsoup3-devel openssl-devel libxdo-devel librsvg2-devel dbus-devel`。pnpm 12 可用其 GitHub Release 上的独立可执行文件（自带运行时，不依赖系统的 Node 与 corepack）。

## 常用命令

```bash
pnpm install
pnpm tauri dev                          # 开发模式运行
pnpm build                              # 前端类型检查（tsc）并构建
pnpm test                               # 前端单元测试（vitest，测试文件与被测模块放在一起：*.test.ts）
pnpm format                             # 用 Prettier 格式化前端、脚本与配置（检查：pnpm format:check）
pnpm tauri build --debug --bundles app  # 打包 debug 版 .app，用于端到端测试（macOS）
pnpm tauri build --debug --no-bundle    # 只编译 debug 版可执行文件 src-tauri/target/debug/zshell（Linux）
pnpm tauri build                        # 打包 release 版（Linux 上是 deb 与 rpm，见 tauri.linux.conf.json）
```

后端检查在 `src-tauri/` 下运行：

```bash
cargo fmt                               # 检查：cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

格式由 Prettier（`.prettierrc.json`）和 rustfmt（`src-tauri/rustfmt.toml`）决定，两边都是 140 列，CI 会检查。Prettier 管 `src-tauri/` 与 Markdown 以外的文件，`src/lib/bindings.ts` 和 `pnpm-lock.yaml` 是生成的，也不参与（见 `.prettierignore`）。全量格式化的提交记在 `.git-blame-ignore-revs` 里，GitHub 的 blame 会跳过它们；本地运行一次 `git config blame.ignoreRevsFile .git-blame-ignore-revs` 即可同样跳过。

ZMODEM 的测试除了自己的收发互测，还与 lrzsz 的 `lsz` / `lrz` 对传（含经由伪终端），未安装 lrzsz 时跳过（`brew install lrzsz`；CI 上没有）。

`cargo test` 中的 `catalog_covers_all_keys_in_sources` 会检查后端源码用到的消息 key 都在英语语言包中（见 [I18N.md](I18N.md)）。

`cargo test` 中的 `bindings_are_up_to_date` 检查 `src/ipc/bindings.ts` 与 Rust 类型一致；改了与前端交换的类型、错误码或 `bindings.rs` 里列出的常量后，在 `src-tauri/` 下重新生成并提交：

```bash
UPDATE_BINDINGS=1 cargo test bindings
```

## 约定

- **语言**：`docs/` 用中文，其他一切（代码、注释、界面文案、README、提交信息）用英语，见 [I18N.md](I18N.md)「语言约定」。
- **提交信息**：Conventional Commits（`feat:`、`fix:`、`docs:`、`chore:`、`refactor:`、`test:`、`ci:`，可带 scope，如 `feat(sftp): …`）；正文不手动折行，每段或每个列表项一行。
- **版本号**：只在 `src-tauri/Cargo.toml` 维护，由 `scripts/release.sh` 修改（规则见下文「发布」）。`tauri.conf.json` 不写 `version`，Tauri 取 Cargo 包版本；`TERM_PROGRAM_VERSION` 用 `CARGO_PKG_VERSION`；`package.json` 为 private 包，不写版本。
- **文档分工**：做什么、先后顺序与未实现功能的设计写在 [ROADMAP.md](ROADMAP.md)；跨模块的设计写在 [ARCHITECTURE.md](ARCHITECTURE.md)；模块内的细节（边界情况、库的坑）写在代码注释里。根目录的 `CLAUDE.md`（英语）是给 AI 助手的项目说明，汇总上述约定；约定变化时同步更新。
- **单元测试**：`src/lib/` 与 `src/ipc/` 里不依赖 React、DOM 和后端命令的导出函数，在相邻的 `*.test.ts` 里测试；组件不写单元测试，界面上的行为按下文「端到端测试」检查。vitest 没有配置 DOM 环境，测试里不要碰 `localStorage` 和后端命令。
- **文件大小**：一个 Rust 文件超过约 800 行、并且里面有可以独立的类型时，改成目录模块，按类型分文件，`mod.rs` 保留模块文档和 `pub use`（如 `session/`、`config/`）。测试让文件超过这个长度时，移到同目录的 `tests.rs`（如 `config/`、`vault/`、`zmodem/`）。

## 端到端测试

需要真实 SSH 服务器的功能，用隔离的 HOME 和临时 sshd 测试，不碰自己的 `vault.json`、`settings.json` 和 `~/.ssh`（known_hosts、config、默认私钥）。

**隔离的 HOME**：在一个临时目录 `$T` 下运行 debug 包里的可执行文件：

```bash
HOME=$T/home src-tauri/target/debug/bundle/macos/ZShell.app/Contents/MacOS/zshell
```

会话配置写在 `$T/home/Library/Application Support/ZShell/vault.json`。隔离的 HOME 里没有钥匙串，新建的保险库不加密，可以直接读写。注意 WKWebView 的 localStorage（如侧栏宽度）可能不跟随 HOME，会写进正式应用的 WebKit 数据。

Linux 上运行 `src-tauri/target/debug/zshell`，配置在 `$T/home/.config/ZShell/`，WebKitGTK 的数据也跟随 HOME（前提是没有设置 `XDG_CONFIG_HOME` 等变量，有则一并指到 `$T`）。HOME 要用绝对路径：相对路径下缓存目录也是相对的，拖出给其他应用的文件 URI 与"显示"的位置都会出错。系统钥匙串（Secret Service）、polkit 与 logind 是真实会话的，没有隔离：新建的保险库会把密钥存进自己的钥匙串，debug 构建的条目在 service `org.boyin.zshell.dev` 下，不会碰到正式应用的，测试后可以删掉；应用锁要先装上策略文件（`sudo install -m 644 src-tauri/linux/org.boyin.zshell.policy /usr/share/polkit-1/actions/`）。

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

Linux（Debian / Ubuntu）上 `Subsystem` 写 `/usr/lib/openssh/sftp-server`（Fedora 是 `/usr/libexec/openssh/sftp-server`）。以 `/usr/sbin/sshd -D -e -f sshd_config` 前台运行，日志直接输出到终端（`LogLevel VERBOSE` 能看到客户端提供了哪些密钥）。测试 ProxyJump 时起两个 sshd（如 2222 作跳板机、2223 作目标）。

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

ProxyCommand 用 macOS 自带的 `nc %h %p`，或一个不存在的命令看 stderr 是否显示在终端里。隔离的 HOME 里没有钥匙串，带用户名的代理每次连接都会在终端里问密码，正好测内联询问与重试。会话与代理可以直接写进不加密的 `vault.json`（`{"header":{"format":"zshell-vault","version":2,"protection":"none"},"data":{"profiles":[…],"proxies":[…]}}`）；预先写好 `$T/home/.ssh/known_hosts`（`[localhost]:2222 <主机公钥>`）可以省去主机密钥确认。

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

两者需同步修改，然后运行 `scripts/generate-icons.sh`：它用 `tauri icon` 重新生成全部图标，再用 macOS 母版覆盖 `icon.icns`，删掉用不到的图标（移动端、512 px 的 `icon.png`、只有一种缩放的 Store 图片），再生成 `msix/` 下的 Store 图片。

各平台实际用到的图标：

- macOS：`icon.icns`（16–1024，应用包与开发模式的 Dock 图标）。
- Windows：`icon.ico`（16–256，exe 资源、窗口与任务栏、NSIS 与 MSI）。
- Microsoft Store：`msix/` 下的 `Square44x44Logo`（开始菜单的应用列表、任务栏）、`Square150x150Logo`（开始菜单磁贴）、`StoreLogo`，各有 100 / 200 / 400 % 三种缩放；`Square44x44Logo` 另有任务栏等处直接使用的 `targetsize-16/24/32/48/256` 及其 `altform-unplated`（不画底板）版本。清单里写不带限定符的文件名，`package-msix.ps1` 用 `makepri` 生成 `resources.pri`，Windows 按它找到合适的那张。
- PNG（`bundle.icon` 里的 `128x128@2x`、`128x128`、`32x32`）：非 Windows 平台的默认窗口图标取列表里第一个 PNG，Linux 的 deb / rpm 把它们全部装进 `hicolor` 图标主题（名为 `zshell`，polkit 认证框也用它）；README 也用 `128x128.png`。
- Linux 的矢量图标：deb / rpm 另把母版 `icon.svg` 原样装成 `hicolor/scalable/apps/zshell.svg`（`tauri.linux.conf.json`），桌面在任何尺寸与缩放下都用它，位图只是后备。改母版时注意它要保持自包含（不引用字体与外部文件）。

## CI

- `.github/workflows/ci.yml`：推送到 main 和 PR 时在 `macos-latest`、`windows-latest`、`windows-11-arm` 与 `ubuntu-22.04` 上各跑一遍 `pnpm build`、`pnpm test`、`cargo clippy --all-targets -D warnings` 和 `cargo test`（Linux 先用 apt 装上系统库）；格式检查（`pnpm format:check`、`cargo fmt --check`）只在 macOS 上跑，因为 Windows 上检出的文件是 CRLF 换行。平台专用代码（Windows 的 ConPTY、OpenSSH agent 管道、Pageant，macOS 的 AppKit，Linux 的 GTK）在别的平台上编译不到，靠它检查。Windows ARM64 与 Ubuntu 22.04 是 release 打包用的 runner，只在那里出现的编译问题（ARM64 上 ring 要用 clang，22.04 的 GTK / WebKitGTK 较旧）不会等到发版才发现；代码里没有按处理器区分的部分，所以不单独检查 Intel Mac。同一 PR 的新推送会取消旧的运行；main 上的每次运行都跑完，因为 release 要求 tag 的提交上 CI 通过。另有一个 Linux job 用 `cargo audit` 检查 Rust 依赖的 RustSec 公告，评估过不适用的公告写在 `src-tauri/.cargo/audit.toml` 里，各带理由和复查时间；还有一个 Linux job 跑 `pnpm licenses:generate`，新依赖的许可证不在 `about.toml` 里、或 `clarify` 的校验和过期时在 PR 上就失败，而不是等到发布。
- `.github/workflows/release.yml`：只能手动运行。在版本 tag 上运行时创建草稿 release（见下文「发布」）；在分支上运行时只打包。无论哪种，各平台的安装包都同时作为 workflow artifacts 附在该次运行上（`macOS`、`Linux`、`Windows`；`.app` 打成 `.app.tar.gz`，zip 格式的 artifact 会丢掉可执行权限和 symlink）。macOS 一个 job；Linux 一个 job（`ubuntu-22.04`，打出的可执行文件要求的 glibc 不高于构建时的，22.04 的 2.35 比 Debian 12 与 Fedora 的都旧），打 x64 的 deb 与 rpm；Windows 的 x64 与 ARM64 各一个 job 并行构建（ARM64 在 `windows-11-arm` runner 上原生编译），x64 打出安装包和免安装 exe，两边的 exe 再交给 `msix` job 打 Store 包（安装包、免安装 exe 与 Store 包里的 x64 exe 是同一个，不重复编译）。在 tag 上运行时不保存 Rust 缓存：tag 上存的缓存只有同一个 tag 能读到，留着只占 10 GB 的缓存配额。Store 包无论在 tag 还是分支上运行都只作为 workflow artifact（`Microsoft Store`），不附到 release（见下文「Microsoft Store」）。勾选 "Microsoft Store only" 时只打 Store 包，跳过 macOS job，x64 也不打安装包，在 tag 上运行也不创建 release。
- 两个 workflow 的权限：顶层只给 `contents: read`，checkout 不留凭据（`persist-credentials: false`）；release 里只有最后创建 release 的 job 有 `contents: write`，它只用 checkout 与 download-artifact，不跑构建工具。各个 Action 都固定到完整的 commit SHA，行尾注释写对应版本，Dependabot 照常更新；`dtolnay/rust-toolchain` 按分支发布，固定 SHA 后要显式写 `toolchain: stable`。
- `.github/dependabot.yml`：每周检查 npm、Cargo 和 GitHub Actions 的依赖更新。各生态的 minor / patch 更新合并为一个 PR，Tauri（及 xterm.js、russh）的相关包各自成组；major 更新单独成 PR。提交前缀为 `chore(deps)` / `ci`，不进更新日志。Dependabot 的 PR 会触发 CI。

## 发布

**版本号**：语义化版本 `X.Y.Z`，从 1.0.0 开始。
- 按语义化版本选择升哪一位：功能里程碑或新功能升 minor（1.1.0、1.2.0…），修复和小改进升 patch（1.3.1、1.3.2…）。
- 不按提交升版本。规划涉及 `src/`（前端）或 `src-tauri/`（后端）代码的修改时，先定版本号方案（升到哪个版本、包含哪些修改），确定后再动手；一个版本可以包含多个提交。只改文档、CI 或脚本不需要新版本。
- `vault.json`、`settings.json` 等配置保持向后兼容：新字段加默认值，旧文件照常读取。放弃兼容要先与维护者商定，升 major，并在更新日志里标为破坏性变更。
- 不用预发布后缀（`-beta.1` 之类）：Windows 的 MSI 只接受数字版本号。

**升版本号与发布分开**：每个版本都打 tag，但不一定发布。git-cliff 以 tag 划分版本，所以没发布的版本也要打 tag 并推送，否则下一版的更新日志会把这一版的提交再算一遍；推送 tag 不会触发构建，要发布时再单独运行 release workflow，之前任何一个已推送的版本都可以补发。

**步骤**（`scripts/release.sh`，需在 main 上且工作区干净）：
1. `scripts/release.sh prepare X.Y.Z`：修改 `src-tauri/Cargo.toml` 中的版本号并更新 `Cargo.lock`，用 git-cliff 在 `CHANGELOG.md` 顶部加上该版本一节（`--unreleased --prepend`，之前各版本已润色的内容不变）。git-cliff 的配置在 `cliff.toml`，收录 `feat` / `fix` / `perf`、`refactor`（列在 "Internal" 下，排在面向用户的变化之后）和破坏性变更，其他类型（`docs`、`ci`、`chore` 等）不出现在更新日志里，因此提交类型要选准。git-cliff 的版本固定在脚本的 `git_cliff` 里，升级时改那一行。中途失败（例如没有网络）时脚本会还原改动，可以直接重跑。
2. 润色该版本一节：合并同一功能的多条、去掉内部里程碑编号，内容保持英语。来自 issue 的修改在条目末尾署名（`(thanks @name, #12)`），并把此人加进 `THANKS.md`（每人一行，已有的补上 issue 编号）。只有 `docs` / `chore` 等提交的版本，git-cliff 生成的一节可能是空的，手写一句面向用户的说明（`tag` 要求有该版本一节）；"Internal" 下的条目同样润色成读者看得懂的说明。
3. `scripts/release.sh tag`：运行格式检查、`pnpm build`、`pnpm test`、clippy 与测试，提交 `chore: release vX.Y.Z` 并打 `vX.Y.Z` tag（之后的参数原样传给 `git commit`，如 `--trailer`）。
4. `git push origin main vX.Y.Z`。只升版本号的话到此为止。
5. 要发布时：`scripts/release.sh publish [X.Y.Z]`（默认当前版本），用 `gh workflow run release.yml --ref vX.Y.Z` 在该 tag 上运行 release workflow。workflow 先校验 tag 与 `Cargo.toml` 版本一致、`CHANGELOG.md` 中有 `## [X.Y.Z]` 一节，以及 CI 在该提交上通过（还在运行就等它跑完；没有运行过，例如只推了 tag，就失败，因为打包的 job 不跑测试），然后并行打包，最后由一个 job 把各平台的包（Store 包除外）一起放进新建的草稿 release。发布说明由 `scripts/release-notes.sh` 从 `CHANGELOG.md` 取：不是每个版本都发布，所以包含上一个已发布的 release（不算草稿）之后的全部版本；只有这一个版本时就是该节原文，有多个时每个版本一节、以版本号和日期为标题，新的在前。本地运行 `scripts/release-notes.sh X.Y.Z` 可以预览。各个包：
   - macOS 通用包（`universal-apple-darwin`，`.app` 与 `.dmg`）。
   - Windows NSIS 安装包与 MSI。
   - Linux 的 deb 与 rpm（x64）。不打 AppImage：应用锁的 polkit 策略文件（`src-tauri/linux/org.boyin.zshell.policy`）要由包管理器装进 `/usr/share/polkit-1/actions/`。
   - Windows 免安装的单独 exe（打安装包那次构建出的 `zshell.exe`，改名为 `ZShell_<版本>_x64_standalone.exe`）。需要系统有 WebView2 运行时；配置仍写在 `%APPDATA%`，不是便携模式。
   - Microsoft Store 用的 `ZShell_<版本>.msixbundle`（x64 与 ARM64，由 `scripts/package-msix.ps1` 用 Windows SDK 的 `makeappx` 打包，清单模板在 `src-tauri/msix/`，不签名）。只作为该次运行的 workflow artifact，不附到 release。
6. 检查草稿后手动发布，作为正式版本发布（不勾选 pre-release）。
7. 从该次运行的 `Microsoft Store` artifact 下载 `.msixbundle`（`gh run download <run-id> -n "Microsoft Store"`），在 Partner Center 新建提交并上传（要填的内容见下文「Microsoft Store 提交」），每个发布的版本都这样提交。Store 会重新签名并负责用户的更新。artifact 默认保留 90 天，过期了用 "Microsoft Store only" 在该 tag 上重新运行即可。

**Microsoft Store**：商店页面 <https://apps.microsoft.com/detail/9n502tzg8hk8>（Store ID `9N502TZG8HK8`），2026-10 首次上架。以 full trust 桌面应用（`runFullTrust`，不进 AppContainer）的 MSIX 上架，由 Store 用微软的证书重新签名，不需要自己的代码签名证书，安装与更新由 Store 负责（应用本身没有自动更新）。不走 EXE / MSI 上架：那条路要求安装包用受信任 CA 的证书签名，且 Store 不负责更新。GitHub Release 的各个包照旧，可以与 Store 版同时安装。`.msixbundle` 不放进 GitHub Release：它没有签名，Windows 不允许安装（只有开发者模式下用 `Add-AppxPackage -Register` 注册解包后的目录可以，见 `package-msix.ps1` 的注释）；自己签名也不可行，签名证书的 subject 必须与清单里 Partner Center 分配的 `Publisher`（`CN=<GUID>`）一致，CA 不会签发这样的证书，换 Publisher 又成了与 Store 版不同的另一个应用。要 MSIX 的用户从 Store 安装。文件系统虚拟化保持 MSIX 的默认行为：应用在 `AppData` 下新建的文件写到包的私有目录（`%LOCALAPPDATA%\Packages\BoyinChen.ZShell_0mrn21pkbrd8j\LocalCache`），卸载时删除，已有的文件原地读写；所以机器上已有 GitHub 版的配置时 Store 版直接沿用，全新安装的配置只有 Store 版看得到（关闭虚拟化要用只给特定游戏的受限能力 `unvirtualizedResources`，不考虑）。清单的身份、版本号与最低系统版本见 `src-tauri/msix/AppxManifest.xml` 的注释。

**Microsoft Store 提交**：Partner Center 里要填的文字都在 `src-tauri/msix/listing.md`（英语，各节对应 Partner Center 的各个字段），改功能介绍时与 README 一起更新。
- 商店页面只有 English (United States)。清单声明了五种语言（`AppxManifest.xml` 的 `<Resources>`，测试保证与语言包一致），上传包后 Partner Center 会在 "Languages supported by your packages" 里列出另外四种：点 Remove 移除，只是不为它们提供商店页面。
- 定价与可用性：免费，所有市场。属性：类别 Developer tools，隐私政策、网站与支持联系方式按 `listing.md`。
- 年龄分级（IARC 问卷）：类别选工具 / 效率类应用；没有暴力、性、赌博等内容，用户之间不能交流或分享内容，不分享位置，没有数字购买，不是网页浏览器（链接在系统浏览器里打开），预期结果为最低分级（3+ / Everyone）。
- "What's new in this version"：上次提交到 Store 之后各版本在 `CHANGELOG.md` 里的条目，超过 1500 字时缩写。
- 提交选项：`listing.md` 的 "Notes for certification" 整段填入给认证人员的说明，其中 `runFullTrust` 一段也用作受限能力的理由。
- 截图在 Windows 上用英语界面截，1366 × 768 以上的 PNG。

**依赖的许可证**：ZShell 不开源、免费分发（见 `LICENSE.md`），依赖的许可证必须满足三点：允许分发不附源码的构建；不限制用途，用户可以在工作中商用；义务至多是附上声明。满足这三点的常见许可证都可以接受，列在 `src-tauri/about.toml` 的 `accepted` 里：宽松许可证（MIT、Apache-2.0、BSD-2/3-Clause、ISC、Zlib、BSL-1.0、Unicode、BlueOak、Python-2.0 等）、没有条件的（0BSD、MIT-0、Unlicense、CC0-1.0）、原样使用的 MPL-2.0，以及数据与字体常用的 CC-BY-3.0/4.0、CDLA-Permissive-2.0、OFL-1.1。`pnpm licenses:generate` 对 crate 与 npm 包都按这份列表检查，不在列表里的会让生成失败（release workflow 也随之失败）。引入新依赖前先看它（及其新带来的间接依赖）的许可证：
- GPL、LGPL、AGPL 等 copyleft 许可证不接受：GPL / AGPL 要求整个程序以同样的许可证开源；LGPL 要求用户能替换该库，Rust 静态链接做不到。只有在多许可证（`MIT OR GPL-3.0`）里能选宽松的一个时才可用。
- MPL-2.0 是文件级 copyleft：原样使用没有问题（声明里附有主页，说明源码在哪里），但修改了 MPL 文件（`[patch]`、vendoring）就要以 MPL 公开修改后的这些文件。
- 声明以外的条件一律不接受：要求在界面或文档里致谢的 BSD-4-Clause，限制用途的 CC-BY-NC、SSPL、BUSL、Elastic License、"Commons Clause"，以及 JSON License「用于善而非恶」这类没法确定是否遵守的条款。拿不准的先问。确认能遵守后再加进 `accepted`。
- Creative Commons 只接受 CC-BY 与 CC0：CC-BY 要求的署名与许可证链接由声明满足，CC0 没有条件。CC-BY-SA 要求改编作品沿用同一许可证，CC-BY-NC 禁止商用，CC-BY-ND 禁止演绎，都不接受。OFL-1.1 的字体可以随应用分发，只是不能单独出售。
- 带例外的表达式（`Apache-2.0 WITH LLVM-exception`）还没有加进列表：`generate-licenses.mjs` 解析表达式时不认识 `WITH`，cargo-about 的写法也要先确认。遇到时再处理（例外只增加许可，判断看前面的许可证）。

**第三方许可证声明**：依赖的许可证要求随二进制附上其文本。`pnpm licenses:generate`（`scripts/generate-licenses.mjs`）生成 `public/third-party-licenses.json`（不进仓库），Vite 把它复制进前端产物、随 exe 内嵌，设置「关于」里的许可证对话框读取显示。Rust 依赖用 `cargo-about`（配置 `src-tauri/about.toml`，覆盖五个发布目标，不含 build / dev 依赖），前端只收运行时依赖（`pnpm licenses list --prod`）。本地需 `cargo install --locked cargo-about --features cli`，未安装时只生成前端部分，设置里注明；CI 中未安装则报错，release workflow 打包前生成。cargo-about 认不出许可证文件的 crate 在 `about.toml` 里按校验和 `clarify`，Dependabot 升级后文件变了会生成失败，更新校验和即可。

**签名**：尚未签名。macOS 只做 ad-hoc 签名（`APPLE_SIGNING_IDENTITY=-`，保证 Apple Silicon 能运行），下载后需在"隐私与安全性"中放行或去掉 quarantine；Windows 未签名，SmartScreen 会提示。以后接入证书只需给 workflow 配 secret。

# ZShell 开发指南

## 环境

- Rust stable、Node.js、pnpm（CI 使用 Node 26、pnpm 12）。
- macOS 上开发；Windows 专用代码（ConPTY、OpenSSH agent 命名管道 / Pageant）本地无法编译，依赖 CI 检查与 Windows 上的手动测试。

## 常用命令

```bash
pnpm install
pnpm tauri dev                          # 开发模式运行
pnpm build                              # 前端类型检查（tsc）并构建
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

- `.github/workflows/windows.yml`：推送到 main 和 PR 时在 `windows-latest` 上跑 `cargo clippy --all-targets -D warnings` 和 `cargo test`，保证 Windows 专用代码能通过编译。
- `.github/workflows/release.yml`：只能手动运行。在版本 tag 上运行时创建草稿 release（见下文「发布」）；在分支上运行时只打包，产物作为 workflow artifacts。勾选 "Microsoft Store only" 时只打 Store 包，省去 macOS 与其他 Windows 包。
- `.github/dependabot.yml`：每周检查 npm、Cargo 和 GitHub Actions 的依赖更新。各生态的 minor / patch 更新合并为一个 PR，Tauri（及 xterm.js、russh）的相关包各自成组；major 更新单独成 PR。提交前缀为 `chore(deps)` / `ci`，不进更新日志。Dependabot 的 PR 会触发 Windows CI。

## 发布

**版本号**：语义化版本 `X.Y.Z`，从 1.0.0 开始。
- 改动了 `src/`（前端）或 `src-tauri/`（后端）代码的提交，之后都要升版本号，哪怕只改了一个字；只改文档、CI 或脚本的提交不升。
- 按语义化版本选择升哪一位：功能里程碑或新功能升 minor（1.1.0、1.2.0…），修复和小改进升 patch（1.3.1、1.3.2…）。
- `profiles.json`、`settings.json` 等配置保持向后兼容：新字段加默认值，旧文件照常读取。只有不得不放弃兼容时才升 major，并在更新日志里标为破坏性变更。
- 不用预发布后缀（`-beta.1` 之类）：Windows 的 MSI 只接受数字版本号。

**升版本号与发布分开**：每个版本都打 tag，但不一定发布。git-cliff 以 tag 划分版本，所以没发布的版本也要打 tag 并推送，否则下一版的更新日志会把这一版的提交再算一遍；推送 tag 不会触发构建，要发布时再单独运行 release workflow，之前任何一个已推送的版本都可以补发。

**步骤**（`scripts/release.sh`，需在 main 上且工作区干净）：
1. `scripts/release.sh prepare X.Y.Z`：修改 `src-tauri/Cargo.toml` 中的版本号并更新 `Cargo.lock`，用 git-cliff 在 `CHANGELOG.md` 顶部加上该版本一节（`--unreleased --prepend`，之前各版本已润色的内容不变）。git-cliff 的配置在 `cliff.toml`，只收录 `feat` / `fix` / `perf` 和破坏性变更，其他类型（`docs`、`ci`、`chore`、`refactor` 等）不出现在更新日志里，因此提交类型要选准。
2. 润色该版本一节：合并同一功能的多条、去掉内部里程碑编号，内容保持英语。只有 `refactor` / `chore` 等提交的版本，git-cliff 生成的一节可能是空的，手写一句面向用户的说明（`tag` 要求有该版本一节）。
3. `scripts/release.sh tag`：运行 `pnpm build`、clippy 与测试，提交 `chore: release vX.Y.Z` 并打 `vX.Y.Z` tag（之后的参数原样传给 `git commit`，如 `--trailer`）。
4. `git push origin main vX.Y.Z`。只升版本号的话到此为止。
5. 要发布时：`scripts/release.sh publish [X.Y.Z]`（默认当前版本），用 `gh workflow run release.yml --ref vX.Y.Z` 在该 tag 上运行 release workflow。workflow 先校验 tag 与 `Cargo.toml` 版本一致、`CHANGELOG.md` 中有 `## [X.Y.Z]` 一节，再由一个 job 统一创建草稿 release（以该节为发布说明；避免并行 job 重复创建），然后并行打包并上传：
   - macOS 通用包（`universal-apple-darwin`，`.app` 与 `.dmg`）。
   - Windows NSIS 安装包与 MSI。
   - Windows 免安装的单独 exe（`--no-bundle` + `uploadPlainBinary`，文件名 `ZShell_<版本>_x64_standalone.exe`）。需要系统有 WebView2 运行时；配置仍写在 `%APPDATA%`，不是便携模式。
   - Microsoft Store 用的 `ZShell_<版本>.msixbundle`（x64 与 ARM64，由 `scripts/package-msix.ps1` 用 Windows SDK 的 `makeappx` 打包，清单模板在 `src-tauri/msix/`，不签名）。
6. 检查草稿后手动发布，作为正式版本发布（不勾选 pre-release）。
7. 在 Partner Center 新建提交，上传该版本的 `.msixbundle`。Store 会重新签名并负责用户的更新。

**第三方许可证声明**：依赖的许可证要求随二进制附上其文本。`pnpm licenses:generate`（`scripts/generate-licenses.mjs`）生成 `public/third-party-licenses.json`（不进仓库），Vite 把它复制进前端产物、随 exe 内嵌，设置「关于」里的许可证对话框读取显示：左栏是可搜索的包列表（按 Rust / JavaScript 分组），右栏是选中包的许可证全文。文件里每个包记录名称、版本、声明的许可证、主页和所用许可证文本的下标，相同的文本只存一份。
- Rust 依赖用 `cargo-about`：配置 `src-tauri/about.toml` 列出接受的许可证、四个发布目标（macOS 与 Windows 专用的 crate 都在内），不含 build / dev 依赖；脚本读取 `cargo about generate --format json` 的输出。双许可证优先用 MIT。新依赖带来未列出的许可证时生成失败，确认能遵守后再加进 `accepted`。
- 有些 crate 的许可证文件名 cargo-about 认不出（windows-rs 的 `license-mit`、Tauri 插件的 `LICENSE_MIT`），在 `about.toml` 里用 `clarify` 按校验和指定；文件内容变了会生成失败，更新校验和即可。完全不带许可证文件的 crate（objc2 系列、webview2-com 等）用标准许可证文本。
- 前端依赖取 `pnpm licenses list --prod`（运行时依赖，含间接依赖），读各包自带的 LICENSE / COPYING / NOTICE；不带文件的包（Tauri 插件的 JS 包）只写许可证标识与主页。
- 本地需 `cargo install --locked cargo-about --features cli`（0.9 起命令行要 `cli` feature）。未安装时只生成前端部分并注明；没运行过时设置里显示"未包含"。CI 中（设置了 `CI` 环境变量）未安装则报错。release workflow 的打包 job 与 Store job 用 `taiki-e/install-action` 装预编译的 cargo-about，打包前生成。
- 生成的文件约 420 KB（约 400 个 crate、20 个 npm 包），Tauri 默认的 `compression` 特性在构建时把前端资源用 brotli 压缩后嵌入，在 exe 里只占约 20 KB，不必另外压缩。

**签名**：尚未签名。macOS 只做 ad-hoc 签名（`APPLE_SIGNING_IDENTITY=-`，保证 Apple Silicon 能运行），下载后需在"隐私与安全性"中放行或去掉 quarantine；Windows 未签名，SmartScreen 会提示。以后接入证书只需给 workflow 配 secret。

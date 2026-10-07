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

`cargo test` 中的 `catalog_covers_all_keys_in_sources` 会检查后端源码用到的消息 key 都在英语语言包中（见 [I18N.md](I18N.md)）。

## 约定

- **语言**：`docs/` 用中文，其他一切（代码、注释、界面文案、README、提交信息）用英语，见 [I18N.md](I18N.md)「语言约定」。
- **提交信息**：Conventional Commits（`feat:`、`fix:`、`docs:`、`chore:`、`refactor:`、`ci:`，可带 scope，如 `feat(sftp): …`）；正文不手动折行，每段或每个列表项一行。
- **版本号**：只在 `src-tauri/Cargo.toml` 维护。`tauri.conf.json` 不写 `version`，Tauri 取 Cargo 包版本；`TERM_PROGRAM_VERSION` 用 `CARGO_PKG_VERSION`；`package.json` 为 private 包，不写版本。
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
LogLevel VERBOSE
```

以 `/usr/sbin/sshd -D -e -f sshd_config` 前台运行，日志直接输出到终端（`LogLevel VERBOSE` 能看到客户端提供了哪些密钥）。测试 ProxyJump 时起两个 sshd（如 2222 作跳板机、2223 作目标）。

**ssh-agent**：Unix socket 路径有长度限制，临时目录太深时用相对路径启动：在 `$T` 下运行 `ssh-agent -a ./a.sock`，并以 `SSH_AUTH_SOCK=./a.sock` 从同一目录启动应用。

**模拟从 Finder 启动**（没有 `LANG`、只有最小 PATH），用于测试本地终端的登录 shell 与区域设置：

```bash
env -i HOME=$T/home USER=$USER LOGNAME=$USER TMPDIR=$TMPDIR PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  src-tauri/target/debug/bundle/macos/ZShell.app/Contents/MacOS/zshell
```

测试 HOME 中放一个空的 `.zshrc`，否则 zsh 会显示新用户向导。

**只能在真机键盘上验证的**：按键长按重复（合成的按键事件不会自动重复）、输入法组字与候选框位置。

## 图标

贝壳螺旋起笔的"Z"，以块状光标收尾（沙色底、珊瑚色线条）。母版在 `src-tauri/icons/source/`：

- `icon.svg`：满幅，用于 Windows 与各尺寸 PNG。
- `icon-macos.svg`：同一图形按 Apple 网格放置（1024 画布中央 824 的圆角方形，四周留 100 边距）。

两者需同步修改，然后运行 `scripts/generate-icons.sh`：它用 `tauri icon` 重新生成全部图标，再用 macOS 母版覆盖 `icon.icns`，并删掉 CLI 总会生成的移动端图标。

## CI

- `.github/workflows/windows.yml`：推送到 main 和 PR 时在 `windows-latest` 上跑 `cargo clippy --all-targets -D warnings` 和 `cargo test`，保证 Windows 专用代码能通过编译。
- `.github/workflows/release.yml`：见下文「发布」。手动运行该 workflow 只打包，产物作为 workflow artifacts。

## 发布

1. 修改 `src-tauri/Cargo.toml` 中的版本号（`Cargo.lock` 随之更新）。
2. 生成更新日志：`pnpm dlx git-cliff --tag vX.Y.Z -o CHANGELOG.md`。配置在 `cliff.toml`，只收录 `feat` / `fix` / `perf` 和破坏性变更，其他类型（`docs`、`ci`、`chore`、`refactor` 等）不出现在更新日志里，因此提交类型要选准。生成后可以手动润色，内容保持英语。
3. 提交以上改动（如 `chore: release vX.Y.Z`），推送 `vX.Y.Z` tag。
4. workflow 先校验 tag 与 `Cargo.toml` 版本一致、`CHANGELOG.md` 中有 `## [X.Y.Z]` 一节，再由一个 job 统一创建草稿 release（以该节为发布说明；避免并行 job 重复创建），然后并行打包并上传：
   - macOS 通用包（`universal-apple-darwin`，`.app` 与 `.dmg`）。
   - Windows NSIS 安装包与 MSI。
   - Windows 免安装的单独 exe（`--no-bundle` + `uploadPlainBinary`，文件名 `ZShell_<版本>_x64_standalone.exe`）。需要系统有 WebView2 运行时；配置仍写在 `%APPDATA%`，不是便携模式。
5. 检查草稿后手动发布。

**签名**：尚未签名。macOS 只做 ad-hoc 签名（`APPLE_SIGNING_IDENTITY=-`，保证 Apple Silicon 能运行），下载后需在"隐私与安全性"中放行或去掉 quarantine；Windows 未签名，SmartScreen 会提示。以后接入证书只需给 workflow 配 secret。

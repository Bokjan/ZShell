# 待办

本文记录已经查明、但暂时不处理的小事：在等上游发版的问题，不值得单开里程碑的修复，以及评审提出、眼下收益抵不上改动的优化。功能规划写在 [ROADMAP.md](ROADMAP.md)。一项处理完就从这里删掉，经过写进提交信息。

## 等待上游

### SSH：认证完成前 keepalive 定时器空转，占满一个核

- **现象**：密码、口令或验证码提示停在终端里超过 keepalive 间隔（默认 30 秒），或者在等 Duo 推送时，ZShell 占满一个 CPU 核。用户输入、服务器断开（`LoginGraceTime` 默认 120 秒）或关掉标签后恢复。
- **原因**：russh 0.64.1 客户端主循环（`client/mod.rs` 约 1426–1438 行）在 `keepalive_timer` 到期时，只有状态是 `Authenticated` 才发 keepalive；定时器只在 `received_data || sent_keepalive` 时重置（约 1515–1521 行）。认证完成前到期，这个分支什么都不做，也不重置。tokio 的 `Sleep` 到期后每次 poll 都是 Ready，`select!` 就一直空转，直到对端再发数据。`ssh/mod.rs` 在连接时就把 `keepalive_interval` 交给 russh。2026-10-12 用同一份 `Cargo.lock` 写小程序复现：keepalive 设 1 秒，认证前空等 4 秒用掉 3.16 秒 CPU，不设 keepalive 时是 0。
- **上游修复**：我们提了 [Eugeny/russh#791](https://github.com/Eugeny/russh/pull/791)（2026-10-16），等待合入。问题由 [Eugeny/russh#642](https://github.com/Eugeny/russh/pull/642)（认证前不发 keepalive，2026-02-28 合入）引入。修法：定时器到期时不论是否发出 keepalive 都记下 `keepalive_elapsed`，用它代替 `sent_keepalive` 决定重置 keepalive 定时器、不重置 inactivity 定时器（空转时 inactivity 定时器每圈都被重置，认证前 `inactivity_timeout` 也一直不生效，这个 PR 一并修好）。附两个测试：认证前 500 毫秒内套接字读取次数，以及认证前 `inactivity_timeout` 能触发。
- **处理**：russh 发布修复后，在 `src-tauri/` 下升级 russh，用上面的方法确认认证提示停留时 CPU 不再升高，作为补丁版本发布。
- **复现素材**（PR 说明里已经用到）：
  - 受影响版本：0.57.1（2026-02-28，第一个包含 #642 的版本）到 0.64.1，main 到 `a0ded76`（2026-10-04）为止。
  - 复现：客户端 `Config { keepalive_interval: Some(1s), .. }`，用 `client::connect_stream` 连一个 `russh::server::run_stream` 起的本地服务器（服务器密钥任意，handler 用默认实现），`authenticate_none` 被拒后什么都不做，睡 4 秒，前后用 `getrusage(RUSAGE_SELF)` 量 CPU。结果：keepalive 1 秒时 4 秒里用掉约 3.2 秒 CPU，`keepalive_interval: None` 时是 0。另一种：本地监听只发 `SSH-2.0-silent\r\n` 之后不说话，`timeout(2s, connect_stream(..))` 超时丢掉 future 后，被 spawn 的会话任务照样空转（4 秒里约 4.3 秒 CPU）。
  - 补丁思路：`keepalive_timer` 的分支里，未认证时记下「到期了」，循环末尾重置定时器的条件 `received_data || sent_keepalive` 加上它；或者在分支的 else 里直接重置。
- **相关**：重交换时 russh 不调用 `check_server_key`，见下一条。
- **备选**：上游迟迟不修时，用 `[patch.crates-io]` 指向打了补丁的 russh（russh 是 Apache-2.0，可以分发修改版）；或者 `keepalive_interval` 传 `None`，认证完成后用 `Handle::send_keepalive` 自己计时，但要自己实现 `keepalive_max` 的断线判断，而且流控会暂停整条连接，可能把暂停误判为断线。
- **登记**：2026-10-12（代码审查 H1）。

### SSH：重交换时不比对主机密钥

- **现象**：会话中途的密钥重交换不检查服务器的主机密钥。已经握有会话密钥的一方（实际上就是服务器本身）可以在重交换时换一把主机密钥而不被发现；网络上的中间人做不到。
- **原因**：russh 0.64.1 只在初次密钥交换（`encrypted.is_none()`，`client/mod.rs` 约 2008–2023 行）调用 `Handler::check_server_key`，重交换那一支（约 1986 行起）不调用。`kex_done` 回调只拿到算法名，拿不到主机密钥，ZShell 这边也补不上。
- **上游修复**：还没有，上游也没有相关 issue。
- **处理**：russh 在重交换时也调用 `check_server_key`（或把主机密钥交给 `kex_done`）之后，在 `ssh/handler.rs` 里记下初次接受的密钥，重交换时要求同一把，不再询问用户。之前那段比对代码因为永远走不到，已经删掉（v2.11.1）。
- **上游 issue 的素材**：重交换时 `KexProgress::Done` 带着 `server_host_key`，只是没有交给 handler；OpenSSH 每次密钥交换都验证主机密钥，并要求和初次的一致。
- **登记**：2026-10-12（代码审查 L10）。

### CI：pnpm/action-setup 停在 v6.0.10

- **现象**：Dependabot 把 `pnpm/action-setup` 升到 v6.1.0（#3，2026-10-15 合入）后，Windows runner 上 PowerShell 步骤里的每条 `pnpm` 命令都不输出任何东西、立即以 0 退出：CI 的 Windows 与 Windows ARM64 job 里 `pnpm build`、`pnpm test` 没有运行却显示通过（cargo 的步骤不受影响），直到 v3.4.0 的 release workflow 在 Windows x64 上找不到安装包才暴露。
- **上游**：[pnpm/action-setup#291](https://github.com/pnpm/action-setup/issues/291)（2026-10-12 提出，未修复）是同一处：v6.1.0 在 Windows 上先装 pnpm 11 再 `self-update` 到 12，留下的 `pnpm.cmd` shim 指向不存在的路径。那里的步骤会报错失败，我们这里却静默成功，原因没有深究。
- **处理**：三个 workflow 都固定在 v6.0.10，Dependabot 跳过 6.1.0；CI 的 `Check that pnpm runs` 在 `pnpm --version` 没有输出时失败，所以 Dependabot 提出更新的版本时，CI 能看出它是否修好。修好后合入 Dependabot 的 PR，删掉 `dependabot.yml` 里的 ignore 和这一条。
- **登记**：2026-10-16。

### Windows：最大化后按 F11，任务栏位置留黑边

- **现象**：窗口先最大化，再按 F11 进入全屏，屏幕底部原来任务栏的那一条变成黑边或透明。窗口没有最大化时进入全屏是正常的。
- **原因**：tao 0.37.1 的问题。Windows 上主窗口没有原生边框，客户区由 tao 在 `WM_NCCALCSIZE` 里自己计算：窗口最大化时，它把客户区裁成显示器的工作区（去掉任务栏）。进入全屏时 tao 没有清掉最大化状态，客户区仍被裁成工作区，任务栏那一条就成了没有绘制的非客户区。
- **上游修复**：[tauri-apps/tao#1353](https://github.com/tauri-apps/tao/pull/1353)（提交 [`2831c7d`](https://github.com/tauri-apps/tao/commit/2831c7d69d144a062cc8b99a070c579a0787641b)），全屏时 `WM_NCCALCSIZE` 直接返回 0。2026-09-30 合入 dev 分支，排在待发的 [tauri-apps/tao#1357](https://github.com/tauri-apps/tao/pull/1357) 里，会随 0.37.2 发布。
- **处理**：0.37.2 发布后，在 `src-tauri/` 下执行 `cargo update -p tao`。tauri-runtime-wry 2.12.1 对 tao 的要求是 `^0.37.0`，不用等 Tauri 发版。升级后到 Windows 上确认问题消失，作为补丁版本发布。
- **备选**：如果上游迟迟不发版，在 `src-tauri/src/window/win.rs` 里用 `SetWindowSubclass` 给主窗口挂一个子类，在窗口没有 `WS_CAPTION` 时拦截 `WM_NCCALCSIZE` 并返回 0（tao 只在全屏时去掉 `WS_CAPTION`）。这和上游修复的效果相同，需要给 windows-sys 加上 `Win32_UI_Shell` feature；等 tao 升级后再删掉。
- **登记**：2026-10-11。

### Windows：打字后弹出的文件对话框没有指针（已绕过，等 WebView2 修复后去掉）

- **现象**：系统开着「鼠标 → 指针选项 → 打字时隐藏指针」（默认开）时，在终端输入 `rz` 回车，弹出的文件选择框里看不到指针，只要指针不回到网页上就一直看不到。凡是打字后紧接着弹出的原生对话框都会这样。
- **原因**：Chromium 的「打字时隐藏指针」（`features::kHideCursorWhileTyping`，`ui/wm/core/cursor_manager.cc`）在 Windows 上通过 `DesktopWindowTreeHostWin::OnCursorVisibilityChangedNative` 调用 `::ShowCursor(FALSE)`，把共享输入队列的指针显示计数降到 0 以下；只有指针在网页上移动时它才会把计数加回来。文件对话框和 WebView2 的窗口共用这个计数，所以对话框里也看不到指针。
- **上游修复**：Chromium 提交「Restore mouse cursor for native dialogs on Windows」（Cr-Commit-Position `refs/heads/main@{#1716435}`，2026-10-11，bug 567626032）不再调用 `::ShowCursor()`，并在模态对话框弹出时（`HandleCancelMode`）恢复指针。还要等它进入 WebView2 运行时的稳定版。
- **绕过**（v3.0.2）：前端的文件对话框都通过 `src/ipc/fileDialog.ts` 打开（`fileDialog.test.ts` 检查有没有直接用插件的），它在对话框前后调用 `window_file_dialog`。后端 `src-tauri/src/window/win.rs` 的 `need_pointer` 在主线程把计数加到 0 以上，最后一个需要指针的窗口关掉后再减回同样的次数。取色窗口（`src-tauri/src/color_pick/win.rs`）也调用它：用键盘打开吸管时，同样可能刚打过字。
- **处理**：等 WebView2 稳定版运行时用上包含 #1716435 的 Chromium 版本，并在 Windows 上确认：关掉绕过后，打开「打字时隐藏指针」，输入 `rz`，对话框里有指针。确认之后删掉 `fileDialog.ts`、`fileDialog.test.ts`、`window_file_dialog`、`win::file_dialog` 和 `win::need_pointer`（连同取色窗口里对它的调用），各组件改回直接用 `@tauri-apps/plugin-dialog`。WebView2 运行时随系统自动更新，但旧版 Windows 10 上可能停在较老的版本，可以等新版普及一段时间再删。
- **登记**：2026-10-13。

### Linux：依赖里的 glib 0.18 有一条中等级别的安全公告

- **现象**：Dependabot 报 `src-tauri/Cargo.lock` 里的 `glib` 0.18.5 不安全（[警告 #1](https://github.com/Bokjan/ZShell/security/dependabot/1)），要求 0.20.0 以上。问题出在 `glib::VariantStrIter` 的 `Iterator` / `DoubleEndedIterator` 实现，只有调用 `Variant::array_iter_str` 才会碰到。
- **原因**：glib 是 Tauri 在 Linux 上用的 GTK3 绑定（`gtk` 0.18、`webkit2gtk` 2.0，经 tao、wry、muda 引入）的底层，固定在 0.18。gtk-rs 的 GTK3 绑定已经停在 0.18 并归档，不会有兼容 glib 0.20 的版本；0.18 到 0.20 也是不兼容升级，`cargo update` 升不上去。
- **影响**：macOS 和 Windows 的包不编译这些 crate。2026-10-16 在 Linux 已发布之后（3.3.0 的依赖树）重新核对：Linux 目标下依赖 glib 的 22 个 crate（tauri 2.12.3、wry 0.57.0、tao 0.37.1、muda 0.20.0、webkit2gtk 2.0.2 等）和 ZShell 自己都没有调用 `array_iter_str`。该警告已在 GitHub 上以“代码未用到”关闭。
- **处理**：Tauri 把 Linux 后端迁到 GTK4 后，glib 会跟着升到 0.20 以上。在那之前，每次升级 Tauri 或 GTK 相关依赖后重新检查：`cargo tree -i glib@0.18.5 --target x86_64-unknown-linux-gnu` 列出的 crate 源码里有没有 `array_iter_str`。
- **登记**：2026-10-11。

### Windows：windows-rs 0.100 只发了一部分

- **现象**：Dependabot 提了把 `windows-core` 升到 0.100、`windows-future` 升到 0.100 的 PR（#6、#7），Windows 上编译失败：`#[implement]` 生成的代码用的是 `windows` 0.62 依赖的 `windows-core` 0.62，依赖树里有两个版本的 `windows_core`，`IDataObject: Interface` 等约束不满足。
- **原因**：windows-rs 2026-09-03 发布 0.100.0 时，`windows-core`、`windows-future`、`windows-implement` 等小 crate 上了 crates.io，`windows` 本体还没发，最新仍是 0.62.2。按 [microsoft/windows-rs#4867](https://github.com/microsoft/windows-rs/issues/4867)，`windows` 和 `windows-sys` 改用从 SDK 头文件生成的新元数据，可能晚些才发布。新元数据下很多 Win32 API 的签名、类型和 feature 名都会变。
- **处理**：`.github/dependabot.yml` 已经忽略 `windows-core` 和 `windows-future`，两个 PR 已关闭。`windows` 0.100 发布后，Dependabot 会提它的升级。那时手动把三个 crate 一起升到同一版本，按新 API 改用到它们的代码（`sftp/drag/win.rs` 的拖放、`applock/win.rs` 的 Windows Hello、`pty/shell.rs` 的 `WindowsIntegrityPolicy`），交给 Windows 上的 CI 编译。`windows-sys` 0.61 版本号独立，不受影响。
- **登记**：2026-10-15。

### 前端：后台终端滚动时重建整屏 DOM（已用补丁绕过，等 xterm.js 修复后去掉）

- **现象**：后台标签页里的终端有输出、内容向上滚动时，每一帧都把整屏的行重新生成成 DOM 节点，切到后台也不停。2026-10-17 在 Store 版 3.5.0 上测量：后台终端（222×61，跑着 Claude Code）20 秒里重建约 460 个节点，每次滚动约 50 行；输出越快越多，最多每帧一屏。节点随后被回收，不算泄漏，但持续占用 CPU、制造 GC 压力。
- **原因**：xterm.js 6.0.0 的 `CoreBrowserTerminal` 在每次滚动时调用 `SelectionService.refresh()`，不管有没有选区；下一帧 `RenderService.handleSelectionChanged` 不看 `_isPaused`，直接交给渲染器；`DomRenderer.handleSelectionChanged` 无条件 `renderRows(0, rows - 1)`。WebGL 渲染器走 `refreshRows`，暂停时不画，所以只有 DOM 渲染器受影响；而后台标签页的终端交出 WebGL 上下文、改用 DOM 渲染器（`src/lib/webgl.ts`），所以每个后台终端都受影响。同一次排查里更大的一处（隐藏的标签页只设了 `visibility: hidden`，xterm.js 的 IntersectionObserver 认为它仍然可见，后台终端一直不暂停，同一终端 20 秒重建约 8.3 万个节点）已经改为把隐藏页移出视口（`styles/terminals.css`），这里剩下的是绕过暂停的这一处。
- **上游**：[xtermjs/xterm.js#6208](https://github.com/xtermjs/xterm.js/issues/6208)（2026-10-05 提出，未修复）就是这一处，建议的修法和我们的补丁相同；评论里还列出几条同样绕过暂停的路径：暂停时的 `handleBlur` / `handleFocus`、暂停前已排队的刷新、交出 WebGL 后 `DomRenderer.handleResize` 的重画。master 的提交 [`5c2b19f`](https://github.com/xtermjs/xterm.js/commit/5c2b19f2c)（2026-03-12）让 `DomRenderer` 只重画新旧选区所在的行、没有选区时不重画，6.1.0-beta 里的 OffscreenCanvas `WidthCache` 也让这些重画不再逐字测量 DOM，但正式版仍是 6.0.0（2025-12-22），6.1.0 尚无发布日期。
- **关注**：issue 里已有人在讨论，也有人请求指派，我们不另提 PR，持续关注 #6208 的进展和 6.1.0 的发布。issue 列出的其余路径在 ZShell 里大概也会碰到：切换标签页时后台终端失去焦点，隐藏时释放 WebGL 之后 `DomRenderer.handleResize` 的重画。不过它们每次切换只发生一次，ZShell 的隐藏页也仍然参与布局，字宽能正常测量和缓存，所以开销不大（未实测）。上游若只修了 `handleSelectionChanged`，要再确认这几条路径。
- **绕过**：`pnpm patch` 给 `@xterm/xterm@6.0.0` 打补丁（`patches/@xterm__xterm@6.0.0.patch`，登记在 `pnpm-workspace.yaml`）：`RenderService.handleSelectionChanged` 在暂停时只记下选区，并置 `_needsSelectionRefresh` 和 `_needsFullRefresh` 后返回，恢复渲染时 `_renderRows` 按记下的状态补画选区（`setRenderer` 用的是同一套标记）。只改 Vite 用的 `lib/xterm.mjs`；那一行压缩代码约 160 KB，所以补丁文件有 300 多 KB。2026-10-17 在 Store 版里用运行时替换同样逻辑验证：后台终端滚动 10 次，原版重画 550 次，补丁后 0 次；后台期间设置的选区切回前台后画在正确的行上。
- **处理**：升级 xterm.js 时 pnpm 会因补丁打不上而报错。届时看上游是否已在暂停时推迟 `handleSelectionChanged`：已修复就从补丁里去掉这一处，补丁里的另一处（见下面输入法一项）也不需要了，才删掉 `patches/` 和 `pnpm-workspace.yaml` 里的 `patchedDependencies`；还没有就在新版本上重做补丁。都用下面的方法确认后台终端滚动时不再重建行。`@xterm/addon-*` 要和 xterm.js 一起升到配套的版本。
- **验证方法**：Store 版要带调试端口启动，得在包的上下文里先启动 `cmd`，由它设置环境变量再启动应用（`Invoke-CommandInDesktopPackage` 不会传递调用方的环境变量）：`Invoke-CommandInDesktopPackage -PackageFamilyName BoyinChen.ZShell_0mrn21pkbrd8j -AppId ZShell -Command cmd.exe -Args '/c set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333&& start "" "<安装目录>\zshell.exe"'`。之后用 Node 直接连 `/json/list` 里的 `webSocketDebuggerUrl`（不带 Origin，WebView2 会拒绝带 Origin 的连接），用 `Runtime.evaluate` 在后台终端的 `.xterm-rows` 上挂 MutationObserver 计数，同时从 React fiber 找到它的 `Terminal`，订阅 `onScroll` 对照；没有输出时可以用 `term.scrollLines()` 触发滚动。Chrome 要看 DevTools，用 `chrome://inspect` 添加 `localhost:9333`。
- **登记**：2026-10-17。

### macOS：开着输入法时，按 Shift 打的第一个字丢失（已用补丁绕过，等 xterm.js 修复后去掉）

- **现象**：终端里用中文输入法打 `？`、`！`、`：` 等要按 Shift 的标点，第一次按下没有反应，再按一次才出来。日文输入法下的 `#`、`"` 也一样。
- **原因**：WebKit 开着输入法时，输入法提交的字符先以 `input` 事件到达，字符自己的 keydown 在后面。xterm.js 6.0.0 的 `_keyDown` 对任何 keydown 都置 `_keyDownSeen`，`_inputEvent` 见它为真就认为这个字会由 keydown 处理，丢掉 `input`；按住的 Shift 的 keydown 已经把它置上了。随后字符的 keydown（keyCode 229）走 `CompositionHelper._handleAnyTextareaChanges`，比对输入框前后的内容，但字符已经在里面，差值为空，也没补回来。字符的 keyup 把标志清掉，所以只丢第一次。
- **上游**：[xtermjs/xterm.js#5374](https://github.com/xtermjs/xterm.js/issues/5374)（未关闭），修复是 [xtermjs/xterm.js#6200](https://github.com/xtermjs/xterm.js/pull/6200)（2026-10-02 提出，未合入）：只按修饰键的 keydown 不置 `_keyDownSeen`。PR 说明快速连打时第二个字丢失的情况（#5887）不在修复范围内。
- **绕过**：在同一个 `patches/@xterm__xterm@6.0.0.patch` 里照 #6200 改：`_keyDown` 里的 `this._keyDownSeen=!0` 改成 `this._keyDownSeen=!Tc(e)`，`Tc` 是压缩后的 `wasModifierKeyOnlyEvent`（`_keyUp` 里也用它）。
- **处理**：升级 xterm.js 时看 #6200 或别的修复是否已经发布：已发布就从补丁里去掉这一处，否则在新版本上重做。在 macOS 上用拼音输入法在终端里按一次 Shift+/，应该立刻出现 `？`。
- **登记**：2026-10-21。

### 前端：切换标签页后第一帧终端空白（已绕过，等 xterm.js 修复后去掉）

- **现象**：切到一个标签页时，终端先空白一帧（约 16 毫秒），下一帧才显示内容。2026-10-17 在 Store 版 3.5.0 上用 CDP 的 `Page.startScreencast` 录到，隐藏页移出视口之前（只有 `visibility: hidden`）就有。
- **原因**：后台终端交出 WebGL 上下文（`src/lib/webgl.ts`），显示时 `drawWithWebgl` 重新加载 `WebglAddon`。DOM 渲染器的行立刻被移除，新的 WebGL 画布是空的，xterm.js 6.0.0 要到下一个动画帧才画它。有两个原因让这一帧赶不上：
  1. **画布差 1 像素被清空**：WebGL 渲染器的 `handleResize` 把画布设成「列数 × 单元格的设备像素」，CSS 尺寸是除以 dpr 后四舍五入的值；在小数缩放下，插件的 ResizeObserver（`device-pixel-content-box`）测出的实际尺寸差 1 像素（175% 时 1794×1140 → 1794×1139），`_setCanvasDevicePixelDimensions` 重设画布，画布被清空，只请求下一帧重画。整数缩放下没有这一条。
  2. **显示时终端还在暂停**：隐藏页在视口外，RenderService 是暂停的；移回视口后，IntersectionObserver 的回调要等这一帧画完才到，这一帧里 `setRenderer` 请求的刷新因为暂停被推迟。任何缩放下都有，前提是隐藏页移出视口（`styles/terminals.css`）。
- **上游**：
  - 第 1 条是 [xtermjs/xterm.js#4922](https://github.com/xtermjs/xterm.js/issues/4922)，由 [xtermjs/xterm.js#5529](https://github.com/xtermjs/xterm.js/pull/5529) 修复（2025-12-28 合入 master，晚于 6.0.0）：重设画布后发出 `sync: true`，同步重画。6.1.0-beta 已包含，正式版尚未发布。
  - 第 2 条是我们提的 [xtermjs/xterm.js#6239](https://github.com/xtermjs/xterm.js/issues/6239)（2026-10-17 提出，之前上游没有人报过），修复是 [xtermjs/xterm.js#6240](https://github.com/xtermjs/xterm.js/pull/6240)，等待评审。master 的 `RenderService.refreshRows` 先看 `_isPaused`，暂停时连同步请求一起丢掉，`setRenderer` 的刷新也要等暂停结束，所以只升到 6.1.0，有隐藏页移出视口时仍会空白一帧。正确的修法是画布已经空了时不因暂停而不画：`refreshRows` 收到 `sync` 时先执行 `_pausedResizeTask` 再同步绘制，`setRenderer` 同步画一次。两处缺一不可：只改前者，整数缩放下换渲染器后仍空白；只改后者，小数缩放下画布随后又被清空。#6240 就是这样改的，附了单元测试；issue 里有最小复现（npm 6.1.0-beta.315 上每次切换都空白一帧）。
- **关注**：
  - `@xterm/xterm` 6.1.0 正式版的发布。它只带第 1 条的修复，绕过要保留。
  - #6239 和 #6240：评审意见（要改补丁时，从 fork `Bokjan/xterm.js` 的 `paused-blank-render` 分支继续），是否合入、进了哪个版本（看 PR 的 milestone 或 release notes）。
  - 维护者如果选了别的修法，要看它能不能让显示的那一帧就有内容。例如只在 IntersectionObserver 恢复渲染时同步补画是不够的：回调本来就晚一帧，补画时那一帧已经空白地画出去了。
- **绕过**：`webgl.ts` 加载插件后，给新的 WebGL 画布再挂一个 ResizeObserver。它创建在插件的之后，同一轮回调里排在插件后面，也就是画布刚被重设之后、这一帧绘制之前；第一次回调时断开，并调用私有的 `_core._renderService._renderRows` 同步画整屏，这个函数不看暂停。可选链访问，xterm.js 内部改名时什么都不做，空白帧又会出现，不会报错。图集重建时重新加载插件也走这里。
- **验证**：在临时目录里搭测试页（用仓库里打过补丁的 xterm.js 和 `webgl.ts`，照应用的 `.tab-page` CSS 放两个终端，切换时先改 class，再释放旧终端的 WebGL、加载新终端的），在独立配置目录的 Chrome 里用 `--force-device-scale-factor` 设缩放，用 CDP 录屏，按两个终端的文字颜色把每帧分成「A」「B」「空白」。Chrome 要加 `--disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows`，否则窗口被遮住后 rAF 停止。每种情形切换 6 次，空白帧数：

  | 缩放 / 隐藏方式 | 修改前 | 修改后 |
  |---|---|---|
  | 175%，visibility | 6 | 0 |
  | 175%，移出视口 | 6 | 0 |
  | 100%，visibility | 0 | 0 |
  | 100%，移出视口 | 6 | 0 |

  Windows 上不能用隔离的 HOME 跑开发版：配置目录取自系统的已知文件夹（`%APPDATA%\ZShell`），不看 `HOME`。
- **每次升级 xterm.js 时**：先确认 `RenderService` 里还有 `_renderRows`，而且仍然不看暂停、直接绘制。改名或改了行为时绕过会静默失效（可选链什么都不做），要按新版本找到对应的同步绘制入口，改写调用。然后按上面的方法录屏，四种情形都应为 0。
- **移除**（上游两条都修好、ZShell 已升到包含两处修复的版本之后）：
  1. 删掉 `src/lib/webgl.ts` 里绕过的代码：`Internals` 接口，`drawWithWebgl` 里的 `sized` 变量，`load()` 里给新画布挂 ResizeObserver 的那段（连同上面的注释），以及 `release()` 里断开 `sized` 的两行。
  2. 去掉绕过后录屏：四种情形都应为 0。任一情形出现空白帧，说明上游没修全，恢复绕过，到上游的 issue 里说明。
  3. 删掉这一条，提交信息里写明上游修复的版本和 PR。
- **登记**：2026-10-17。

## 暂缓

### SFTP：文件夹里的符号链接查完才显示列表

- **现象**：`sftp::list` 先读目录，再对每个符号链接 `metadata` 一次（16 路并发，`SYMLINK_LOOKUPS`），全部查完才返回。`/usr/lib`、`node_modules` 这类有成百上千个链接的文件夹，面板要等到查完才出现：1000 个链接、往返 100 ms 时约多等 6 秒。
- **为什么不直接「先显示再补」**：链接是否指向文件夹决定它排在「文件夹在前」的哪一组，也决定双击是进入还是打开、路径补全里有没有它（`lib/pathCompletion.ts`），修改权限时改的也是目标的权限（`targetPermissions`）。先显示的话，补上之前行会跳位置，双击可能走错。
- **要做时**：列表先返回，链接标成「目标未知」（排在文件夹之后、文件之前，双击时先查一次再决定）；目标按批推给前端（事件或分页命令），面板合并时保持滚动位置和选中项。切换文件夹时丢掉上一个文件夹还没查完的结果。
- **登记**：2026-10-19（评审 R1）。

### 保险库：每次修改整份重写

- **现状**：`Vault::update` 克隆整份数据，改完序列化、加密，再解密比对（`envelope::write`），经临时文件和改名整份写出 `vault.json`。改一个会话也是整份。
- **为什么不改**：个人使用的会话和密码是几 KiB 到几十 KiB，整份写一次是毫秒级，而且只在用户保存时发生。增量写要么每条单独加密，要么改成追加日志再定期合并，都要改文件格式和崩溃时的恢复逻辑，还要保证旧版本能读或明确拒绝。
- **何时再看**：保险库到了 MiB 级，或者出现高频写入（比如每次连接都记录点什么）时。
- **登记**：2026-10-19（评审 R14）。

### 串口：Windows 上改用重叠 I/O

- **现状**：Windows 上每个串口会话用两个线程，句柄是同步的，读和写轮流占用：读超时设成 10 ms，好让打字不必等读完，代价是每个会话每秒醒 100 次（`serial/mod.rs` 的 `READ_TIMEOUT`）。写入用的第二个句柄要用 `not_inherited` 防止被子进程继承。
- **要做时**：自己 `CreateFileW(..., FILE_FLAG_OVERLAPPED)` 打开，用 `COMPort::from_raw_handle` 包起来，继续用 serialport 设 DCB、break、查可读字节数（`from_raw_handle` 不做 `open` 里的 DCB 初始化，要自己补）。读超时设成「有一个字节就返回，否则一直等」，读写两个线程都等 `WaitForMultipleObjects([完成事件, 停止事件])`，关闭时置停止事件再 `CancelIoEx`。不用 tokio：它在 Windows 上不支持任意句柄的 IOCP。结果是空闲时不再唤醒，读写不再轮流，第二个句柄和 `not_inherited` 都不再需要。
- **为什么暂缓**：只能在 Windows 上用真实设备或 com0com 这类虚拟串口对实测，2026-10-19 手边没有；CI 只能确认能编译。Unix 那一半（tokio）已经做了，见提交「perf(serial)」。
- **登记**：2026-10-19（评审 T3）。

## 未查明

### macOS：退出全屏后窗口不见了（偶现）

- **现象**：在内置屏上点绿灯进入全屏，退出，再进入全屏后退出，窗口就看不到了：不是空白窗口，而是什么都没有。Dock 里仍有图标，切回 ZShell 也找不回窗口，按 ⌘W 能把它关掉（应用随之退出）。所以窗口还在，只是不显示，也没法调到前面。2026-10-14 测试 3.2.0 开发版（隔离 HOME，单窗口，没有标签页）时遇到一次。
- **已排除**：同一天在同一台机器上没能复现，包括：用菜单来回切换全屏 5 次（含快速连续切换）；全屏时双击标题栏（系统设置是双击缩放）；用鼠标点绿灯进入全屏，鼠标移到顶部滑出标题栏后点绿灯退出，做两轮。系统日志里没有相关记录。代码里没有在全屏切换前后移动或隐藏窗口的逻辑，全屏切换时只有 `watch_full_screen`（`window/mod.rs`）更新菜单栏全屏项的文字、在 200 毫秒后把焦点交还 WebView，以及页面根据 `data-fullscreen` 显示或隐藏左上角的应用菜单按钮。
- **下次遇到时**：在 ⌘W 之前先记下 `CGWindowListCopyWindowInfo` 里这个窗口的位置、层级、透明度和 `kCGWindowIsOnscreen`，再看 Mission Control 里是否还留着一个全屏 Space；记下是第几次切换、用什么方式退出的。
- **登记**：2026-10-14。

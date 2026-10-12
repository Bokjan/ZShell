# 待办

本文记录已经查明、但暂时不处理的小事：在等上游发版的问题，以及不值得单开里程碑的修复。功能规划写在 [ROADMAP.md](ROADMAP.md)。一项处理完就从这里删掉，经过写进提交信息。

## 等待上游

### SSH：认证完成前 keepalive 定时器空转，占满一个核

- **现象**：密码、口令或验证码提示停在终端里超过 keepalive 间隔（默认 30 秒），或者在等 Duo 推送时，ZShell 占满一个 CPU 核。用户输入、服务器断开（`LoginGraceTime` 默认 120 秒）或关掉标签后恢复。
- **原因**：russh 0.64.1 客户端主循环（`client/mod.rs` 约 1426–1438 行）在 `keepalive_timer` 到期时，只有状态是 `Authenticated` 才发 keepalive；定时器只在 `received_data || sent_keepalive` 时重置（约 1515–1521 行）。认证完成前到期，这个分支什么都不做，也不重置。tokio 的 `Sleep` 到期后每次 poll 都是 Ready，`select!` 就一直空转，直到对端再发数据。`ssh/mod.rs` 在连接时就把 `keepalive_interval` 交给 russh。2026-10-12 用同一份 `Cargo.lock` 写小程序复现：keepalive 设 1 秒，认证前空等 4 秒用掉 3.16 秒 CPU，不设 keepalive 时是 0。
- **上游修复**：还没有。问题由 [Eugeny/russh#642](https://github.com/Eugeny/russh/pull/642)（认证前不发 keepalive，2026-02-28 合入）引入；2026-10-12 查过，crates.io 上最新仍是 0.64.1，main 分支（最近一次修改这个文件是 2026-10-04 的 `a0ded76`）也没有改，上游没有相关 issue。修法是未认证时到期也重置定时器，一两行。
- **处理**：russh 发布修复后，在 `src-tauri/` 下升级 russh，用上面的方法确认认证提示停留时 CPU 不再升高，作为补丁版本发布。
- **上游 issue 的素材**（我们不主动提；以后需要时照此写，上游用英文）：
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

### Windows：最大化后按 F11，任务栏位置留黑边

- **现象**：窗口先最大化，再按 F11 进入全屏，屏幕底部原来任务栏的那一条变成黑边或透明。窗口没有最大化时进入全屏是正常的。
- **原因**：tao 0.37.1 的问题。Windows 上主窗口没有原生边框，客户区由 tao 在 `WM_NCCALCSIZE` 里自己计算：窗口最大化时，它把客户区裁成显示器的工作区（去掉任务栏）。进入全屏时 tao 没有清掉最大化状态，客户区仍被裁成工作区，任务栏那一条就成了没有绘制的非客户区。
- **上游修复**：[tauri-apps/tao#1353](https://github.com/tauri-apps/tao/pull/1353)（提交 [`2831c7d`](https://github.com/tauri-apps/tao/commit/2831c7d69d144a062cc8b99a070c579a0787641b)），全屏时 `WM_NCCALCSIZE` 直接返回 0。2026-09-30 合入 dev 分支，排在待发的 [tauri-apps/tao#1357](https://github.com/tauri-apps/tao/pull/1357) 里，会随 0.37.2 发布。
- **处理**：0.37.2 发布后，在 `src-tauri/` 下执行 `cargo update -p tao`。tauri-runtime-wry 2.12.1 对 tao 的要求是 `^0.37.0`，不用等 Tauri 发版。升级后到 Windows 上确认问题消失，作为补丁版本发布。
- **备选**：如果上游迟迟不发版，在 `src-tauri/src/window.rs` 里用 `SetWindowSubclass` 给主窗口挂一个子类，在窗口没有 `WS_CAPTION` 时拦截 `WM_NCCALCSIZE` 并返回 0（tao 只在全屏时去掉 `WS_CAPTION`）。这和上游修复的效果相同，需要给 windows-sys 加上 `Win32_UI_Shell` feature；等 tao 升级后再删掉。
- **登记**：2026-10-11。

### Linux：依赖里的 glib 0.18 有一条中等级别的安全公告

- **现象**：Dependabot 报 `src-tauri/Cargo.lock` 里的 `glib` 0.18.5 不安全（[警告 #1](https://github.com/Bokjan/ZShell/security/dependabot/1)），要求 0.20.0 以上。问题出在 `glib::VariantStrIter` 的 `Iterator` / `DoubleEndedIterator` 实现，只有调用 `Variant::array_iter_str` 才会碰到。
- **原因**：glib 是 Tauri 在 Linux 上用的 GTK3 绑定（`gtk` 0.18、`webkit2gtk` 2.0，经 tao、wry、muda 引入）的底层，固定在 0.18。gtk-rs 的 GTK3 绑定已经停在 0.18 并归档，不会有兼容 glib 0.20 的版本；0.18 到 0.20 也是不兼容升级，`cargo update` 升不上去。
- **影响**：macOS 和 Windows 的包不编译这些 crate。2026-10-11 核对过，依赖树里引用 glib 的 21 个 crate 都没有调用 `array_iter_str`。该警告已在 GitHub 上以“代码未用到”关闭。
- **处理**：Tauri 把 Linux 后端迁到 GTK4 后，glib 会跟着升到 0.20 以上。支持 Linux 之前再确认一次：依赖树里还是 0.18 的话，重新检查有没有 crate 调用 `array_iter_str`。
- **登记**：2026-10-11。

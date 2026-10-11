# 待办

本文记录已经查明、但暂时不处理的小事：在等上游发版的问题，以及不值得单开里程碑的修复。功能规划写在 [ROADMAP.md](ROADMAP.md)。一项处理完就从这里删掉，经过写进提交信息。

## 等待上游

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

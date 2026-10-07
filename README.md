# ZShell

个人使用的跨平台（macOS / Windows）SSH 客户端，基于 Tauri 2 + Rust + xterm.js。

设计与里程碑见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 开发

依赖：Rust（stable）、Node.js、pnpm。

```bash
pnpm install
pnpm tauri dev      # 开发模式
pnpm tauri build    # 打包
```

## 推荐 IDE

[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

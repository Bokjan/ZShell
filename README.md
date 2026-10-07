# ZShell

A cross-platform (macOS / Windows) SSH client for personal use, built with Tauri 2, Rust and xterm.js.

Design notes and milestones (in Chinese): [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Development

Requirements: Rust (stable), Node.js, pnpm.

```bash
pnpm install
pnpm tauri dev      # run in development mode
pnpm tauri build    # build release bundles
```

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

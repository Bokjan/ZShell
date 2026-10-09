# ZShell

A cross-platform (macOS / Windows) SSH client for personal use, meant as an alternative to Xshell. Tauri 2 with a Rust + tokio backend (`src-tauri/`, russh for SSH and SFTP) and a React + TypeScript + xterm.js frontend (`src/`).

## Documentation

The docs are in Chinese and are the source of truth for plans and design:

- `docs/ROADMAP.md`: current state, milestones and the design notes for unimplemented features. Read the relevant milestone before starting a feature.
- `docs/ARCHITECTURE.md`: overall structure and cross-module design decisions.
- `docs/I18N.md`: language policy and how user-visible text is handled.
- `docs/DEVELOPMENT.md`: commands, end-to-end testing, CI and releases.

When a feature lands: mark its milestone ✅ in ROADMAP.md, move cross-module design into ARCHITECTURE.md, and keep module-level details (edge cases, library pitfalls) in code comments rather than the docs.

## Language

- Files under `docs/` are written in Chinese. Everything else is English: code, comments, UI text, terminal prompts and error messages from the backend, README, CHANGELOG, commit messages.
- The maintainer talks to you in Chinese; reply in Chinese.
- Never hard-code user-visible text. Frontend: add keys to `src/locales/en.json` and use `t()` (keys are type-checked by `tsc`). Backend: use `t!("key")` / `Error::new("code")` with keys in `src-tauri/locales/en.json` (`cargo test` checks coverage). Use full sentences with `{name}` parameters, never concatenated fragments, and branch on error codes, never on message text.

## Commands

```bash
pnpm build                                  # tsc + vite build
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
pnpm tauri dev                              # run the app
```

Run the first two before committing; the Windows CI job runs the same clippy and tests. Windows-only code (ConPTY, the OpenSSH agent pipe, Pageant) cannot be compiled on macOS, so CI is the only check for it.

## Testing

Never touch the maintainer's real configuration (`profiles.json`, `settings.json`, `~/.ssh`) during tests. For anything that needs a live SSH server, run the debug app with an isolated `HOME` and a temporary `sshd`, as described in the end-to-end testing section of `docs/DEVELOPMENT.md`. Kill only the processes you started; the maintainer may have their own instance open.

## Commits

- Conventional Commits subjects, with an optional scope: `feat(sftp): …`, `fix(macos): …`, `docs: …`, `ci: …`, `chore: …`, `refactor: …`. Only `feat`, `fix` and `perf` (and breaking changes) reach `CHANGELOG.md`, so pick the type accordingly.
- Do not hard-wrap the body: each paragraph or bullet stays on one line.
- Commit with the repository's local git identity (check `git config user.email`).

## Conventions

- The app version lives only in `src-tauri/Cargo.toml` and follows semantic versioning. A milestone or new feature is a minor version, fixes and small improvements are patch versions, and configuration files stay backward compatible. Versions are not bumped per commit: when planning a change to code in `src/` or `src-tauri/`, propose a version plan (which version, and which changes it covers) and agree on it with the maintainer before starting work. Versions are set with `scripts/release.sh prepare` / `tag` and every version is tagged, but a release is only built with `scripts/release.sh publish` (see "发布" in `docs/DEVELOPMENT.md`). Ask before pushing a version tag, and before publishing.
- Styles use the CSS theme variables; no hard-coded colors.
- `PRIVACY.md` (the Microsoft Store privacy policy) states exactly what the app stores and what it connects to. When a change affects either, update it and its "Last updated" date.
- App shortcuts are intercepted in the capture phase so the terminal never sees them. On Windows, shortcuts that would clash with the shell use Ctrl+Shift (Ctrl+F, Ctrl+E and so on belong to the shell).
- SSH prompts (host keys, passwords, passphrases, keyboard-interactive) are asked inline in the terminal, as OpenSSH does, not in dialogs. Follow OpenSSH's behavior where there is one.

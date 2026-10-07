//! The default local shell and the environment it starts with.

#[cfg(any(windows, target_os = "macos", test))]
use std::ffi::OsStr;
use std::path::Path;
#[cfg(any(windows, test))]
use std::path::PathBuf;

use portable_pty::CommandBuilder;

/// Variables set by other terminal emulators, inherited when ZShell itself was started from
/// one; they would mislead shell startup files.
const FOREIGN_TERMINAL_VARS: &[&str] =
    &["TERM_SESSION_ID", "ITERM_SESSION_ID", "ITERM_PROFILE", "LC_TERMINAL", "LC_TERMINAL_VERSION"];

/// A shell command ready to spawn.
pub struct Shell {
    pub command: CommandBuilder,
    /// The program, for error messages.
    pub program: String,
}

impl Shell {
    /// Short name for display, e.g. "zsh" or "pwsh".
    pub fn name(&self) -> String {
        display_name(&self.program)
    }
}

/// The user's default shell with ZShell's terminal environment.
pub fn default_shell() -> Shell {
    let (mut command, program) = default_command();
    configure(&mut command);
    Shell { command, program }
}

/// The user's login shell (`$SHELL`, else the password database), started as a login shell
/// (argv[0] `-zsh`) like Terminal.app does: apps started from Finder only get a minimal
/// PATH from launchd, and `/etc/zprofile` / `~/.zprofile` set up the real one.
#[cfg(unix)]
fn default_command() -> (CommandBuilder, String) {
    let command = CommandBuilder::new_default_prog();
    let program = command.get_shell();
    (command, program)
}

/// PowerShell 7 if installed, else Windows PowerShell, which ships with Windows.
#[cfg(windows)]
fn default_command() -> (CommandBuilder, String) {
    let program = std::env::var_os("PATH").and_then(|path| find_in_path(&path, "pwsh.exe")).unwrap_or_else(|| {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
    });
    let mut command = CommandBuilder::new(&program);
    command.arg("-NoLogo");
    (command, program.to_string_lossy().into_owned())
}

/// Sets the variables programs use to detect the terminal's capabilities.
pub fn configure(command: &mut CommandBuilder) {
    for name in FOREIGN_TERMINAL_VARS {
        command.env_remove(name);
    }
    // Windows programs don't use TERM; Windows Terminal doesn't set it either.
    #[cfg(unix)]
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "ZShell");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    // Apps started from Finder have no locale variables; without them the shell and most
    // tools fall back to ASCII. Like Terminal.app, derive one from the system language.
    #[cfg(target_os = "macos")]
    if ["LC_ALL", "LC_CTYPE", "LANG"].iter().all(|name| command.get_env(name).is_none_or(OsStr::is_empty)) {
        let tag = sys_locale::get_locale().unwrap_or_default();
        command.env("LANG", posix_locale(&tag, |name| Path::new("/usr/share/locale").join(name).is_dir()));
    }
}

fn display_name(program: &str) -> String {
    Path::new(program).file_stem().map_or_else(|| program.to_owned(), |stem| stem.to_string_lossy().into_owned())
}

/// The first `file` in the directories of a PATH-style list. App execution aliases (e.g.
/// `pwsh.exe` installed from the Microsoft Store) are reparse points that cannot be
/// followed, so the entry itself is checked.
#[cfg(any(windows, test))]
fn find_in_path(path: &OsStr, file: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(file))
        .find(|candidate| std::fs::symlink_metadata(candidate).is_ok_and(|meta| !meta.is_dir()))
}

/// A BCP 47 language tag (`en-US`, `zh-Hans-CN`) as a POSIX UTF-8 locale (`en_US.UTF-8`,
/// `zh_CN.UTF-8`) if `exists` says the system has it, else `en_US.UTF-8`.
#[cfg(any(target_os = "macos", test))]
fn posix_locale(tag: &str, exists: impl Fn(&str) -> bool) -> String {
    let tag = tag.split(['@', '.']).next().unwrap_or_default();
    let mut parts = tag.split(['-', '_']);
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    let region = parts.find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()));
    if let Some(region) = region.filter(|_| !language.is_empty()) {
        let name = format!("{language}_{}.UTF-8", region.to_ascii_uppercase());
        if exists(&name) {
            return name;
        }
    }
    "en_US.UTF-8".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_language_tags_to_posix_locales() {
        let exists = |name: &str| ["en_US.UTF-8", "zh_CN.UTF-8", "de_DE.UTF-8"].contains(&name);
        assert_eq!(posix_locale("en-US", exists), "en_US.UTF-8");
        assert_eq!(posix_locale("zh-Hans-CN", exists), "zh_CN.UTF-8");
        assert_eq!(posix_locale("de_DE@euro", exists), "de_DE.UTF-8");
        // Unknown to the system, without a region, or empty.
        assert_eq!(posix_locale("fr-FR", exists), "en_US.UTF-8");
        assert_eq!(posix_locale("zh-Hans", exists), "en_US.UTF-8");
        assert_eq!(posix_locale("", exists), "en_US.UTF-8");
    }

    #[test]
    fn display_names() {
        assert_eq!(display_name("/bin/zsh"), "zsh");
        assert_eq!(display_name("/opt/homebrew/bin/fish"), "fish");
        assert_eq!(display_name("pwsh.exe"), "pwsh");
    }

    #[test]
    fn finds_programs_in_path() {
        let root = std::env::temp_dir().join(format!("zshell-path-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(b.join("dir.exe")).unwrap();
        std::fs::write(b.join("pwsh.exe"), "").unwrap();
        let path = std::env::join_paths([&a, &b]).unwrap();
        assert_eq!(find_in_path(&path, "pwsh.exe"), Some(b.join("pwsh.exe")));
        assert_eq!(find_in_path(&path, "dir.exe"), None);
        assert_eq!(find_in_path(&path, "missing.exe"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}

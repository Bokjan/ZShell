//! The local shells, what each runs (the default one, a program, a WSL distribution) and the
//! environment it starts with.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;

use crate::error::{Error, Result};
use crate::model::{EnvVar, LocalOptions, LocalShell};

/// Variables set by other terminal emulators, inherited when ZShell itself was started from
/// one; they would mislead shell startup files. Also the activation token the desktop started
/// ZShell with (Linux), which a program started from the shell would otherwise take as its own.
const FOREIGN_TERMINAL_VARS: &[&str] = &[
    "TERM_SESSION_ID",
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "XDG_ACTIVATION_TOKEN",
    "DESKTOP_STARTUP_ID",
];

/// A shell command ready to spawn.
pub struct Shell {
    pub command: CommandBuilder,
    /// The program, for error messages.
    pub program: String,
    /// What it is called, e.g. "zsh", "pwsh" or a WSL distribution's name.
    name: String,
}

impl Shell {
    /// Runs `program` with `command`, named after it.
    pub fn new(command: CommandBuilder, program: String) -> Self {
        Self { name: display_name(&program), command, program }
    }

    /// Short name for display, e.g. "zsh" or "pwsh".
    pub fn name(&self) -> String {
        self.name.clone()
    }
}

/// Whether this system lets ZShell start a local shell.
#[cfg(not(windows))]
pub fn local_shells_allowed() -> bool {
    true
}

/// Windows in S mode only runs Store apps and blocks command-line shells such as PowerShell
/// and cmd. An error (the API is missing before Windows 10 1803) counts as no S mode.
#[cfg(windows)]
pub fn local_shells_allowed() -> bool {
    !windows::System::Profile::WindowsIntegrityPolicy::IsEnabled().unwrap_or(false)
}

/// The user's default shell with ZShell's terminal environment.
pub fn default_shell() -> Shell {
    let (mut command, program) = default_command();
    configure(&mut command);
    Shell::new(command, program)
}

/// What a local terminal runs (`local`), with ZShell's terminal environment and the user's
/// variables over it, in its start folder. Fails, rather than starting somewhere else, when
/// the program or the folder isn't there, or for WSL on another system. Blocks: it looks for
/// files, in `PATH` too, where a network drive may not answer.
pub fn local_shell(local: &LocalOptions) -> Result<Shell> {
    let home = std::env::home_dir();
    // WSL takes its start folder as an argument.
    let dir = match local.shell {
        LocalShell::Wsl { .. } => None,
        _ => start_dir(&local.start_dir, home.as_deref()),
    };
    let mut shell = match &local.shell {
        LocalShell::Default => default_shell(),
        LocalShell::Program { path, args } => {
            let home = home.as_deref().unwrap_or(Path::new(""));
            let paths = env_path(&local.env).or_else(|| std::env::var_os("PATH"));
            let program = find_program(path, dir.as_deref().unwrap_or(home), home, paths.as_deref())
                .ok_or_else(|| Error::new("local.programNotFound").param("path", path))?;
            let mut command = CommandBuilder::new(&program);
            command.args(args);
            configure(&mut command);
            #[cfg(windows)]
            if let Some(msystem) = git_msystem(&program, |path| path.exists()) {
                // Git Bash's profile goes to the home folder unless asked to stay where it starts.
                command.env("CHERE_INVOKING", "1");
                command.env("MSYSTEM", msystem);
            }
            Shell::new(command, program.display().to_string())
        }
        LocalShell::Wsl { distro } => {
            #[cfg(windows)]
            let shell = wsl(distro, &local.start_dir, &local.env);
            #[cfg(not(windows))]
            let shell = Err(Error::new("local.wslUnavailable").param("name", distro));
            shell?
        }
    };
    for EnvVar { name, value } in &local.env {
        shell.command.env(name, value);
    }
    if let Some(dir) = dir {
        if !dir.is_dir() {
            return Err(Error::new("local.startDirNotFound").param("path", dir.display()));
        }
        shell.command.cwd(dir);
    }
    Ok(shell)
}

/// `path` with `~` for `home` (`~`, `~/…`, and `~\…` on Windows); not someone else's (`~bob`).
fn expand_home(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\").filter(|_| cfg!(windows))) {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_owned(),
        None => PathBuf::from(path),
    }
}

/// The folder a local terminal starts in: `dir` with `~` for `home`, and relative to it;
/// `None` for the home folder (`dir` empty), where the terminal starts anyway.
fn start_dir(dir: &str, home: Option<&Path>) -> Option<PathBuf> {
    let home = home.unwrap_or(Path::new(""));
    (!dir.is_empty()).then(|| home.join(expand_home(dir, home)))
}

/// The `PATH` a local session's variables `env` set, if they do (the last one, as it wins).
fn env_path(env: &[EnvVar]) -> Option<std::ffi::OsString> {
    let is_path = |name: &str| if cfg!(windows) { name.eq_ignore_ascii_case("PATH") } else { name == "PATH" };
    env.iter().rev().find(|var| is_path(&var.name)).map(|var| var.value.clone().into())
}

/// Where the program `path` is, if it is there: `~` for `home`; a relative path in the folder
/// the terminal starts in (`dir`); a bare name in the folders of `paths` (the `PATH` it runs
/// with), on Windows also with `.exe`. Always from `dir`, never relative, so that what is
/// started is what was found: given a relative path, `CommandBuilder` would look for it itself,
/// by rules of its own on each system (on Windows, from ZShell's own current folder).
fn find_program(path: &str, dir: &Path, home: &Path, paths: Option<&OsStr>) -> Option<PathBuf> {
    let program = expand_home(path, home);
    if program.components().count() > 1 || program.is_absolute() {
        return Some(dir.join(program)).filter(|program| is_file(program));
    }
    // From `dir` too: `PATH` may have relative folders.
    let in_path = |file: &str| std::env::split_paths(paths?).map(|folder| dir.join(folder).join(file)).find(|candidate| is_file(candidate));
    let exe = cfg!(windows) && program.extension().is_none();
    in_path(path).or_else(|| exe.then(|| in_path(&format!("{path}.exe"))).flatten())
}

/// Whether `path` is there and not a folder: the entry itself, as Windows' app execution
/// aliases can't be followed (see `find_in_path`).
fn is_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| !meta.is_dir())
}

/// A WSL distribution, in `dir` (its home folder if empty), with ZShell's terminal variables
/// and the user's `env` passed through `WSLENV`.
#[cfg(windows)]
fn wsl(distro: &str, dir: &str, env: &[EnvVar]) -> Result<Shell> {
    let program = system32().join("wsl.exe");
    if !program.is_file() {
        return Err(Error::new("local.wslUnavailable").param("name", distro));
    }
    let mut command = CommandBuilder::new(&program);
    command.args(["-d", distro, "--cd", if dir.is_empty() { "~" } else { dir }]);
    configure(&mut command);
    let names: Vec<&str> = TERMINAL_VARS.iter().copied().chain(env.iter().map(|var| var.name.as_str())).collect();
    let existing = command.get_env("WSLENV").and_then(OsStr::to_str).map(str::to_owned);
    command.env("WSLENV", wslenv(existing.as_deref(), &names));
    Ok(Shell { name: distro.to_owned(), program: program.display().to_string(), command })
}

/// The variables ZShell sets for the program in the terminal (see `configure`), which WSL
/// passes on only through `WSLENV`.
#[cfg(any(windows, test))]
const TERMINAL_VARS: &[&str] = &["TERM_PROGRAM", "TERM_PROGRAM_VERSION", "COLORTERM"];

/// `WSLENV` with `names` added: the variables Windows passes to a WSL distribution, separated
/// by `:`, each with flags after a `/` (`existing` keeps its own).
#[cfg(any(windows, test))]
fn wslenv(existing: Option<&str>, names: &[&str]) -> String {
    let mut list: Vec<&str> = existing.unwrap_or_default().split(':').filter(|entry| !entry.is_empty()).collect();
    for &name in names {
        let listed = list.iter().any(|entry| entry.split('/').next().is_some_and(|listed| listed.eq_ignore_ascii_case(name)));
        if !listed {
            list.push(name);
        }
    }
    list.join(":")
}

/// The `MSYSTEM` to run Git for Windows' bash with if `program` is it (`bin\bash.exe` or
/// `usr\bin\bash.exe` in a folder with `git-bash.exe` or `cmd\git.exe`), by the environment
/// it installed (`exists` tells which of its files are there), as `git-bash.exe` sets it.
#[cfg(any(windows, test))]
fn git_msystem(program: &Path, exists: impl Fn(&Path) -> bool) -> Option<&'static str> {
    let is = |path: Option<&Path>, name: &str| path.and_then(Path::file_name).is_some_and(|file| file.eq_ignore_ascii_case(name));
    if !is(Some(program), "bash.exe") {
        return None;
    }
    let bin = program.parent().filter(|bin| is(Some(bin), "bin"))?;
    let root = bin.parent().filter(|usr| is(Some(usr), "usr")).and_then(Path::parent).or(bin.parent())?;
    if !exists(&root.join("git-bash.exe")) && !exists(&root.join("cmd").join("git.exe")) {
        return None;
    }
    Some(super::discover::git_msystem(|dir| exists(&root.join(dir))))
}

#[cfg(windows)]
pub(super) fn system32() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into())).join("System32")
}

/// PowerShell 7, if installed: `pwsh.exe` in `PATH`.
#[cfg(windows)]
pub(super) fn find_pwsh() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| find_in_path(&path, "pwsh.exe"))
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
    let program = find_pwsh().unwrap_or_else(|| system32().join(r"WindowsPowerShell\v1.0\powershell.exe"));
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

pub(super) fn display_name(program: &str) -> String {
    Path::new(program).file_stem().map_or_else(|| program.to_owned(), |stem| stem.to_string_lossy().into_owned())
}

/// The first `file` in the directories of a PATH-style list. App execution aliases (e.g.
/// `pwsh.exe` installed from the Microsoft Store) are reparse points that cannot be
/// followed, so the entry itself is checked.
#[cfg(any(windows, test))]
fn find_in_path(path: &OsStr, file: &str) -> Option<PathBuf> {
    std::env::split_paths(path).map(|dir| dir.join(file)).find(|candidate| is_file(candidate))
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
    fn start_folders_are_relative_to_home() {
        let home = Path::new("/home/me");
        assert_eq!(start_dir("", Some(home)), None);
        assert_eq!(start_dir("~", Some(home)), Some(home.to_owned()));
        assert_eq!(start_dir("~/src", Some(home)), Some(home.join("src")));
        assert_eq!(start_dir("src", Some(home)), Some(home.join("src")));
        let root = std::env::temp_dir();
        assert_eq!(start_dir(&root.display().to_string(), Some(home)), Some(root));
        // Someone else's home folder isn't expanded.
        assert_eq!(start_dir("~bob", Some(home)), Some(home.join("~bob")));
    }

    #[test]
    fn passes_terminal_variables_to_wsl() {
        assert_eq!(wslenv(None, TERMINAL_VARS), "TERM_PROGRAM:TERM_PROGRAM_VERSION:COLORTERM");
        // What is there stays, with its flags; nothing is listed twice.
        assert_eq!(wslenv(Some("USERPROFILE/p:colorterm"), &["COLORTERM", "EDITOR"]), "USERPROFILE/p:colorterm:EDITOR");
        assert_eq!(wslenv(Some(":A::"), &["B"]), "A:B");
    }

    #[test]
    fn tells_git_bash_by_its_installation() {
        let root = Path::new("C:").join("Program Files").join("Git");
        let has = |files: &'static [&'static str]| {
            let root = root.clone();
            move |path: &Path| files.iter().any(|file| root.join(file) == path)
        };
        let bash = root.join("bin").join("bash.exe");
        let usr_bash = root.join("usr").join("bin").join("bash.exe");
        assert_eq!(git_msystem(&bash, has(&["git-bash.exe", "mingw64"])), Some("MINGW64"));
        assert_eq!(git_msystem(&usr_bash, has(&["cmd/git.exe", "clangarm64"])), Some("CLANGARM64"));
        assert_eq!(git_msystem(&root.join("bin").join("BASH.EXE"), has(&["git-bash.exe"])), Some("MINGW64"));
        // Another bash, or a bash elsewhere.
        assert_eq!(git_msystem(&bash, has(&[])), None);
        assert_eq!(git_msystem(&root.join("bin").join("zsh.exe"), has(&["git-bash.exe"])), None);
        assert_eq!(git_msystem(&root.join("bash.exe"), has(&["git-bash.exe"])), None);
    }

    #[test]
    fn programs_are_found_where_the_terminal_starts() {
        let root = std::env::temp_dir().join(format!("zshell-programs-{}", std::process::id()));
        let (home, start, bin) = (root.join("home"), root.join("start"), root.join("bin"));
        for dir in [home.join("tools"), start.join("scripts"), bin.clone()] {
            std::fs::create_dir_all(dir).unwrap();
        }
        for file in [home.join("tools").join("fish"), start.join("scripts").join("run"), bin.join("tool")] {
            std::fs::write(file, "").unwrap();
        }
        let path = std::env::join_paths([&bin]).unwrap();
        let find = |program: &str, paths: Option<&OsStr>| find_program(program, &start, &home, paths);

        // A relative path from the start folder, not ZShell's own current folder.
        assert_eq!(find("scripts/run", None), Some(start.join("scripts").join("run")));
        assert_eq!(
            find(&Path::new(".").join("scripts").join("run").display().to_string(), None),
            Some(start.join(".").join("scripts").join("run"))
        );
        assert_eq!(find("run", None), None);
        // `~` for the home folder, as in the start folder.
        assert_eq!(find("~/tools/fish", None), Some(home.join("tools").join("fish")));
        assert_eq!(find("~", None), None);
        // A bare name in the `PATH` given, made full when that has relative folders.
        assert_eq!(find("tool", Some(&path)), Some(bin.join("tool")));
        assert_eq!(find("tool", None), None);
        assert_eq!(find("run", Some(OsStr::new("scripts"))), Some(start.join("scripts").join("run")));
        assert_eq!(find(&bin.join("tool").display().to_string(), None), Some(bin.join("tool")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_sessions_own_path_is_looked_in() {
        let var = |name: &str, value: &str| EnvVar { name: name.into(), value: value.into() };
        assert_eq!(env_path(&[]), None);
        assert_eq!(env_path(&[var("PATH", "/a"), var("EDITOR", "vi"), var("PATH", "/b")]), Some("/b".into()));
        assert_eq!(env_path(&[var("Path", "/c")]), cfg!(windows).then(|| "/c".into()));
    }

    #[test]
    fn programs_missing_are_not_found() {
        let missing = std::env::temp_dir().join("zshell-missing").join("shell");
        let here = std::env::temp_dir();
        assert_eq!(find_program(&missing.display().to_string(), &here, &here, None), None);
        let path = std::env::var_os("PATH");
        assert_eq!(find_program("zshell-no-such-shell", &here, &here, path.as_deref()), None);
        let local = LocalOptions::new(LocalShell::Program { path: missing.display().to_string(), args: Vec::new() });
        assert_eq!(local_shell(&local).err().map(|e| e.code().to_owned()).as_deref(), Some("local.programNotFound"));
        let local = LocalOptions { start_dir: missing.display().to_string(), ..LocalOptions::new(LocalShell::Default) };
        assert_eq!(local_shell(&local).err().map(|e| e.code().to_owned()).as_deref(), Some("local.startDirNotFound"));
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

//! The shells found on this computer, for the local terminal menu and the settings: on
//! Windows, PowerShell, cmd, Git Bash (from the registry, where its installer records it) and
//! the WSL distributions (from the registry too, rather than `wsl.exe -l`, which is slow and
//! writes UTF-16); on macOS and Linux, those `/etc/shells` lists that are there. Found again
//! only when asked (see `commands::session::local_shells`): kept meanwhile, never saved.

use std::path::{Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;

use crate::model::LocalShell;

/// A shell found here.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FoundShell {
    pub kind: FoundKind,
    /// What it is called: the program's name, or the WSL distribution's; the frontend names
    /// cmd in the user's language.
    pub name: String,
    pub shell: LocalShell,
}

/// What kind of shell it is. Each system finds only its own kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FoundKind {
    #[cfg_attr(not(windows), allow(dead_code))]
    PowerShell,
    #[cfg_attr(not(windows), allow(dead_code))]
    WindowsPowerShell,
    #[cfg_attr(not(windows), allow(dead_code))]
    Cmd,
    #[cfg_attr(not(windows), allow(dead_code))]
    GitBash,
    #[cfg_attr(not(windows), allow(dead_code))]
    Wsl,
    /// One of `/etc/shells`.
    #[cfg_attr(not(any(unix, test)), allow(dead_code))]
    Unix,
}

/// The shells found here: on Windows, the default one first; elsewhere, in the order of
/// `/etc/shells`.
pub fn discover() -> Vec<FoundShell> {
    #[cfg(windows)]
    return windows::discover();
    #[cfg(unix)]
    return unix::discover();
}

/// The paths of `/etc/shells` (its `text`): absolute ones, without comments.
#[cfg(any(unix, test))]
pub(super) fn etc_shells(text: &str) -> Vec<&str> {
    text.lines().map(|line| line.split('#').next().unwrap_or_default().trim()).filter(|line| line.starts_with('/')).collect()
}

/// `paths` that are there (`real` gives where each leads, `None` if it isn't there), each
/// program once: `/bin/zsh` and `/usr/bin/zsh` are one where `/bin` links to `/usr/bin`. Named
/// by the program, and by its folder too where two programs share a name.
#[cfg(any(unix, test))]
pub(super) fn unique_shells(paths: &[&str], real: impl Fn(&str) -> Option<PathBuf>, login: bool) -> Vec<FoundShell> {
    let mut seen = Vec::new();
    let mut found: Vec<(String, &str)> = Vec::new();
    for &path in paths {
        let Some(target) = real(path) else { continue };
        if seen.contains(&target) {
            continue;
        }
        seen.push(target);
        found.push((super::shell::display_name(path), path));
    }
    let shared = |name: &str| found.iter().filter(|(other, _)| other == name).count() > 1;
    found
        .iter()
        .map(|(name, path)| {
            let folder = Path::new(path).parent().map(|dir| dir.display().to_string()).unwrap_or_default();
            let name = if shared(name) { format!("{name} ({folder})") } else { name.clone() };
            let args = if login { vec!["-l".to_owned()] } else { Vec::new() };
            FoundShell { kind: FoundKind::Unix, name, shell: LocalShell::Program { path: (*path).to_owned(), args } }
        })
        .collect()
}

/// A WSL distribution recorded under `Lxss`: its key's name (a GUID), and its values.
#[cfg(any(windows, test))]
#[derive(Debug, Default)]
pub(super) struct Distribution {
    pub key: String,
    pub name: Option<String>,
    /// 1 once installed; other values while it is being installed, converted or removed.
    pub state: Option<u32>,
}

/// The WSL distributions to offer, by name, the default one (`default`, a key's name) first:
/// installed, and not those Docker Desktop runs itself on, which Windows Terminal hides too.
#[cfg(any(windows, test))]
pub(super) fn wsl_distributions(mut distributions: Vec<Distribution>, default: Option<&str>) -> Vec<String> {
    let is_default = |d: &Distribution| default.is_some_and(|key| key.eq_ignore_ascii_case(&d.key));
    distributions.sort_by_key(|d| !is_default(d));
    distributions
        .into_iter()
        .filter(|d| d.state.is_none_or(|state| state == 1))
        .filter_map(|d| d.name)
        .filter(|name| !name.is_empty() && !name.starts_with("docker-desktop"))
        .collect()
}

/// The `MSYSTEM` a Git for Windows installation runs its shell in, by the environment it
/// installed (`has` tells which of its folders are there): MSYS2's names, as `git-bash.exe`
/// sets it.
#[cfg(any(windows, test))]
pub(super) fn git_msystem(has: impl Fn(&str) -> bool) -> &'static str {
    if has("clangarm64") {
        "CLANGARM64"
    } else if has("mingw64") {
        "MINGW64"
    } else if has("mingw32") {
        "MINGW32"
    } else {
        "MINGW64"
    }
}

#[cfg(unix)]
mod unix {
    use super::*;

    pub fn discover() -> Vec<FoundShell> {
        let text = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        // A login shell on macOS, as Terminal.app starts; not on Linux, as its terminals don't.
        unique_shells(&etc_shells(&text), |path| std::fs::canonicalize(path).ok(), cfg!(target_os = "macos"))
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    use super::super::registry;
    use super::*;

    const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";

    pub fn discover() -> Vec<FoundShell> {
        let mut found = Vec::new();
        let system = super::super::shell::system32();
        let program = |kind, name: &str, path: &Path, args: &[&str]| FoundShell {
            kind,
            name: name.to_owned(),
            shell: LocalShell::Program { path: path.display().to_string(), args: args.iter().map(|&arg| arg.to_owned()).collect() },
        };
        if let Some(pwsh) = super::super::shell::find_pwsh() {
            found.push(program(FoundKind::PowerShell, "PowerShell", &pwsh, &["-NoLogo"]));
        }
        let powershell = system.join(r"WindowsPowerShell\v1.0\powershell.exe");
        if powershell.is_file() {
            found.push(program(FoundKind::WindowsPowerShell, "Windows PowerShell", &powershell, &["-NoLogo"]));
        }
        let cmd = system.join("cmd.exe");
        if cmd.is_file() {
            found.push(program(FoundKind::Cmd, "cmd", &cmd, &[]));
        }
        let git = [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
            .into_iter()
            .find_map(|root| registry::string(root, r"SOFTWARE\GitForWindows", "InstallPath"));
        if let Some(bash) = git.map(|root| PathBuf::from(root).join(r"bin\bash.exe")).filter(|bash| bash.is_file()) {
            found.push(program(FoundKind::GitBash, "Git Bash", &bash, &["--login", "-i"]));
        }
        let distributions = registry::subkeys(HKEY_CURRENT_USER, LXSS)
            .into_iter()
            .map(|key| {
                let path = format!(r"{LXSS}\{key}");
                let name = registry::string(HKEY_CURRENT_USER, &path, "DistributionName");
                let state = registry::dword(HKEY_CURRENT_USER, &path, "State");
                Distribution { key, name, state }
            })
            .collect();
        let default = registry::string(HKEY_CURRENT_USER, LXSS, "DefaultDistribution");
        for distro in wsl_distributions(distributions, default.as_deref()) {
            found.push(FoundShell { kind: FoundKind::Wsl, name: distro.clone(), shell: LocalShell::Wsl { distro } });
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_etc_shells() {
        let text = "# List of acceptable shells\n\n/bin/bash\n/bin/zsh  # the default\n  /usr/bin/fish\nnologin\n";
        assert_eq!(etc_shells(text), ["/bin/bash", "/bin/zsh", "/usr/bin/fish"]);
    }

    #[test]
    fn lists_each_shell_once() {
        let real = |path: &str| match path {
            "/bin/zsh" | "/usr/bin/zsh" => Some(PathBuf::from("/usr/bin/zsh")),
            "/bin/bash" => Some(PathBuf::from("/usr/bin/bash")),
            "/opt/homebrew/bin/bash" => Some(PathBuf::from("/opt/homebrew/Cellar/bash/5.3/bin/bash")),
            _ => None,
        };
        let paths = ["/bin/zsh", "/usr/bin/zsh", "/bin/bash", "/bin/gone", "/opt/homebrew/bin/bash"];
        let found = unique_shells(&paths, real, true);
        let names: Vec<&str> = found.iter().map(|shell| shell.name.as_str()).collect();
        assert_eq!(names, ["zsh", "bash (/bin)", "bash (/opt/homebrew/bin)"]);
        assert_eq!(found[0].shell, LocalShell::Program { path: "/bin/zsh".into(), args: vec!["-l".into()] });
        let not_login = unique_shells(&["/bin/zsh"], real, false);
        assert_eq!(not_login[0].shell, LocalShell::Program { path: "/bin/zsh".into(), args: Vec::new() });
    }

    #[test]
    fn lists_installed_wsl_distributions_default_first() {
        let distro =
            |key: &str, name: Option<&str>, state: Option<u32>| Distribution { key: key.into(), name: name.map(Into::into), state };
        let distributions = vec![
            distro("{a}", Some("Debian"), Some(1)),
            distro("{b}", Some("Ubuntu-24.04"), Some(1)),
            distro("{c}", Some("docker-desktop"), Some(1)),
            distro("{d}", Some("Installing"), Some(3)),
            distro("{e}", None, Some(1)),
            distro("{f}", Some("Old"), None),
        ];
        assert_eq!(wsl_distributions(distributions, Some("{B}")), ["Ubuntu-24.04", "Debian", "Old"]);
        assert!(wsl_distributions(Vec::new(), None).is_empty());
    }

    #[test]
    fn tells_git_environments_apart() {
        assert_eq!(git_msystem(|dir| dir == "mingw64"), "MINGW64");
        assert_eq!(git_msystem(|dir| dir == "clangarm64" || dir == "mingw64"), "CLANGARM64");
        assert_eq!(git_msystem(|dir| dir == "mingw32"), "MINGW32");
        assert_eq!(git_msystem(|_| false), "MINGW64");
    }
}

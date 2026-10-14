//! The folder a terminal's shell is in, for the file panel to open there and follow it: read
//! from Linux's `/proc` over the connection's SFTP session, as any file the user may read,
//! without running anything on the server or changing the shell's environment.
//!
//! Every channel of an SSH connection is a child of the connection's own process on the
//! server (OpenSSH's `sshd-session`, Dropbear's connection process), so that process is above
//! the SFTP server (`/proc/self`), and its children with a terminal are the shells of the
//! connection's panes, oldest first in the order the panes opened them.
//! A pane's shell is the one at its place in that order; when the server has a different
//! number of shells than the connection's open panes (one just closing), none is.
//!
//! Where a program the shell runs is itself a shell the user can read (`bash` started from the
//! shell), that one is followed; otherwise (an editor, `su`, whose shell belongs to root) the
//! pane's own shell is. Without `/proc` (other systems, an SFTP server in a chroot) the folder
//! isn't known here; the frontend may know it from the shell (OSC 7).

use std::collections::HashMap;
use std::sync::Mutex;

use russh_sftp::client::SftpSession;
use tokio::io::AsyncReadExt;

use crate::session::SessionId;

/// What finding the folder needs of the server's files: an SFTP session, or a fake in tests.
pub(crate) trait Procfs {
    /// A link's target.
    async fn read_link(&self, path: &str) -> Option<String>;
    /// A small text file (`/proc` reports a size of 0 for them, so read up to a limit).
    async fn read(&self, path: &str) -> Option<String>;
    /// A folder's entries.
    async fn list(&self, path: &str) -> Option<Vec<String>>;
}

/// More than `/proc/<pid>/stat` or a `children` list holds.
const READ_LIMIT: usize = 16 * 1024;

impl Procfs for SftpSession {
    async fn read_link(&self, path: &str) -> Option<String> {
        SftpSession::read_link(self, path).await.ok()
    }

    async fn read(&self, path: &str) -> Option<String> {
        // Dropping the file closes it without waiting for the answer.
        let file = self.open(path).await.ok()?;
        let mut bytes = Vec::new();
        file.take(READ_LIMIT as u64).read_to_end(&mut bytes).await.ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn list(&self, path: &str) -> Option<Vec<String>> {
        Some(self.read_dir(path).await.ok()?.map(|entry| entry.file_name()).collect())
    }
}

/// The fields of `/proc/<pid>/stat` used here.
#[derive(Debug, PartialEq)]
struct Stat {
    /// The command's name (15 bytes at most).
    command: String,
    ppid: u32,
    /// The controlling terminal's device number; 0 without one.
    tty: u64,
    /// The process group in the foreground of its terminal.
    foreground: i64,
    /// Clock ticks after boot.
    start: u64,
}

/// Parses `/proc/<pid>/stat`: after the command name, which is in parentheses and may contain
/// anything (spaces, parentheses), come the state, the parent and so on (see proc(5)).
fn parse_stat(text: &str) -> Option<Stat> {
    let (open, close) = (text.find('(')?, text.rfind(')')?);
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    Some(Stat {
        command: text.get(open + 1..close)?.to_owned(),
        ppid: fields.get(1)?.parse().ok()?,
        tty: fields.get(4)?.parse().ok()?,
        foreground: fields.get(5)?.parse().ok()?,
        start: fields.get(19)?.parse().ok()?,
    })
}

async fn stat(fs: &impl Procfs, pid: u32) -> Option<Stat> {
    parse_stat(&fs.read(&format!("/proc/{pid}/stat")).await?)
}

/// Programs that are shells, by their command name.
const SHELLS: &[&str] = &["bash", "zsh", "fish", "sh", "dash", "ash", "ksh", "mksh", "yash", "tcsh", "csh", "nu", "elvish", "xonsh"];

/// How far above the SFTP server the connection's process may be.
const MAX_DEPTH: usize = 3;

/// The connection's process: the SSH server above the SFTP server. That is its parent, unless
/// the user's login shell started the SFTP server without making way for it (fish does): the
/// nearest `sshd` (or `sshd-session`, `dropbear`) above it, or its parent when there is none.
async fn connection_process(fs: &impl Procfs) -> Option<u32> {
    let own: u32 = fs.read_link("/proc/self").await?.trim().parse().ok()?;
    let parent = stat(fs, own).await?.ppid;
    let mut pid = parent;
    for _ in 0..MAX_DEPTH {
        let Some(process) = stat(fs, pid).await else { break };
        if process.command.starts_with("sshd") || process.command.starts_with("dropbear") {
            return Some(pid);
        }
        pid = process.ppid;
    }
    Some(parent)
}

/// The shells of the connection's process `parent`: its children with a terminal, oldest first.
async fn shells(fs: &impl Procfs, parent: u32) -> Vec<u32> {
    // Listed by the kernel where it keeps the list (most do); otherwise among all processes,
    // which takes longer.
    let children: Vec<u32> = match fs.read(&format!("/proc/{parent}/task/{parent}/children")).await {
        Some(list) => list.split_whitespace().filter_map(|pid| pid.parse().ok()).collect(),
        None => fs.list("/proc").await.unwrap_or_default().iter().filter_map(|name| name.parse().ok()).collect(),
    };
    let mut found = Vec::new();
    for pid in children {
        if let Some(stat) = stat(fs, pid).await.filter(|stat| stat.ppid == parent && stat.tty != 0) {
            found.push((stat.start, pid));
        }
    }
    found.sort();
    found.into_iter().map(|(_, pid)| pid).collect()
}

/// The folder `path` names, if it is one that still exists (a deleted folder's link ends in
/// " (deleted)").
fn as_folder(path: String) -> Option<String> {
    (path.starts_with('/') && !path.ends_with(" (deleted)")).then_some(path)
}

/// The folder of shell `pid`, a child of `parent` (not a process that has replaced it since),
/// or of a shell it runs in the foreground that the user may read.
async fn folder_of(fs: &impl Procfs, parent: u32, pid: u32) -> Option<String> {
    let shell = stat(fs, pid).await.filter(|shell| shell.ppid == parent && shell.tty != 0)?;
    if let Some(foreground) = u32::try_from(shell.foreground).ok().filter(|foreground| *foreground != pid) {
        if stat(fs, foreground).await.is_some_and(|process| SHELLS.contains(&process.command.as_str())) {
            if let Some(path) = fs.read_link(&format!("/proc/{foreground}/cwd")).await.and_then(as_folder) {
                return Some(path);
            }
        }
    }
    fs.read_link(&format!("/proc/{pid}/cwd")).await.and_then(as_folder)
}

/// The shells of a connection by pane (session), in the order they were opened, and what has
/// been found of them on the server.
#[derive(Default)]
pub struct ShellDirs(Mutex<State>);

#[derive(Default)]
struct State {
    /// The sessions whose shells are open, oldest first.
    open: Vec<SessionId>,
    /// The connection's process, once found.
    parent: Option<u32>,
    /// The shells found, by session.
    pids: HashMap<SessionId, u32>,
}

impl ShellDirs {
    /// Session `id` is opening a shell, after those of the sessions open now.
    pub fn opened(&self, id: SessionId) {
        self.0.lock().unwrap().open.push(id);
    }

    pub fn closed(&self, id: SessionId) {
        let mut state = self.0.lock().unwrap();
        state.open.retain(|open| *open != id);
        state.pids.remove(&id);
    }

    /// The folder the shell of session `id` is in; `None` where it can't be found.
    pub(crate) async fn find(&self, fs: &impl Procfs, id: SessionId) -> Option<String> {
        let (index, count, parent, known) = {
            let state = self.0.lock().unwrap();
            let index = state.open.iter().position(|open| *open == id)?;
            (index, state.open.len(), state.parent, state.pids.get(&id).copied())
        };
        let parent = match parent {
            Some(parent) => parent,
            None => {
                let parent = connection_process(fs).await?;
                self.0.lock().unwrap().parent = Some(parent);
                parent
            }
        };
        if let Some(pid) = known {
            if let Some(path) = folder_of(fs, parent, pid).await {
                return Some(path);
            }
        }
        let shells = shells(fs, parent).await;
        let pid = *shells.get(index).filter(|_| shells.len() == count)?;
        {
            let mut state = self.0.lock().unwrap();
            // Unless the session closed meanwhile.
            if state.open.contains(&id) {
                state.pids.insert(id, pid);
            }
        }
        folder_of(fs, parent, pid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/proc` as a map of paths to contents (links and files alike).
    #[derive(Default)]
    struct Fake(HashMap<String, String>);

    impl Procfs for Fake {
        async fn read_link(&self, path: &str) -> Option<String> {
            self.0.get(path).cloned()
        }
        async fn read(&self, path: &str) -> Option<String> {
            self.0.get(path).cloned()
        }
        async fn list(&self, path: &str) -> Option<Vec<String>> {
            let prefix = format!("{path}/");
            let mut names: Vec<String> =
                self.0.keys().filter_map(|key| key.strip_prefix(&prefix)?.split('/').next().map(str::to_owned)).collect();
            names.sort();
            names.dedup();
            Some(names)
        }
    }

    impl Fake {
        /// Process `pid` with its terminal (device, foreground process group), 0 for none.
        fn process(&mut self, pid: u32, command: &str, ppid: u32, (tty, foreground): (u64, i64), start: u64, cwd: &str) {
            let rest: Vec<String> = (0..12).map(|_| "0".to_owned()).collect();
            let stat = format!("{pid} ({command}) S {ppid} {pid} {pid} {tty} {foreground} 0 {} {start} 0", rest.join(" "));
            self.0.insert(format!("/proc/{pid}/stat"), stat);
            self.0.insert(format!("/proc/{pid}/cwd"), cwd.into());
        }
    }

    /// A connection (100) with an SFTP server (110), shells for sessions 1 and 2 (started in
    /// that order, though not in pid order), and another connection's shell (300).
    fn server() -> Fake {
        let mut fs = Fake::default();
        fs.0.insert("/proc/self".into(), "110".into());
        fs.process(100, "sshd-session", 1, (0, -1), 10, "/");
        fs.process(110, "sftp-server", 100, (0, -1), 30, "/home/alice");
        fs.process(205, "bash", 100, (34816, 205), 20, "/srv/www");
        fs.process(201, "zsh", 100, (34817, 201), 25, "/home/alice/src");
        fs.process(300, "bash", 299, (34818, 300), 5, "/tmp");
        fs
    }

    fn panes(ids: &[SessionId]) -> ShellDirs {
        let dirs = ShellDirs::default();
        for id in ids {
            dirs.opened(*id);
        }
        dirs
    }

    #[test]
    fn reads_the_fields_after_a_strange_command_name() {
        let stat = "42 (a) b (c) S 7 42 42 34816 42 4194560 0 0 0 0 0 0 0 0 20 0 1 0 12345 0";
        assert_eq!(parse_stat(stat), Some(Stat { command: "a) b (c".into(), ppid: 7, tty: 34816, foreground: 42, start: 12345 }));
        assert_eq!(parse_stat("42 (a) S 7"), None);
    }

    #[tokio::test]
    async fn finds_each_pane_s_shell_in_the_order_they_opened() {
        let fs = server();
        let dirs = panes(&[1, 2]);
        assert_eq!(dirs.find(&fs, 1).await.as_deref(), Some("/srv/www"));
        assert_eq!(dirs.find(&fs, 2).await.as_deref(), Some("/home/alice/src"));
        assert_eq!(dirs.find(&fs, 3).await, None);
    }

    #[tokio::test]
    async fn looks_among_all_processes_without_the_children_list() {
        let mut fs = server();
        assert_eq!(panes(&[1, 2]).find(&fs, 2).await.as_deref(), Some("/home/alice/src"));
        fs.0.insert("/proc/100/task/100/children".into(), "110 201 205".into());
        fs.0.remove("/proc/300/stat");
        assert_eq!(panes(&[1, 2]).find(&fs, 2).await.as_deref(), Some("/home/alice/src"));
    }

    #[tokio::test]
    async fn finds_the_connection_above_a_login_shell_that_stayed() {
        let mut fs = server();
        fs.process(110, "sftp-server", 105, (0, -1), 30, "/home/alice");
        fs.process(105, "fish", 100, (0, -1), 29, "/home/alice");
        assert_eq!(panes(&[1, 2]).find(&fs, 2).await.as_deref(), Some("/home/alice/src"));
        // Without an SSH server above it, its parent.
        fs.process(100, "tinysshd", 1, (0, -1), 10, "/");
        assert_eq!(panes(&[1, 2]).find(&fs, 2).await, None);
    }

    #[tokio::test]
    async fn finds_none_while_the_shells_dont_match_the_panes() {
        let fs = server();
        // A third pane's shell isn't there yet (or one of them has gone).
        assert_eq!(panes(&[1, 2, 3]).find(&fs, 1).await, None);
        assert_eq!(panes(&[1]).find(&fs, 1).await, None);
    }

    #[tokio::test]
    async fn follows_a_shell_run_in_the_foreground_but_not_other_programs() {
        let mut fs = server();
        fs.process(205, "bash", 100, (34816, 400), 20, "/srv/www");
        fs.process(400, "bash", 205, (34816, 400), 40, "/var/log");
        let dirs = panes(&[1, 2]);
        assert_eq!(dirs.find(&fs, 1).await.as_deref(), Some("/var/log"));
        fs.process(400, "vim", 205, (34816, 400), 40, "/var/log");
        assert_eq!(dirs.find(&fs, 1).await.as_deref(), Some("/srv/www"));
        // A root shell (su) can't be read.
        fs.process(400, "bash", 205, (34816, 400), 40, "/root");
        fs.0.remove("/proc/400/cwd");
        assert_eq!(dirs.find(&fs, 1).await.as_deref(), Some("/srv/www"));
    }

    #[tokio::test]
    async fn finds_a_new_pane_s_shell_after_one_closed() {
        let mut fs = server();
        let dirs = panes(&[1, 2]);
        assert_eq!(dirs.find(&fs, 1).await.as_deref(), Some("/srv/www"));
        // Session 1 closed and another opened; pid 205 has gone to another connection.
        dirs.closed(1);
        dirs.opened(3);
        fs.process(205, "bash", 299, (34820, 205), 50, "/elsewhere");
        fs.process(207, "bash", 100, (34816, 207), 60, "/opt");
        assert_eq!(dirs.find(&fs, 3).await.as_deref(), Some("/opt"));
    }

    #[tokio::test]
    async fn knows_no_folder_without_proc_or_for_a_deleted_one() {
        assert_eq!(panes(&[1]).find(&Fake::default(), 1).await, None);
        let mut fs = server();
        fs.process(205, "bash", 100, (34816, 205), 20, "/srv/old (deleted)");
        assert_eq!(panes(&[1, 2]).find(&fs, 1).await, None);
    }
}

//! What a new session runs: a [`SessionSpec`] from the frontend (a saved session, a quick
//! connection, another shell on a tab's connection, a local shell) resolved into its
//! [`Backend`], with how its log is named and the remote side's character encoding. The
//! session itself is then started by `SessionManager::spawn`, which knows nothing of the
//! backends.
//!
//! Resolving blocks for a local terminal, which looks for its program and folder (in `PATH`
//! too, where a network drive may not answer): `session_open` does it off the async workers.

use std::future::Future;
use std::sync::Arc;

use serde::Deserialize;
use ts_rs::TS;

use crate::config::ProfileStore;
use crate::connections::{Connections, LiveConnection};
use crate::encoding;
use crate::error::{Error, Result};
use crate::logging::LogInfo;
use crate::model::{self, Connection, LocalOptions, LocalShell, Profile, Protocol, Remote, SerialOptions, SshOptions, SshProfile};
use crate::net::Route;
use crate::pty;
use crate::serial;
use crate::session::{SessionId, SessionManager, TermIo};
use crate::settings::SettingsStore;
use crate::ssh;
use crate::telnet;

/// What a new session runs.
#[derive(Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionSpec {
    /// A saved session, with the backend for its protocol. `carry`: forwarding rules to start
    /// besides the automatic ones (those running before the tab reconnected).
    Profile { profile_id: String, carry: Vec<String> },
    /// An SSH or Telnet connection typed into the search box, without a saved session. SSH
    /// without a user name uses the local one, like `ssh host`.
    Quick { protocol: Protocol, username: String, host: String, port: u16 },
    /// Another shell on the SSH connection of session `source` (duplicating its tab), without
    /// connecting or authenticating again. Fails with `session.notConnected` if that session
    /// has no connection (any more), and `session.notFound` if it is another window's.
    Shared { source: SessionId },
    /// A shell in a local pseudo terminal, in the home folder, without a saved session: the
    /// default one, or one of those found here (`pty::discover`).
    Local { shell: LocalShell },
}

/// What resolving a spec reads: the saved sessions, the live connections, the sessions (whose
/// log a shared one is named after) and the settings.
pub struct Sources<'a> {
    pub profiles: &'a ProfileStore,
    pub connections: &'a Connections,
    pub sessions: &'a SessionManager,
    pub settings: &'a SettingsStore,
}

/// A session about to start: how its log is named and whether it starts logged, the remote
/// side's character encoding, and its backend.
pub struct Launch {
    pub log_info: LogInfo,
    pub auto_log: bool,
    pub encoding: &'static encoding_rs::Encoding,
    pub backend: Backend,
}

/// What `spec` runs, for a tab of `webview`.
pub fn launch(spec: SessionSpec, webview: &str, sources: Sources) -> Result<Launch> {
    let connections = sources.connections.clone();
    let utf8 = encoding_rs::UTF_8;
    match spec {
        SessionSpec::Profile { profile_id, carry } => {
            let store = sources.profiles;
            let profile = store.get(&profile_id)?;
            let log_info = LogInfo { profile: Some(profile.id.clone()), ..log_info(&profile) };
            let (auto_log, encoding) = (profile.auto_log, encoding::for_profile(&profile.encoding));
            let Profile { id, name, encoding: label, connection, .. } = profile;
            let backend = match connection {
                Connection::Ssh(ssh) => {
                    let route = store.route(&ssh.remote)?;
                    Backend::Ssh { profile: SshProfile { id, name, encoding: label, ssh }, route, connections, carry }
                }
                Connection::Telnet(remote) => Backend::Telnet { route: store.route(&remote)?, remote, password_of: Some(id) },
                Connection::Serial(serial) => Backend::Serial(serial),
                // Imported, it runs what the file says: not before the user has seen it.
                Connection::Local(local) if local.imported => return Err(Error::new("local.notConfirmed")),
                Connection::Local(local) => Backend::Local(local_shell(&local)?),
            };
            let encoding = if matches!(backend, Backend::Local(_)) { utf8 } else { encoding };
            Ok(Launch { log_info, auto_log, encoding, backend })
        }
        SessionSpec::Quick { protocol, username, host, port } => {
            let username = match username.trim() {
                "" if protocol == Protocol::Ssh => model::local_username(),
                name => name.to_owned(),
            };
            let host = host.trim().to_owned();
            let name = if username.is_empty() { host.clone() } else { format!("{username}@{host}") };
            match protocol {
                Protocol::Ssh if username.is_empty() => return Err(Error::new("profile.missingFields")),
                Protocol::Serial => return Err(Error::new("profile.missingDevice")),
                // Nothing to connect to: local terminals have a spec of their own.
                Protocol::Local => return Err(Error::new("profile.missingHost")),
                _ if host.is_empty() || port == 0 => return Err(Error::new("profile.missingHost")),
                _ if !model::is_plain_host(&host) => return Err(Error::new("profile.invalidHost")),
                _ => {}
            }
            let log_info = LogInfo { session: name.clone(), host: host.clone(), user: username.clone(), profile: None };
            let remote = Remote::new(host, port, username);
            let backend = match protocol {
                Protocol::Telnet => Backend::Telnet { remote, route: Route::default(), password_of: None },
                _ => Backend::Ssh { profile: SshProfile::quick(name, remote), route: Route::default(), connections, carry: Vec::new() },
            };
            // Logged only when started by hand: there is no session to record automatically.
            Ok(Launch { log_info, auto_log: false, encoding: utf8, backend })
        }
        SessionSpec::Shared { source } => {
            // Named like the source's log; a log of its own, as a new tab of the session would get.
            let log_info = sources.sessions.sink(webview, source)?.log().info.clone();
            let connection = connections.get(source)?;
            // Ended, but its session hasn't noticed yet (it is about to report it lost).
            if connection.ssh().handle().is_closed() {
                return Err(Error::new("session.notConnected"));
            }
            let auto_log = log_info.profile.as_ref().and_then(|id| sources.profiles.get(id).ok()).is_some_and(|p| p.auto_log);
            let encoding = encoding::for_profile(&connection.ssh().profile().encoding);
            Ok(Launch { log_info, auto_log, encoding, backend: Backend::Shared { connection, connections } })
        }
        SessionSpec::Local { shell } => {
            let shell = local_shell(&LocalOptions::new(shell))?;
            let log_info = LogInfo { session: shell.name(), host: "localhost".to_owned(), user: model::local_username(), profile: None };
            let auto_log = sources.settings.get().logs.auto_local;
            Ok(Launch { log_info, auto_log, encoding: utf8, backend: Backend::Local(shell) })
        }
    }
}

/// What a local terminal runs, where the system allows local terminals (not Windows in S mode).
fn local_shell(local: &LocalOptions) -> Result<pty::Shell> {
    if !pty::local_shells_allowed() {
        return Err(Error::new("local.blocked"));
    }
    pty::local_shell(local)
}

/// How a session's log names it: by the profile, its address and user.
fn log_info(profile: &Profile) -> LogInfo {
    let (host, user) = match &profile.connection {
        Connection::Serial(serial) => (serial.device.clone(), String::new()),
        Connection::Local(_) => ("localhost".to_owned(), model::local_username()),
        Connection::Ssh(SshOptions { remote, .. }) | Connection::Telnet(remote) => (remote.host.clone(), remote.username.clone()),
    };
    LogInfo { session: profile.name.clone(), host, user, profile: None }
}

/// How `route` goes, for the debug log: the jump hosts and the proxy, if any.
fn describe_route(route: &Route) -> String {
    let jumps: Vec<String> = route.jumps.iter().map(|jump| format!("\"{}\"", jump.name)).collect();
    let mut text = String::new();
    if !jumps.is_empty() {
        text.push_str(&format!(", through jump hosts {}", jumps.join(", ")));
    }
    if let Some(proxy) = &route.proxy {
        text.push_str(&format!(", through proxy \"{}\"", proxy.name));
    }
    text
}

/// A session's backend, with what it runs on.
pub enum Backend {
    Ssh {
        profile: SshProfile,
        route: Route,
        connections: Connections,
        carry: Vec<String>,
    },
    Shared {
        connection: Arc<LiveConnection>,
        connections: Connections,
    },
    /// `password_of`: the saved session whose password answers the login prompt.
    Telnet {
        remote: Remote,
        route: Route,
        password_of: Option<String>,
    },
    Serial(SerialOptions),
    Local(pty::Shell),
}

impl Backend {
    /// The task of session `id`. A shared connection learns of the session before the task
    /// runs, so that closing the session right away releases it.
    pub fn start(self, id: SessionId, io: TermIo) -> impl Future<Output = ()> + Send + 'static {
        if let Backend::Shared { connection, connections } = &self {
            connections.attach(id, connection.clone(), &io);
        }
        self.run(id, io)
    }

    async fn run(self, id: SessionId, io: TermIo) {
        match &self {
            Backend::Ssh { profile, route, .. } => {
                let remote = &profile.ssh.remote;
                let target = model::host_port(&remote.host, remote.port);
                log::info!("session {id}: SSH \"{}\", {}@{target}{}", profile.name, remote.username, describe_route(route));
            }
            Backend::Shared { connection, .. } => {
                log::info!(conn = connection.ssh().info().id; "session {id}: another shell on the connection of \"{}\"", connection.ssh().profile().name);
            }
            Backend::Telnet { remote, route, .. } => {
                log::info!("session {id}: Telnet {}{}", model::host_port(&remote.host, remote.port), describe_route(route));
            }
            Backend::Serial(options) => log::info!("session {id}: serial port {}", options.device),
            Backend::Local(shell) => log::info!("session {id}: local shell {}", shell.name()),
        }
        match self {
            Backend::Ssh { profile, route, connections, carry } => {
                let exit = connections.shell_exit(id);
                ssh::run(profile, route, id, io, move |connection, io| connections.connected(id, connection, &carry, io), exit).await
            }
            Backend::Shared { connection, connections } => {
                ssh::run_shared(connection.ssh().clone(), id, io, connections.shell_exit(id)).await
            }
            Backend::Telnet { remote, route, password_of } => {
                // Looked up when the server asks.
                let passwords = route.passwords.clone();
                let password = move || password_of.and_then(|id| passwords.profile(&id));
                telnet::run(remote, route, password, io).await
            }
            Backend::Serial(options) => serial::run(options, io).await,
            Backend::Local(shell) => pty::run(shell, io).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::Remote;
    use crate::persist::SetAside;
    use crate::vault::Vault;

    struct Setup {
        dir: PathBuf,
        profiles: ProfileStore,
        connections: Connections,
        sessions: SessionManager,
        settings: SettingsStore,
    }

    impl Setup {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("zshell-launch-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let store = Arc::new(crate::secrets::MemoryStore::default());
            store.failing.store(true, std::sync::atomic::Ordering::Relaxed);
            let vault = Vault::open(dir.clone(), store);
            vault.try_unlock();
            let settings = SettingsStore::load(dir.join("settings.json"), &SetAside::default());
            Self {
                profiles: ProfileStore::load(Arc::new(vault)),
                connections: Connections::default(),
                sessions: SessionManager::default(),
                settings,
                dir,
            }
        }

        fn launch(&self, spec: SessionSpec) -> Result<Launch> {
            let sources =
                Sources { profiles: &self.profiles, connections: &self.connections, sessions: &self.sessions, settings: &self.settings };
            launch(spec, "main", sources)
        }
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn quick(protocol: Protocol, username: &str, host: &str, port: u16) -> SessionSpec {
        SessionSpec::Quick { protocol, username: username.into(), host: host.into(), port }
    }

    fn code(result: Result<Launch>) -> String {
        result.err().map(|e| e.code().to_owned()).unwrap_or_default()
    }

    #[test]
    fn quick_connections_are_checked_and_named_after_their_target() {
        let setup = Setup::new("quick");
        assert_eq!(code(setup.launch(quick(Protocol::Telnet, "", " ", 23))), "profile.missingHost");
        assert_eq!(code(setup.launch(quick(Protocol::Telnet, "", "a", 0))), "profile.missingHost");
        assert_eq!(code(setup.launch(quick(Protocol::Telnet, "", "a b", 23))), "profile.invalidHost");
        assert_eq!(code(setup.launch(quick(Protocol::Serial, "", "a", 1))), "profile.missingDevice");

        let launch = setup.launch(quick(Protocol::Ssh, " bob ", " host ", 22)).unwrap();
        assert_eq!((launch.log_info.session.as_str(), launch.log_info.host.as_str()), ("bob@host", "host"));
        assert!(!launch.auto_log && launch.log_info.profile.is_none());
        assert!(matches!(launch.backend, Backend::Ssh { ref carry, .. } if carry.is_empty()));
        let launch = setup.launch(quick(Protocol::Telnet, "", "host", 23)).unwrap();
        assert_eq!(launch.log_info.session, "host");
        assert!(matches!(launch.backend, Backend::Telnet { password_of: None, .. }));
    }

    #[test]
    fn a_saved_session_runs_with_its_protocol_log_and_encoding() {
        let setup = Setup::new("profile");
        assert_eq!(code(setup.launch(SessionSpec::Profile { profile_id: "gone".into(), carry: Vec::new() })), "profile.notFound");

        let telnet = Profile {
            auto_log: true,
            encoding: "gbk".into(),
            ..Profile::new("box".into(), Connection::Telnet(Remote::new("h".into(), 23, "u".into())))
        };
        let telnet = setup.profiles.save(telnet, None).unwrap();
        let launch = setup.launch(SessionSpec::Profile { profile_id: telnet.id.clone(), carry: Vec::new() }).unwrap();
        assert_eq!(launch.log_info.profile.as_deref(), Some(telnet.id.as_str()));
        assert_eq!((launch.log_info.host.as_str(), launch.log_info.user.as_str()), ("h", "u"));
        assert!(launch.auto_log);
        assert_eq!(launch.encoding, encoding_rs::GBK);
        assert!(matches!(launch.backend, Backend::Telnet { password_of: Some(ref id), .. } if *id == telnet.id));
    }

    #[test]
    fn a_local_session_runs_once_confirmed_and_only_where_its_program_is() {
        let setup = Setup::new("local");
        let missing = std::env::temp_dir().join("zshell-launch-missing").join("shell").display().to_string();
        let program = |path: &str| LocalShell::Program { path: path.to_owned(), args: Vec::new() };
        let saved = |local: LocalOptions| {
            let profile = Profile { auto_log: true, ..Profile::new("here".into(), Connection::Local(local)) };
            let profile = setup.profiles.save(profile, None).unwrap();
            SessionSpec::Profile { profile_id: profile.id, carry: Vec::new() }
        };
        assert_eq!(code(setup.launch(saved(LocalOptions::new(program(&missing))))), "local.programNotFound");
        // Imported: not looked for before it is confirmed.
        let mut imported = Profile::new("there".into(), Connection::Local(LocalOptions::new(program(&missing))));
        imported.normalize_imported().unwrap();
        imported.id = "imported".into();
        let order = vec![model::Item::Profile { id: imported.id.clone() }];
        setup.profiles.add_all(|_| Ok(crate::config::Additions { profiles: vec![imported], order, ..Default::default() })).unwrap();
        let imported = || SessionSpec::Profile { profile_id: "imported".into(), carry: Vec::new() };
        assert_eq!(code(setup.launch(imported())), "local.notConfirmed");
        setup.profiles.confirm_local("imported").unwrap();
        assert_eq!(code(setup.launch(imported())), "local.programNotFound");
        let gone = LocalOptions { start_dir: missing.clone(), ..LocalOptions::new(LocalShell::Default) };
        assert_eq!(code(setup.launch(saved(gone))), "local.startDirNotFound");
        #[cfg(not(windows))]
        assert_eq!(code(setup.launch(saved(LocalOptions::new(LocalShell::Wsl { distro: "Ubuntu".into() })))), "local.wslUnavailable");

        let launch = setup.launch(saved(LocalOptions::new(LocalShell::Default))).unwrap();
        assert_eq!((launch.log_info.session.as_str(), launch.log_info.host.as_str()), ("here", "localhost"));
        assert!(launch.auto_log && launch.log_info.profile.is_some());
        assert_eq!(launch.encoding, encoding_rs::UTF_8);
        assert!(matches!(launch.backend, Backend::Local(_)));

        let launch = setup.launch(SessionSpec::Local { shell: LocalShell::Default }).unwrap();
        assert!(!launch.auto_log && launch.log_info.profile.is_none());
        assert_eq!(code(setup.launch(SessionSpec::Local { shell: program(&missing) })), "local.programNotFound");
        assert_eq!(code(setup.launch(quick(Protocol::Local, "", "host", 22))), "profile.missingHost");
    }

    #[test]
    fn a_shared_shell_needs_a_session_of_the_window() {
        let setup = Setup::new("shared");
        assert_eq!(code(setup.launch(SessionSpec::Shared { source: 1 })), "session.notFound");
    }
}

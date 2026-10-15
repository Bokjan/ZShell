use std::time::Duration;

use russh::keys::PublicKeyOrCertificate;
use russh::server::{self, Auth as ServerAuth, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::model::Remote;
use crate::session::SessionInput;
use crate::ssh::abortable::{abortable, AbortOnDrop};
// Serves as the host key and the user's key.
use crate::ssh::TEST_KEY as KEY;

/// A server that requires `steps` in turn, as sshd's `AuthenticationMethods publickey,password`
/// does. Keyboard-interactive asks `prompt` (hidden); it and password take `secret`. With
/// `max_attempts`, it disconnects after that many refusals, as sshd does after
/// `MaxAuthTries`; with `hangs_up`, the connection drops when a password arrives.
#[derive(Default)]
struct Server {
    steps: Vec<MethodKind>,
    passed: usize,
    prompt: &'static str,
    secret: &'static str,
    max_attempts: Option<usize>,
    hangs_up: bool,
    /// Ends the server's side of the connection, when `hangs_up`.
    hang_up: Option<AbortOnDrop>,
}

impl Server {
    fn pass(&mut self, method: MethodKind) -> ServerAuth {
        if self.steps.get(self.passed) != Some(&method) {
            return ServerAuth::reject();
        }
        self.passed += 1;
        match self.steps.get(self.passed) {
            None => ServerAuth::Accept,
            Some(next) => ServerAuth::Reject { proceed_with_methods: Some(MethodSet::from(&[*next][..])), partial_success: true },
        }
    }
}

impl server::Handler for Server {
    type Error = russh::Error;

    // Keys are refused before they are signed with, as sshd does with keys it doesn't know.
    async fn auth_publickey_offered(&mut self, _user: &str, _key: &PublicKey) -> Result<ServerAuth, Self::Error> {
        Ok(if self.steps.get(self.passed) == Some(&MethodKind::PublicKey) { ServerAuth::Accept } else { ServerAuth::reject() })
    }

    async fn auth_publickey(&mut self, _user: &str, _key: &PublicKey) -> Result<ServerAuth, Self::Error> {
        Ok(self.pass(MethodKind::PublicKey))
    }

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<ServerAuth, Self::Error> {
        drop(self.hang_up.take());
        Ok(if password == self.secret { self.pass(MethodKind::Password) } else { ServerAuth::reject() })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<Response<'a>>,
    ) -> Result<ServerAuth, Self::Error> {
        let Some(mut answers) = response else {
            return Ok(ServerAuth::Partial { name: "".into(), instructions: "".into(), prompts: vec![(self.prompt.into(), false)].into() });
        };
        Ok(match answers.next().as_deref() == Some(self.secret.as_bytes()) {
            true => self.pass(MethodKind::KeyboardInteractive),
            false => ServerAuth::reject(),
        })
    }
}

/// Trusts any host key and reports why the connection ended, as `ClientHandler` does.
struct Client(watch::Sender<Option<EndReason>>);

impl russh::client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn disconnected(&mut self, reason: russh::client::DisconnectReason<Self::Error>) -> Result<(), Self::Error> {
        self.0.send_replace(Some(EndReason::of(&reason)));
        match reason {
            russh::client::DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            russh::client::DisconnectReason::Error(e) => Err(e),
        }
    }
}

/// Authenticates with `auth` (the key file is `KEY`) and `saved` as the saved password,
/// against a server that first offers `offered`; `typed` answers prompts. Returns the
/// result and what the terminal showed.
async fn sign_in(server: Server, offered: &[MethodKind], auth: AuthMethod, saved: Option<&str>, typed: &str) -> (Result<()>, String) {
    sign_in_with_key(server, offered, auth, KEY, saved, typed).await
}

/// [`sign_in`] with `key_file` as the session's key file.
async fn sign_in_with_key(
    server: Server,
    offered: &[MethodKind],
    auth: AuthMethod,
    key_file: &str,
    saved: Option<&str>,
    typed: &str,
) -> (Result<()>, String) {
    let mut server = server;
    let key = PrivateKey::from_openssh(KEY).unwrap();
    let defaults = server::Config::default();
    let config = server::Config {
        methods: MethodSet::from(offered),
        keys: vec![key],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        max_auth_attempts: server.max_attempts.unwrap_or(defaults.max_auth_attempts),
        ..defaults
    };
    let (client_side, server_side) = tokio::io::duplex(1 << 16);
    let (server_side, hang_up) = abortable(server_side);
    match server.hangs_up {
        true => server.hang_up = Some(hang_up),
        false => hang_up.disarm(),
    }
    tokio::spawn(async move {
        if let Ok(session) = server::run_stream(Arc::new(config), server_side, server).await {
            let _ = session.await;
        }
    });
    let (ended_tx, ended) = watch::channel(None);
    let mut session =
        russh::client::connect_stream(Arc::new(russh::client::Config::default()), client_side, Client(ended_tx)).await.unwrap();

    let key_path = std::env::temp_dir().join(format!(
        "zshell-auth-key-{}-{}",
        std::process::id(),
        NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&key_path, key_file).unwrap();
    let mut profile = SshProfile::quick("t".into(), Remote::new("example.com".into(), 22, "alice".into()));
    profile.ssh.auth = match auth {
        AuthMethod::PublicKey { .. } => AuthMethod::PublicKey { key_path: key_path.display().to_string() },
        auth => auth,
    };
    let (mut io, input, output, _events) = TermIo::detached((80, 24));
    // A line at a time, as typed: a prompt takes one line of what arrives at once.
    for line in typed.split_inclusive('\r') {
        input.send(SessionInput::Data(line.as_bytes().to_vec())).unwrap();
    }
    let auth = Auth {
        session: &mut session,
        profile: &profile,
        passwords: &SavedPasswords::default(),
        io: &mut io,
        ended,
        saved_password: Some(saved.map(Into::into)),
        tried: Vec::new(),
    };
    // A prompt nothing answers would wait for ever.
    let result = tokio::time::timeout(Duration::from_secs(10), auth.run()).await.expect("authentication waits for input");
    let shown = String::from_utf8_lossy(&output.try_iter().flatten().collect::<Vec<_>>()).into_owned();
    (result, shown)
}

fn key_file() -> AuthMethod {
    AuthMethod::PublicKey { key_path: String::new() }
}

/// Key files of tests running at once.
static NEXT_FILE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A key encrypted with the passphrase `testpass`.
const ENCRYPTED_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABDaRmwemD
htxWb1bChOpjBSAAAAGAAAAAEAAAAzAAAAC3NzaC1lZDI1NTE5AAAAINENTQr9eKhIyDwv
TURh5GMwE/ENMydw1PiA8I0JpMNMAAAAkNYp9HMv7LmX1t+mWzbwJ6pnUQSU4/mJupm6cA
3xLTtzBWDgwkRpsH0uZpT2KqEZbRUkINuNk6DZBG+HwPkAQeUzerukaKXOJAEVzuJ9KXzf
bCB8YKvbCxAid7SV3h+ksepr+amFJb0s7swnytfOowDnE3BwN05niZadRpto4kvLnBu35I
wF5Blj6bR6iFUzJQ==
-----END OPENSSH PRIVATE KEY-----
";

/// The session's own encrypted key: its passphrase is only asked for once the server
/// accepts it, and wrong ones are an error rather than a rejected key.
#[tokio::test]
async fn chosen_encrypted_key_asks_once_accepted() {
    let refusing = || Server { steps: vec![MethodKind::Password], passed: 0, prompt: "", secret: "", ..Default::default() };
    let offered = [MethodKind::PublicKey, MethodKind::Password];
    let (result, shown) = sign_in_with_key(refusing(), &offered, key_file(), ENCRYPTED_KEY, None, "").await;
    assert_eq!(Error::from(result.unwrap_err()).code(), "auth.keyRejected");
    assert!(!shown.contains("passphrase"), "{shown:?}");

    let accepting = || Server { steps: vec![MethodKind::PublicKey], passed: 0, prompt: "", secret: "", ..Default::default() };
    let (result, shown) = sign_in_with_key(accepting(), &offered, key_file(), ENCRYPTED_KEY, None, "testpass\r").await;
    result.unwrap();
    assert!(shown.contains("passphrase"), "{shown:?}");

    let (result, _) = sign_in_with_key(accepting(), &offered, key_file(), ENCRYPTED_KEY, None, "a\rb\rc\r").await;
    assert_eq!(Error::from(result.unwrap_err()).code(), "auth.keyDecryptFailed");

    let (result, _) = sign_in_with_key(accepting(), &offered, key_file(), "not a key", None, "").await;
    assert_eq!(Error::from(result.unwrap_err()).code(), "auth.keyReadFailed");
}

/// `AuthenticationMethods publickey,keyboard-interactive` with a one-time code (Duo,
/// Google Authenticator): the saved password must not be spent on the code prompt.
#[tokio::test]
async fn key_then_verification_code() {
    let server = Server {
        steps: vec![MethodKind::PublicKey, MethodKind::KeyboardInteractive],
        passed: 0,
        prompt: "Verification code: ",
        secret: "123456",
        ..Default::default()
    };
    let (result, shown) = sign_in(server, &[MethodKind::PublicKey], key_file(), Some("hunter2"), "123456\r").await;
    result.unwrap();
    assert!(shown.contains("Verification code: ") && !shown.contains("Permission denied"), "{shown:?}");
}

/// `AuthenticationMethods publickey,password`: the saved password answers the second step.
#[tokio::test]
async fn key_then_saved_password() {
    let server =
        Server { steps: vec![MethodKind::PublicKey, MethodKind::Password], passed: 0, prompt: "", secret: "hunter2", ..Default::default() };
    let (result, shown) = sign_in(server, &[MethodKind::PublicKey], key_file(), Some("hunter2"), "").await;
    result.unwrap();
    assert!(!shown.contains("password"), "{shown:?}");
}

/// Keyboard-interactive comes before password, as in OpenSSH, and the saved password
/// answers PAM's password prompt.
#[tokio::test]
async fn keyboard_interactive_before_password() {
    let server =
        Server { steps: vec![MethodKind::KeyboardInteractive], passed: 0, prompt: "Password: ", secret: "hunter2", ..Default::default() };
    let offered = [MethodKind::Password, MethodKind::KeyboardInteractive];
    let (result, shown) = sign_in(server, &offered, AuthMethod::Password, Some("hunter2"), "").await;
    result.unwrap();
    assert!(!shown.contains("Password: "), "{shown:?}");
}

/// A second step we cannot do names what the server requires.
#[tokio::test]
async fn reports_methods_it_cannot_use() {
    let server =
        Server { steps: vec![MethodKind::PublicKey, MethodKind::HostBased], passed: 0, prompt: "", secret: "", ..Default::default() };
    let (result, _) = sign_in(server, &[MethodKind::PublicKey], key_file(), None, "").await;
    let error = Error::from(result.unwrap_err());
    assert_eq!(error.code(), "auth.moreRequired");
    assert!(error.to_string().contains("hostbased"), "{error}");

    let server = Server { steps: vec![MethodKind::PublicKey], passed: 0, prompt: "", secret: "", ..Default::default() };
    let (result, _) = sign_in(server, &[MethodKind::PublicKey], AuthMethod::Password, Some("hunter2"), "").await;
    assert_eq!(Error::from(result.unwrap_err()).code(), "auth.notOffered");
}

/// The connection dropping while the server considers a password is a lost connection,
/// which reconnecting may get past, not a wrong password; and the terminal doesn't ask
/// again on a connection that is gone.
#[tokio::test]
async fn connection_lost_during_authentication() {
    let server = Server { steps: vec![MethodKind::Password], secret: "hunter2", hangs_up: true, ..Default::default() };
    let (result, shown) = sign_in(server, &[MethodKind::Password], AuthMethod::Password, Some("hunter2"), "").await;
    assert_eq!(Error::from(result.unwrap_err()).code(), "net.connectionLost");
    assert!(!shown.contains("password"), "{shown:?}");
}

/// The server ending the connection (sshd after `MaxAuthTries`) is an authentication
/// failure, with the server's reason, as OpenSSH reports it.
#[tokio::test]
async fn server_disconnects_during_authentication() {
    // The probe with no method counts as the first refusal.
    let server = Server { steps: vec![MethodKind::Password], secret: "hunter2", max_attempts: Some(1), ..Default::default() };
    let (result, _) = sign_in(server, &[MethodKind::Password], AuthMethod::Password, Some("wrong"), "").await;
    let error = Error::from(result.unwrap_err());
    assert_eq!(error.code(), "auth.disconnected");
    assert!(error.to_string().contains("Too many authentication attempts"), "{error}");
}

#[test]
fn tells_password_prompts_from_codes() {
    let asks = |prompt| asks_for_password(prompt, false, "alice");
    assert!(asks("Password: "));
    assert!(asks("alice@example.com's password:"));
    assert!(asks("ALICE@other-name's password:"));
    assert!(!asks("Verification code: "));
    assert!(!asks("Passcode or option (1-3): "));
    assert!(!asks_for_password("Password: ", true, "alice"));
    assert!(asks("Password for alice: "));
    assert!(asks("Password for alice@EXAMPLE.COM: "));
    // pam_oath, and servers that want the password and a code together.
    assert!(!asks("One-time password (OATH) for `alice': "));
    assert!(!asks("Password + OTP: "));
    assert!(!asks("Password for alice (and code): "));
    assert!(!asks("New password: "));
    // Another account's password, or something that only looks like one.
    assert!(!asks("Password for root: "));
    assert!(!asks("root's password: "));
    assert!(!asks("Password for OTP: "));
    assert!(!asks("Password for alicex: "));
    assert!(!asks("Password for root@alice: "));
    assert!(!asks_for_password("Password for : ", false, ""));
}

/// An agent that refuses to sign still yields a (useless) signature, since russh waits
/// for one.
#[tokio::test]
async fn agent_refusal_gives_invalid_signature() {
    let (client, mut agent_side) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        // Answers every request with SSH_AGENT_FAILURE.
        let mut len = [0u8; 4];
        while agent_side.read_exact(&mut len).await.is_ok() {
            let mut request = vec![0; u32::from_be_bytes(len) as usize];
            agent_side.read_exact(&mut request).await.unwrap();
            agent_side.write_all(&[0, 0, 0, 1, 5]).await.unwrap();
        }
    });
    let mut agent = AgentClient::connect(client);
    let key =
        russh::keys::PublicKey::from_openssh("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIErPvl8mnDbXiALZf/lPNK6oppUWYB4bP5AYBJmvaFkh").unwrap();
    let identity = AgentIdentity::PublicKey { key, comment: String::new() };
    let signed = AgentSigner(&mut agent).auth_sign(&identity, None, b"data".to_vec()).await.unwrap();
    let mut expected = b"data".to_vec();
    invalid_signature(Algorithm::Ed25519).encode(&mut expected).unwrap();
    assert_eq!(signed, expected);
}

/// Only a regular file of key size is read: a folder, a device such as `/dev/zero` (which would
/// be read without end) or a huge file is not.
#[tokio::test]
async fn only_key_sized_files_are_read_as_keys() {
    let dir = std::env::temp_dir().join(format!("zshell-key-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let key = dir.join("id_ed25519");
    std::fs::write(&key, KEY).unwrap();
    assert_eq!(read_key_file(&key).await.unwrap(), KEY);
    assert!(read_key_file(&dir).await.is_err());
    let huge = dir.join("huge");
    std::fs::write(&huge, vec![b'a'; MAX_KEY_FILE as usize + 1]).unwrap();
    assert!(read_key_file(&huge).await.is_err());
    #[cfg(unix)]
    assert!(read_key_file(Path::new("/dev/zero")).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

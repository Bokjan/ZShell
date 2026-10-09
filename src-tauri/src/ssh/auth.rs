//! User authentication. Prompts (passwords, passphrases, keyboard-interactive questions)
//! are shown inline in the terminal, the way OpenSSH does it.
//!
//! A server can require more than one method (`AuthenticationMethods publickey,password`):
//! a method it accepts then fails with partial success and the methods still required, and
//! authentication goes on with those, as OpenSSH does.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use russh::client::{AuthResult, Handle, Handler, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::agent::AgentIdentity;
use russh::keys::ssh_encoding::Encode;
use russh::keys::ssh_key::private::KeypairData;
use russh::keys::ssh_key::public::KeyData;
use russh::keys::ssh_key::Signature;
use russh::keys::{load_secret_key, Algorithm, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey};
use russh::{MethodKind, MethodSet, Signer};

use crate::config::{AuthMethod, SshProfile};
use crate::error::Error;
use crate::secrets;
use crate::session::TermIo;

const MAX_ATTEMPTS: usize = 3;

/// How many times in a row a server may ask for more after partial success.
/// `AuthenticationMethods` lists a few methods at most; this only bounds a server that never
/// lets us in.
const MAX_ROUNDS: usize = 8;

/// Key files tried by automatic authentication, in `~/.ssh`.
const DEFAULT_KEYS: &[&str] = &["id_ed25519", "id_ecdsa", "id_rsa"];

/// How one try of a method went.
#[derive(Debug, PartialEq)]
enum Outcome {
    Success,
    /// Accepted, but the server requires more: one of these methods next.
    Partial(MethodSet),
    /// Not accepted with what we have for this method.
    Rejected,
}

impl From<AuthResult> for Outcome {
    fn from(result: AuthResult) -> Self {
        match result {
            AuthResult::Success => Outcome::Success,
            AuthResult::Failure { partial_success: true, remaining_methods } => Outcome::Partial(remaining_methods),
            AuthResult::Failure { .. } => Outcome::Rejected,
        }
    }
}

pub async fn authenticate<H: Handler>(session: &mut Handle<H>, profile: &SshProfile, io: &mut TermIo) -> Result<()> {
    Auth { session, profile, io, saved_password: None, tried: Vec::new() }.run().await
}

/// One authentication: the connection, the session's settings, and what has been used.
struct Auth<'a, H: Handler> {
    session: &'a mut Handle<H>,
    profile: &'a SshProfile,
    io: &'a mut TermIo,
    /// The saved password, read from the keychain when first needed (`None` until then);
    /// it answers one prompt only.
    saved_password: Option<Option<String>>,
    /// Keys offered so far, accepted or not; later rounds skip them (a server may require
    /// two different keys).
    tried: Vec<KeyData>,
}

impl<H: Handler> Auth<'_, H> {
    fn user(&self) -> &str {
        &self.profile.ssh.remote.username
    }

    /// The method the session is set to first, then what the server still requires, in
    /// OpenSSH's order.
    async fn run(mut self) -> Result<()> {
        let user = self.user().to_owned();
        let Some(methods) = (match self.session.authenticate_none(user).await? {
            AuthResult::Success => None,
            AuthResult::Failure { remaining_methods, .. } => Some(remaining_methods),
        }) else {
            return Ok(());
        };
        let mut outcome = self.configured(&methods).await?;
        for _ in 0..MAX_ROUNDS {
            let Outcome::Partial(required) = outcome else { break };
            outcome = self.automatic(&required).await?;
            if outcome == Outcome::Rejected {
                bail!(Error::new("auth.moreRequired").param("methods", method_names(&required)));
            }
        }
        ensure!(outcome == Outcome::Success, Error::new("auth.failed"));
        Ok(())
    }

    /// The method the session is set to, failing with why it didn't get through.
    async fn configured(&mut self, methods: &MethodSet) -> Result<Outcome> {
        let offered = |kinds: &[MethodKind]| kinds.iter().any(|kind| methods.contains(kind));
        let not_offered = || Error::new("auth.notOffered").param("methods", method_names(methods));
        let outcome = match &self.profile.ssh.auth {
            AuthMethod::Auto => {
                let outcome = self.automatic(methods).await?;
                ensure!(outcome != Outcome::Rejected, Error::new("auth.failed"));
                outcome
            }
            AuthMethod::Password => {
                ensure!(offered(&[MethodKind::Password, MethodKind::KeyboardInteractive]), not_offered());
                let outcome = self.passwords(methods).await?;
                ensure!(outcome != Outcome::Rejected, Error::new("auth.passwordFailed"));
                outcome
            }
            AuthMethod::PublicKey { key_path } => {
                ensure!(offered(&[MethodKind::PublicKey]), not_offered());
                let path = expand_home(key_path);
                let key = load_key(&path, self.io).await?;
                let outcome = self.sign_in_with(key).await?;
                ensure!(outcome != Outcome::Rejected, Error::new("auth.keyRejected").param("path", path.display()));
                outcome
            }
            AuthMethod::Agent => {
                ensure!(offered(&[MethodKind::PublicKey]), not_offered());
                let outcome = self.agent().await?;
                ensure!(outcome != Outcome::Rejected, Error::new("auth.agentAllRejected"));
                outcome
            }
        };
        Ok(outcome)
    }

    /// What OpenSSH tries by default, among `methods`: the agent's keys, the session's key
    /// file and the default ones, then keyboard-interactive, then password.
    async fn automatic(&mut self, methods: &MethodSet) -> Result<Outcome> {
        if methods.contains(&MethodKind::PublicKey) {
            let outcome = self.public_keys().await?;
            if outcome != Outcome::Rejected {
                return Ok(outcome);
            }
        }
        self.passwords(methods).await
    }

    async fn public_keys(&mut self) -> Result<Outcome> {
        // No agent, or one without keys, is not an error here.
        match self.agent().await {
            Ok(Outcome::Rejected) => {}
            Ok(outcome) => return Ok(outcome),
            Err(e) if e.downcast_ref::<Error>().is_some_and(|e| e.code() != "net.connectionLost") => {}
            Err(e) => return Err(e),
        }
        let ssh_dir = std::env::home_dir().unwrap_or_default().join(".ssh");
        let mut paths: Vec<PathBuf> = DEFAULT_KEYS.iter().map(|name| ssh_dir.join(name)).collect();
        if let AuthMethod::PublicKey { key_path } = &self.profile.ssh.auth {
            paths.insert(0, expand_home(key_path));
        }
        for path in paths {
            if path.is_file() {
                let outcome = self.try_key_file(&path).await?;
                if outcome != Outcome::Rejected {
                    return Ok(outcome);
                }
            }
        }
        Ok(Outcome::Rejected)
    }

    /// Keyboard-interactive, then password, as far as `methods` has them.
    async fn passwords(&mut self, methods: &MethodSet) -> Result<Outcome> {
        if methods.contains(&MethodKind::KeyboardInteractive) {
            let outcome = self.keyboard_interactive().await?;
            if outcome != Outcome::Rejected {
                return Ok(outcome);
            }
        }
        if methods.contains(&MethodKind::Password) {
            return self.password().await;
        }
        Ok(Outcome::Rejected)
    }

    /// The saved password, once. Quick connections have none.
    async fn take_saved_password(&mut self) -> Option<String> {
        if self.saved_password.is_none() {
            let saved = match self.profile.id.as_str() {
                "" => None,
                id => secrets::password(id.to_owned()).await,
            };
            self.saved_password = Some(saved);
        }
        self.saved_password.as_mut()?.take()
    }

    async fn password(&mut self) -> Result<Outcome> {
        let user = self.user().to_owned();
        if let Some(password) = self.take_saved_password().await {
            match self.session.authenticate_password(&user, password).await?.into() {
                Outcome::Rejected => self.io.print(&format!("{}\n", t!("terminal.savedPasswordRejected"))),
                outcome => return Ok(outcome),
            }
        }
        for _ in 0..MAX_ATTEMPTS {
            self.io.print(&format!("{user}@{}'s password: ", self.profile.ssh.remote.host));
            let password = self.io.read_line(false).await.context(Error::new("auth.cancelled"))?;
            match self.session.authenticate_password(&user, password).await?.into() {
                Outcome::Rejected => self.io.print("Permission denied, please try again.\n"),
                outcome => return Ok(outcome),
            }
        }
        Ok(Outcome::Rejected)
    }

    async fn keyboard_interactive(&mut self) -> Result<Outcome> {
        let user = self.user().to_owned();
        for _ in 0..MAX_ATTEMPTS {
            let mut response = self.session.authenticate_keyboard_interactive_start(&user, None).await?;
            loop {
                match response {
                    KeyboardInteractiveAuthResponse::Success => return Ok(Outcome::Success),
                    KeyboardInteractiveAuthResponse::Failure { partial_success: true, remaining_methods } => {
                        return Ok(Outcome::Partial(remaining_methods))
                    }
                    KeyboardInteractiveAuthResponse::Failure { .. } => break,
                    KeyboardInteractiveAuthResponse::InfoRequest { name, instructions, prompts } => {
                        for text in [name, instructions] {
                            if !text.is_empty() {
                                self.io.print(&format!("{text}\n"));
                            }
                        }
                        let mut answers = Vec::with_capacity(prompts.len());
                        for prompt in &prompts {
                            // The saved password answers a lone password prompt, once. Other
                            // hidden prompts are a second factor (a verification code), where
                            // a wrong answer may count against the account.
                            if prompts.len() == 1 && asks_for_password(&prompt.prompt, prompt.echo) {
                                if let Some(password) = self.take_saved_password().await {
                                    answers.push(password);
                                    continue;
                                }
                            }
                            self.io.print(&prompt.prompt);
                            answers.push(self.io.read_line(prompt.echo).await.context(Error::new("auth.cancelled"))?);
                        }
                        response = self.session.authenticate_keyboard_interactive_respond(answers).await?;
                    }
                }
            }
            self.io.print("Permission denied, please try again.\n");
        }
        Ok(Outcome::Rejected)
    }

    /// Records a key as offered; `false` if it was before.
    fn first_try(&mut self, key: &PublicKey) -> bool {
        let data = key.key_data();
        if self.tried.contains(data) {
            return false;
        }
        self.tried.push(data.clone());
        true
    }

    async fn sign_in_with(&mut self, key: PrivateKey) -> Result<Outcome> {
        if !self.first_try(key.public_key()) {
            return Ok(Outcome::Rejected);
        }
        let hash_alg = rsa_hash(self.session, key.public_key()).await?;
        let user = self.user().to_owned();
        Ok(self.session.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg)).await?.into())
    }

    /// Tries one key file: unreadable files and rejected keys are `Rejected`. An encrypted
    /// key's passphrase is only asked for once the server accepts its public key.
    async fn try_key_file(&mut self, path: &Path) -> Result<Outcome> {
        match load_secret_key(path, None) {
            Ok(key) => self.sign_in_with(key).await,
            Err(russh::keys::Error::KeyIsEncrypted) => self.try_encrypted_key(path).await,
            Err(_) => Ok(Outcome::Rejected),
        }
    }

    async fn try_encrypted_key(&mut self, path: &Path) -> Result<Outcome> {
        // The OpenSSH format keeps the public key readable; legacy PEM keys need the passphrase first.
        let Ok(encrypted) = PrivateKey::read_openssh_file(path) else {
            return match load_key(path, self.io).await {
                Ok(key) => self.sign_in_with(key).await,
                // Wrong passphrases skip the key, as for OpenSSH-format keys (and in OpenSSH).
                Err(e) if e.downcast_ref::<Error>().is_some_and(|e| e.code() == "auth.keyDecryptFailed") => Ok(Outcome::Rejected),
                Err(e) => Err(e),
            };
        };
        let public = encrypted.public_key().clone();
        if !self.first_try(&public) {
            return Ok(Outcome::Rejected);
        }
        let hash_alg = rsa_hash(self.session, &public).await?;
        let user = self.user().to_owned();
        let mut signer = PassphraseSigner { path, key: &encrypted, io: self.io, cancelled: false };
        let result = self.session.authenticate_publickey_with(user, public, hash_alg, &mut signer).await.map_err(SignError::lost)?;
        ensure!(!signer.cancelled, Error::new("auth.cancelled"));
        Ok(result.into())
    }

    /// The agent's keys, in its order, skipping those offered before.
    async fn agent(&mut self) -> Result<Outcome> {
        let mut agent = connect_agent().await.context(Error::new("auth.agentConnectFailed"))?;
        let identities = agent.request_identities().await.context(Error::new("auth.agentListFailed"))?;
        ensure!(!identities.is_empty(), Error::new("auth.agentEmpty"));

        let rsa_hash = self.session.best_supported_rsa_hash().await?.flatten();
        let user = self.user().to_owned();
        for identity in identities {
            // Certificates need a separate auth flow; skip them for now.
            let AgentIdentity::PublicKey { key, .. } = identity else {
                continue;
            };
            if !self.first_try(&key) {
                continue;
            }
            let hash_alg = if key.algorithm().is_rsa() { rsa_hash } else { None };
            let result = self
                .session
                .authenticate_publickey_with(&user, key, hash_alg, &mut AgentSigner(&mut agent))
                .await
                .map_err(SignError::lost)?;
            match result.into() {
                Outcome::Rejected => {}
                outcome => return Ok(outcome),
            }
        }
        Ok(Outcome::Rejected)
    }
}

/// Whether a keyboard-interactive prompt asks for the account's password (PAM's
/// `Password:`), rather than for a verification code or the like.
fn asks_for_password(prompt: &str, echo: bool) -> bool {
    !echo && prompt.to_lowercase().contains("password")
}

/// The methods as the server names them, for messages: `publickey, keyboard-interactive`.
fn method_names(methods: &MethodSet) -> String {
    methods.iter().map(<&str>::from).collect::<Vec<_>>().join(", ")
}

async fn load_key(path: &Path, io: &mut TermIo) -> Result<PrivateKey> {
    match load_secret_key(path, None) {
        Ok(key) => return Ok(key),
        Err(russh::keys::Error::KeyIsEncrypted) => {}
        Err(e) => return Err(e).context(Error::new("auth.keyReadFailed").param("path", path.display())),
    }
    for _ in 0..MAX_ATTEMPTS {
        io.print(&format!("Enter passphrase for key '{}': ", path.display()));
        let passphrase = io.read_line(false).await.context(Error::new("auth.cancelled"))?;
        match load_secret_key(path, Some(&passphrase)) {
            Ok(key) => return Ok(key),
            Err(_) => io.print(&format!("{}\n", t!("terminal.incorrectPassphrase"))),
        }
    }
    bail!(Error::new("auth.keyDecryptFailed").param("path", path.display()))
}

async fn rsa_hash<H: Handler>(session: &Handle<H>, key: &PublicKey) -> Result<Option<HashAlg>> {
    Ok(if key.algorithm().is_rsa() { session.best_supported_rsa_hash().await?.flatten() } else { None })
}

/// Signs with an encrypted key, asking for its passphrase when russh needs the signature,
/// which is only after the server has accepted the public key.
struct PassphraseSigner<'a> {
    path: &'a Path,
    key: &'a PrivateKey,
    io: &'a mut TermIo,
    cancelled: bool,
}

/// Signing itself never fails (see [`AgentSigner`]): this is the connection having closed.
#[derive(Debug)]
struct SignError;

impl SignError {
    /// Not an authentication error, which would stop reconnecting.
    fn lost(_: SignError) -> Error {
        Error::new("net.connectionLost")
    }
}

impl From<russh::SendError> for SignError {
    fn from(_: russh::SendError) -> Self {
        SignError
    }
}

impl Signer for PassphraseSigner<'_> {
    type Error = SignError;

    async fn auth_sign(&mut self, _key: &AgentIdentity, hash_alg: Option<HashAlg>, mut data: Vec<u8>) -> Result<Vec<u8>, SignError> {
        let signature = match self.decrypt().await {
            Some(key) => sign(&key, hash_alg, &data).ok(),
            None => None,
        };
        // Failing here would leave russh waiting for a signature forever. An invalid one
        // instead makes the server reject this key, and authentication moves on.
        let blob = signature.unwrap_or_else(|| invalid_signature(self.key.algorithm()));
        blob.encode(&mut data).map_err(|_| SignError)?;
        Ok(data)
    }
}

impl PassphraseSigner<'_> {
    async fn decrypt(&mut self) -> Option<PrivateKey> {
        for _ in 0..MAX_ATTEMPTS {
            self.io.print(&format!("Enter passphrase for key '{}': ", self.path.display()));
            let Some(passphrase) = self.io.read_line(false).await else {
                self.cancelled = true;
                return None;
            };
            match self.key.decrypt(passphrase) {
                Ok(key) => return Some(key),
                Err(_) => self.io.print(&format!("{}\n", t!("terminal.incorrectPassphrase"))),
            }
        }
        None
    }
}

/// An SSH signature blob over `data`, honouring the negotiated hash for RSA keys.
fn sign(key: &PrivateKey, hash_alg: Option<HashAlg>, data: &[u8]) -> Result<Vec<u8>> {
    let signature: Signature = match key.key_data() {
        KeypairData::Rsa(rsa) => russh::keys::signature::Signer::try_sign(&(rsa, hash_alg), data)?,
        _ => russh::keys::signature::Signer::try_sign(key, data)?,
    };
    Ok(signature.encode_vec()?)
}

/// A well-formed signature blob that cannot verify.
fn invalid_signature(algorithm: Algorithm) -> Vec<u8> {
    let mut blob = Vec::new();
    let _ = algorithm.as_str().encode(&mut blob);
    let _ = [0u8; 0].as_slice().encode(&mut blob);
    blob
}

type DynAgent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

/// Signs with the agent. An agent can refuse after the server has accepted the key (the
/// user denied a confirmation, or didn't touch a security key); failing then would leave
/// russh waiting for a signature forever, so as with [`PassphraseSigner`], an invalid one
/// makes the server reject the key and authentication moves on.
struct AgentSigner<'a, S: AgentStream>(&'a mut AgentClient<S>);

impl<S: AgentStream + Send + Unpin> Signer for AgentSigner<'_, S> {
    type Error = SignError;

    async fn auth_sign(&mut self, key: &AgentIdentity, hash_alg: Option<HashAlg>, data: Vec<u8>) -> Result<Vec<u8>, SignError> {
        if let Ok(signed) = self.0.sign_request(key, hash_alg, data.clone()).await {
            return Ok(signed);
        }
        let algorithm = match key {
            AgentIdentity::PublicKey { key, .. } => key.algorithm(),
            AgentIdentity::Certificate { certificate, .. } => certificate.algorithm(),
        };
        let mut data = data;
        invalid_signature(algorithm).encode(&mut data).map_err(|_| SignError)?;
        Ok(data)
    }
}

/// A connection to the local agent for a forwarded agent channel.
pub async fn agent_stream() -> Result<Box<dyn AgentStream + Send + Unpin>> {
    Ok(connect_agent().await?.into_inner())
}

#[cfg(unix)]
async fn connect_agent() -> Result<DynAgent> {
    Ok(AgentClient::connect_env().await?.dynamic())
}

#[cfg(windows)]
async fn connect_agent() -> Result<DynAgent> {
    // Prefer the Windows OpenSSH agent service, fall back to Pageant.
    match AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
        Ok(agent) => Ok(agent.dynamic()),
        Err(_) => Ok(AgentClient::connect_pageant().await?.dynamic()),
    }
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => std::env::home_dir().map(|home| home.join(rest)).unwrap_or_else(|| path.into()),
        None => path.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use russh::keys::PublicKeyOrCertificate;
    use russh::server::{self, Auth as ServerAuth, Response};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::config::Remote;
    use crate::session::SessionInput;

    /// Serves as the host key and the user's key.
    const KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACBmJacSIfoWm34XIu1XxBNljXN6rq4lFsD5uSBwBECvQAAAAIiHA/GKhwPx
igAAAAtzc2gtZWQyNTUxOQAAACBmJacSIfoWm34XIu1XxBNljXN6rq4lFsD5uSBwBECvQA
AAAEBQtLIgjSDC4h72YyOg7rcfkBUD/Fm2W/HoNlMi5m03MmYlpxIh+habfhci7VfEE2WN
c3quriUWwPm5IHAEQK9AAAAAAAECAwQF
-----END OPENSSH PRIVATE KEY-----
";

    /// A server that requires `steps` in turn, as sshd's `AuthenticationMethods publickey,password`
    /// does. Keyboard-interactive asks `prompt` (hidden); it and password take `secret`.
    struct Server {
        steps: Vec<MethodKind>,
        passed: usize,
        prompt: &'static str,
        secret: &'static str,
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

        async fn auth_publickey(&mut self, _user: &str, _key: &PublicKey) -> Result<ServerAuth, Self::Error> {
            Ok(self.pass(MethodKind::PublicKey))
        }

        async fn auth_password(&mut self, _user: &str, password: &str) -> Result<ServerAuth, Self::Error> {
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

    struct Client;

    impl russh::client::Handler for Client {
        type Error = russh::Error;

        async fn check_server_key(&mut self, _key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    /// Authenticates with `auth` (the key file is `KEY`) and `saved` as the saved password,
    /// against a server that first offers `offered`; `typed` answers prompts. Returns the
    /// result and what the terminal showed.
    async fn sign_in(server: Server, offered: &[MethodKind], auth: AuthMethod, saved: Option<&str>, typed: &str) -> (Result<()>, String) {
        let key = PrivateKey::from_openssh(KEY).unwrap();
        let config = server::Config {
            methods: MethodSet::from(offered),
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        };
        let (client_side, server_side) = tokio::io::duplex(1 << 16);
        tokio::spawn(async move {
            if let Ok(session) = server::run_stream(Arc::new(config), server_side, server).await {
                let _ = session.await;
            }
        });
        let mut session = russh::client::connect_stream(Arc::new(russh::client::Config::default()), client_side, Client).await.unwrap();

        let key_path = std::env::temp_dir().join(format!("zshell-auth-key-{}", std::process::id()));
        std::fs::write(&key_path, KEY).unwrap();
        let mut profile = SshProfile::quick("t".into(), Remote::new("example.com".into(), 22, "alice".into()));
        profile.ssh.auth = match auth {
            AuthMethod::PublicKey { .. } => AuthMethod::PublicKey { key_path: key_path.display().to_string() },
            auth => auth,
        };
        let (mut io, input, output, _events) = TermIo::detached((80, 24));
        if !typed.is_empty() {
            input.send(SessionInput::Data(typed.as_bytes().to_vec())).unwrap();
        }
        let auth = Auth { session: &mut session, profile: &profile, io: &mut io, saved_password: Some(saved.map(Into::into)), tried: Vec::new() };
        // A prompt nothing answers would wait for ever.
        let result = tokio::time::timeout(Duration::from_secs(10), auth.run()).await.expect("authentication waits for input");
        let shown = String::from_utf8_lossy(&output.try_iter().flatten().collect::<Vec<_>>()).into_owned();
        (result, shown)
    }

    fn key_file() -> AuthMethod {
        AuthMethod::PublicKey { key_path: String::new() }
    }

    /// `AuthenticationMethods publickey,keyboard-interactive` with a one-time code (Duo,
    /// Google Authenticator): the saved password must not be spent on the code prompt.
    #[tokio::test]
    async fn key_then_verification_code() {
        let server = Server { steps: vec![MethodKind::PublicKey, MethodKind::KeyboardInteractive], passed: 0, prompt: "Verification code: ", secret: "123456" };
        let (result, shown) = sign_in(server, &[MethodKind::PublicKey], key_file(), Some("hunter2"), "123456\r").await;
        result.unwrap();
        assert!(shown.contains("Verification code: ") && !shown.contains("Permission denied"), "{shown:?}");
    }

    /// `AuthenticationMethods publickey,password`: the saved password answers the second step.
    #[tokio::test]
    async fn key_then_saved_password() {
        let server = Server { steps: vec![MethodKind::PublicKey, MethodKind::Password], passed: 0, prompt: "", secret: "hunter2" };
        let (result, shown) = sign_in(server, &[MethodKind::PublicKey], key_file(), Some("hunter2"), "").await;
        result.unwrap();
        assert!(!shown.contains("password"), "{shown:?}");
    }

    /// Keyboard-interactive comes before password, as in OpenSSH, and the saved password
    /// answers PAM's password prompt.
    #[tokio::test]
    async fn keyboard_interactive_before_password() {
        let server = Server { steps: vec![MethodKind::KeyboardInteractive], passed: 0, prompt: "Password: ", secret: "hunter2" };
        let offered = [MethodKind::Password, MethodKind::KeyboardInteractive];
        let (result, shown) = sign_in(server, &offered, AuthMethod::Password, Some("hunter2"), "").await;
        result.unwrap();
        assert!(!shown.contains("Password: "), "{shown:?}");
    }

    /// A second step we cannot do names what the server requires.
    #[tokio::test]
    async fn reports_methods_it_cannot_use() {
        let server = Server { steps: vec![MethodKind::PublicKey, MethodKind::HostBased], passed: 0, prompt: "", secret: "" };
        let (result, _) = sign_in(server, &[MethodKind::PublicKey], key_file(), None, "").await;
        let error = Error::from(result.unwrap_err());
        assert_eq!(error.code(), "auth.moreRequired");
        assert!(error.to_string().contains("hostbased"), "{error}");

        let server = Server { steps: vec![MethodKind::PublicKey], passed: 0, prompt: "", secret: "" };
        let (result, _) = sign_in(server, &[MethodKind::PublicKey], AuthMethod::Password, Some("hunter2"), "").await;
        assert_eq!(Error::from(result.unwrap_err()).code(), "auth.notOffered");
    }

    #[test]
    fn tells_password_prompts_from_codes() {
        assert!(asks_for_password("Password: ", false));
        assert!(asks_for_password("alice@example.com's password:", false));
        assert!(!asks_for_password("Verification code: ", false));
        assert!(!asks_for_password("Passcode or option (1-3): ", false));
        assert!(!asks_for_password("Password: ", true));
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
        let key = russh::keys::PublicKey::from_openssh("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIErPvl8mnDbXiALZf/lPNK6oppUWYB4bP5AYBJmvaFkh").unwrap();
        let identity = AgentIdentity::PublicKey { key, comment: String::new() };
        let signed = AgentSigner(&mut agent).auth_sign(&identity, None, b"data".to_vec()).await.unwrap();
        let mut expected = b"data".to_vec();
        invalid_signature(Algorithm::Ed25519).encode(&mut expected).unwrap();
        assert_eq!(signed, expected);
    }
}

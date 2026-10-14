//! User authentication. Prompts (passwords, passphrases, keyboard-interactive questions)
//! are shown inline in the terminal, the way OpenSSH does it.
//!
//! A server can require more than one method (`AuthenticationMethods publickey,password`):
//! a method it accepts then fails with partial success and the methods still required, and
//! authentication goes on with those, as OpenSSH does.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

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
use tokio::sync::watch;

use super::handler::EndReason;
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

// OpenSSH's own prompts, word for word: people know them, and scripts that drive a terminal
// (expect) wait for them. They stay in English rather than going into the message catalog
// (see "后端" in docs/I18N.md), like the answers to the host key question.

/// After a password or an answer the server refused.
const PERMISSION_DENIED: &str = "Permission denied, please try again.\n";

fn password_prompt(user: &str, host: &str) -> String {
    format!("{user}@{host}'s password: ")
}

fn passphrase_prompt(key: &Path) -> String {
    format!("Enter passphrase for key '{}': ", key.display())
}

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

/// Authenticates as `profile` says. `ended` receives why the connection ended, from its
/// [`super::handler::ClientHandler`].
pub async fn authenticate<H: Handler>(
    session: &mut Handle<H>,
    profile: &SshProfile,
    io: &mut TermIo,
    ended: watch::Receiver<Option<EndReason>>,
) -> Result<()> {
    Auth { session, profile, io, ended, saved_password: None, tried: Vec::new() }.run().await
}

/// One authentication: the connection, the session's settings, and what has been used.
struct Auth<'a, H: Handler> {
    session: &'a mut Handle<H>,
    profile: &'a SshProfile,
    io: &'a mut TermIo,
    ended: watch::Receiver<Option<EndReason>>,
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

    /// What the server answered, unless the connection has ended. russh reports a connection
    /// that ends while it waits for the answer as a refusal, and one that ended before as an
    /// error; neither says anything about the credentials, and taking them as a refusal would
    /// stop reconnecting.
    async fn answer<T, E: Into<anyhow::Error>>(&mut self, result: std::result::Result<T, E>) -> Result<T> {
        if self.session.is_closed() {
            bail!(self.end().await);
        }
        result.map_err(Into::into)
    }

    /// Why the connection ended during authentication. The server ending it (OpenSSH's
    /// "Too many authentication failures") is an authentication failure, which OpenSSH
    /// reports and gives up on; anything else is a lost connection.
    async fn end(&mut self) -> Error {
        // The handler hears of the end just after russh stops taking requests.
        let _ = tokio::time::timeout(Duration::from_millis(500), self.ended.wait_for(Option::is_some)).await;
        match self.ended.borrow().clone() {
            Some(EndReason { detail, by_server: true }) => Error::new("auth.disconnected").detail(detail),
            Some(EndReason { detail, by_server: false }) => Error::new("net.connectionLost").detail(detail),
            None => Error::new("net.connectionLost"),
        }
    }

    /// The method the session is set to first, then what the server still requires, in
    /// OpenSSH's order.
    async fn run(mut self) -> Result<()> {
        let user = self.user().to_owned();
        let result = self.session.authenticate_none(user).await;
        let Some(methods) = (match self.answer(result).await? {
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
                // As in automatic mode, the passphrase is asked for once the server accepts
                // the key, but a key that can't be read or decrypted is an error.
                let outcome = match load_secret_key(&path, None) {
                    Ok(key) => self.sign_in_with(key).await?,
                    Err(russh::keys::Error::KeyIsEncrypted) => self.try_encrypted_key(&path, true).await?,
                    Err(e) => return Err(e).context(Error::new("auth.keyReadFailed").param("path", path.display())),
                };
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
            Err(e) if e.downcast_ref::<Error>().is_some_and(|e| e.code().starts_with("auth.agent")) => {}
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
            let result = self.session.authenticate_password(&user, password).await;
            match self.answer(result).await?.into() {
                Outcome::Rejected => self.io.print(&format!("{}\n", t!("terminal.savedPasswordRejected"))),
                outcome => return Ok(outcome),
            }
        }
        for _ in 0..MAX_ATTEMPTS {
            self.io.print(&password_prompt(&user, &self.profile.ssh.remote.host));
            let password = self.io.read_line(false).await.context(Error::new("auth.cancelled"))?;
            let result = self.session.authenticate_password(&user, password).await;
            match self.answer(result).await?.into() {
                Outcome::Rejected => self.io.print(PERMISSION_DENIED),
                outcome => return Ok(outcome),
            }
        }
        Ok(Outcome::Rejected)
    }

    async fn keyboard_interactive(&mut self) -> Result<Outcome> {
        let user = self.user().to_owned();
        for _ in 0..MAX_ATTEMPTS {
            let result = self.session.authenticate_keyboard_interactive_start(&user, None).await;
            let mut response = self.answer(result).await?;
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
                            if prompts.len() == 1 && asks_for_password(&prompt.prompt, prompt.echo, &user) {
                                if let Some(password) = self.take_saved_password().await {
                                    answers.push(password);
                                    continue;
                                }
                            }
                            self.io.print(&prompt.prompt);
                            answers.push(self.io.read_line(prompt.echo).await.context(Error::new("auth.cancelled"))?);
                        }
                        let result = self.session.authenticate_keyboard_interactive_respond(answers).await;
                        response = self.answer(result).await?;
                    }
                }
            }
            self.io.print(PERMISSION_DENIED);
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

    async fn rsa_hash(&mut self, key: &PublicKey) -> Result<Option<HashAlg>> {
        if !key.algorithm().is_rsa() {
            return Ok(None);
        }
        let result = self.session.best_supported_rsa_hash().await;
        Ok(self.answer(result).await?.flatten())
    }

    async fn sign_in_with(&mut self, key: PrivateKey) -> Result<Outcome> {
        if !self.first_try(key.public_key()) {
            return Ok(Outcome::Rejected);
        }
        let hash_alg = self.rsa_hash(key.public_key()).await?;
        let user = self.user().to_owned();
        let result = self.session.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg)).await;
        Ok(self.answer(result).await?.into())
    }

    /// Tries one key file: unreadable files and rejected keys are `Rejected`. An encrypted
    /// key's passphrase is only asked for once the server accepts its public key.
    async fn try_key_file(&mut self, path: &Path) -> Result<Outcome> {
        match load_secret_key(path, None) {
            Ok(key) => self.sign_in_with(key).await,
            Err(russh::keys::Error::KeyIsEncrypted) => self.try_encrypted_key(path, false).await,
            Err(_) => Ok(Outcome::Rejected),
        }
    }

    /// Tries an encrypted key file. Wrong passphrases skip the key (as in OpenSSH), unless
    /// it is the one the session is set to use (`chosen`): that fails with
    /// `auth.keyDecryptFailed`.
    async fn try_encrypted_key(&mut self, path: &Path, chosen: bool) -> Result<Outcome> {
        let decrypt_failed = || Error::new("auth.keyDecryptFailed").param("path", path.display());
        // The OpenSSH format keeps the public key readable; legacy PEM keys need the passphrase first.
        let Ok(encrypted) = PrivateKey::read_openssh_file(path) else {
            return match load_key(path, self.io).await {
                Ok(key) => self.sign_in_with(key).await,
                Err(e) if !chosen && e.downcast_ref::<Error>().is_some_and(|e| e.code() == "auth.keyDecryptFailed") => {
                    Ok(Outcome::Rejected)
                }
                Err(e) => Err(e),
            };
        };
        let public = encrypted.public_key().clone();
        if !self.first_try(&public) {
            return Ok(Outcome::Rejected);
        }
        let hash_alg = self.rsa_hash(&public).await?;
        let user = self.user().to_owned();
        let mut signer = PassphraseSigner { path, key: &encrypted, io: self.io, cancelled: false, decrypt_failed: false };
        let result = self.session.authenticate_publickey_with(user, public, hash_alg, &mut signer).await.map_err(SignError::lost);
        // What happened to the passphrase first: a server may hang up on the invalid
        // signature sent without it.
        ensure!(!signer.cancelled, Error::new("auth.cancelled"));
        ensure!(!(chosen && signer.decrypt_failed), decrypt_failed());
        Ok(self.answer(result).await?.into())
    }

    /// The agent's keys, in its order, skipping those offered before.
    async fn agent(&mut self) -> Result<Outcome> {
        let mut agent = connect_agent().await.context(Error::new("auth.agentConnectFailed"))?;
        let identities = agent.request_identities().await.context(Error::new("auth.agentListFailed"))?;
        ensure!(!identities.is_empty(), Error::new("auth.agentEmpty"));

        let result = self.session.best_supported_rsa_hash().await;
        let rsa_hash = self.answer(result).await?.flatten();
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
            let result =
                self.session.authenticate_publickey_with(&user, key, hash_alg, &mut AgentSigner(&mut agent)).await.map_err(SignError::lost);
            match self.answer(result).await?.into() {
                Outcome::Rejected => {}
                outcome => return Ok(outcome),
            }
        }
        Ok(Outcome::Rejected)
    }
}

/// Whether a keyboard-interactive prompt asks for `user`'s password, in one of the forms PAM
/// and sshd use (`Password:`, `alice@host's password:`, `Password for alice:`), rather than
/// for a verification code, another account's password or the like. Anything else
/// (`One-time password:`, `Password + OTP:`, `Password for root:` as alice) is left to the
/// user: a wrong answer may count against the account.
fn asks_for_password(prompt: &str, echo: bool, user: &str) -> bool {
    let text = prompt.trim().trim_end_matches(':').trim_end().to_lowercase();
    let user = user.to_lowercase();
    // Only the part before an `@` counts: after it comes a Kerberos realm (`alice@EXAMPLE.COM`)
    // or the name the server gives itself, which needn't be the one connected to.
    let names_user = |name: &str| !user.is_empty() && (name == user || name.split_once('@').is_some_and(|(name, _)| name == user));
    !echo
        && (text == "password"
            || text.strip_suffix("'s password").is_some_and(names_user)
            || text.strip_prefix("password for ").is_some_and(names_user))
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
        io.print(&passphrase_prompt(path));
        let passphrase = io.read_line(false).await.context(Error::new("auth.cancelled"))?;
        match load_secret_key(path, Some(&passphrase)) {
            Ok(key) => return Ok(key),
            Err(_) => io.print(&format!("{}\n", t!("terminal.incorrectPassphrase"))),
        }
    }
    bail!(Error::new("auth.keyDecryptFailed").param("path", path.display()))
}

/// Signs with an encrypted key, asking for its passphrase when russh needs the signature,
/// which is only after the server has accepted the public key.
struct PassphraseSigner<'a> {
    path: &'a Path,
    key: &'a PrivateKey,
    io: &'a mut TermIo,
    cancelled: bool,
    /// Every passphrase typed was wrong.
    decrypt_failed: bool,
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
            self.io.print(&passphrase_prompt(self.path));
            let Some(passphrase) = self.io.read_line(false).await else {
                self.cancelled = true;
                return None;
            };
            match self.key.decrypt(passphrase) {
                Ok(key) => return Some(key),
                Err(_) => self.io.print(&format!("{}\n", t!("terminal.incorrectPassphrase"))),
            }
        }
        self.decrypt_failed = true;
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

/// The first of the key files automatic authentication tries that exists here, as `~/.ssh/…`:
/// what a session set to use a key file starts with.
pub fn default_key_path() -> Option<String> {
    let ssh_dir = std::env::home_dir()?.join(".ssh");
    DEFAULT_KEYS.iter().find(|name| ssh_dir.join(name).is_file()).map(|name| format!("~/.ssh/{name}"))
}

/// Whether the key file a session names (`~/…` included) exists here.
pub fn key_file_exists(path: &str) -> bool {
    expand_home(path).is_file()
}

/// `~/…` (or `~\…`) in the home folder: `std::env::home_dir()`, `USERPROFILE` on Windows, the
/// folder OpenSSH uses too (for `~/.ssh`).
fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => std::env::home_dir().map(|home| home.join(rest)).unwrap_or_else(|| path.into()),
        None => path.into(),
    }
}

#[cfg(test)]
mod tests;

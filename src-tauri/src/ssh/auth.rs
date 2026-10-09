//! User authentication. Prompts (passwords, passphrases, keyboard-interactive questions)
//! are shown inline in the terminal, the way OpenSSH does it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::agent::AgentIdentity;
use russh::keys::ssh_encoding::Encode;
use russh::keys::ssh_key::private::KeypairData;
use russh::keys::ssh_key::Signature;
use russh::keys::{load_secret_key, Algorithm, HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh::{MethodKind, MethodSet, Signer};

use super::handler::ClientHandler;
use crate::config::{AuthMethod, Profile};
use crate::error::Error;
use crate::secrets;
use crate::session::TermIo;

const MAX_ATTEMPTS: usize = 3;

/// Key files tried by automatic authentication, in `~/.ssh`.
const DEFAULT_KEYS: &[&str] = &["id_ed25519", "id_ecdsa", "id_rsa"];

type Session = Handle<ClientHandler>;

pub async fn authenticate(session: &mut Session, profile: &Profile, io: &mut TermIo) -> Result<()> {
    let user = profile.username.as_str();
    match &profile.auth {
        AuthMethod::Auto => auto(session, profile, io).await,
        AuthMethod::Password => password(session, profile, io).await,
        AuthMethod::PublicKey { key_path } => public_key(session, user, key_path, io).await,
        AuthMethod::Agent => agent(session, user).await,
    }
}

/// What OpenSSH tries by default: the agent's keys, the default key files, then
/// keyboard-interactive or password.
async fn auto(session: &mut Session, profile: &Profile, io: &mut TermIo) -> Result<()> {
    let user = profile.username.as_str();
    let Some(methods) = offered_methods(session, user).await? else {
        return Ok(());
    };
    if methods.contains(&MethodKind::PublicKey) {
        if try_agent(session, user).await? {
            return Ok(());
        }
        let ssh_dir = std::env::home_dir().unwrap_or_default().join(".ssh");
        let paths: Vec<PathBuf> = DEFAULT_KEYS.iter().map(|name| ssh_dir.join(name)).collect();
        for path in paths {
            if path.is_file() && try_key_file(session, user, &path, io).await? {
                return Ok(());
            }
        }
    }
    if methods.contains(&MethodKind::Password) || methods.contains(&MethodKind::KeyboardInteractive) {
        return password_methods(session, profile, &methods, io).await;
    }
    bail!(Error::new("auth.failed"))
}

/// The methods the server accepts for `user`, or `None` if it let us in without any.
async fn offered_methods(session: &mut Session, user: &str) -> Result<Option<MethodSet>> {
    Ok(match session.authenticate_none(user).await? {
        AuthResult::Success => None,
        AuthResult::Failure { remaining_methods, .. } => Some(remaining_methods),
    })
}

async fn password(session: &mut Session, profile: &Profile, io: &mut TermIo) -> Result<()> {
    // Ask the server which methods it accepts: many only allow keyboard-interactive.
    match offered_methods(session, &profile.username).await? {
        Some(methods) => password_methods(session, profile, &methods, io).await,
        None => Ok(()),
    }
}

async fn password_methods(session: &mut Session, profile: &Profile, methods: &MethodSet, io: &mut TermIo) -> Result<()> {
    let user = profile.username.as_str();
    let mut saved = secrets::password(profile.id.clone()).await;
    if !methods.contains(&MethodKind::Password) && methods.contains(&MethodKind::KeyboardInteractive) {
        return keyboard_interactive(session, user, saved, io).await;
    }

    if let Some(password) = saved.take() {
        if session.authenticate_password(user, password).await?.success() {
            return Ok(());
        }
        io.print(&format!("{}\n", t!("terminal.savedPasswordRejected")));
    }
    for _ in 0..MAX_ATTEMPTS {
        io.print(&format!("{user}@{}'s password: ", profile.host));
        let password = io.read_line(false).await.context(Error::new("auth.cancelled"))?;
        if session.authenticate_password(user, password).await?.success() {
            return Ok(());
        }
        io.print("Permission denied, please try again.\n");
    }
    bail!(Error::new("auth.passwordFailed"))
}

async fn keyboard_interactive(
    session: &mut Session,
    user: &str,
    mut saved_password: Option<String>,
    io: &mut TermIo,
) -> Result<()> {
    for _ in 0..MAX_ATTEMPTS {
        let mut response = session.authenticate_keyboard_interactive_start(user, None).await?;
        loop {
            match response {
                KeyboardInteractiveAuthResponse::Success => return Ok(()),
                KeyboardInteractiveAuthResponse::Failure { .. } => break,
                KeyboardInteractiveAuthResponse::InfoRequest { name, instructions, prompts } => {
                    for text in [name, instructions] {
                        if !text.is_empty() {
                            io.print(&format!("{text}\n"));
                        }
                    }
                    let mut answers = Vec::with_capacity(prompts.len());
                    for prompt in &prompts {
                        // A lone hidden prompt is the password; answer it from the keychain once.
                        if prompts.len() == 1 && !prompt.echo {
                            if let Some(password) = saved_password.take() {
                                answers.push(password);
                                continue;
                            }
                        }
                        io.print(&prompt.prompt);
                        answers.push(io.read_line(prompt.echo).await.context(Error::new("auth.cancelled"))?);
                    }
                    response = session.authenticate_keyboard_interactive_respond(answers).await?;
                }
            }
        }
        io.print("Permission denied, please try again.\n");
    }
    bail!(Error::new("auth.failed"))
}

async fn public_key(session: &mut Session, user: &str, key_path: &str, io: &mut TermIo) -> Result<()> {
    let path = expand_home(key_path);
    let key = load_key(&path, io).await?;
    let hash_alg = if key.algorithm().is_rsa() {
        session.best_supported_rsa_hash().await?.flatten()
    } else {
        None
    };
    let result = session
        .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg))
        .await?;
    ensure!(result.success(), Error::new("auth.keyRejected").param("path", path.display()));
    Ok(())
}

async fn load_key(path: &PathBuf, io: &mut TermIo) -> Result<PrivateKey> {
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

/// Tries one key file without failing: unreadable files and rejected keys return `false`.
/// An encrypted key's passphrase is only asked for once the server accepts its public key.
async fn try_key_file(session: &mut Session, user: &str, path: &Path, io: &mut TermIo) -> Result<bool> {
    let key = match load_secret_key(path, None) {
        Ok(key) => key,
        Err(russh::keys::Error::KeyIsEncrypted) => return try_encrypted_key(session, user, path, io).await,
        Err(_) => return Ok(false),
    };
    let hash_alg = rsa_hash(session, key.public_key()).await?;
    let result = session
        .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg))
        .await?;
    Ok(result.success())
}

async fn try_encrypted_key(session: &mut Session, user: &str, path: &Path, io: &mut TermIo) -> Result<bool> {
    // The OpenSSH format keeps the public key readable; legacy PEM keys need the passphrase first.
    let Ok(encrypted) = PrivateKey::read_openssh_file(path) else {
        let key = match load_key(&path.to_path_buf(), io).await {
            Ok(key) => key,
            // Wrong passphrases skip the key, as for OpenSSH-format keys (and in OpenSSH).
            Err(e) if e.downcast_ref::<Error>().is_some_and(|e| e.code() == "auth.keyDecryptFailed") => return Ok(false),
            Err(e) => return Err(e),
        };
        let hash_alg = rsa_hash(session, key.public_key()).await?;
        let result = session
            .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg))
            .await?;
        return Ok(result.success());
    };
    let public = encrypted.public_key().clone();
    let hash_alg = rsa_hash(session, &public).await?;
    let mut signer = PassphraseSigner { path, key: &encrypted, io, cancelled: false };
    let result = session
        .authenticate_publickey_with(user, public, hash_alg, &mut signer)
        .await
        .map_err(SignError::lost)?;
    ensure!(!signer.cancelled, Error::new("auth.cancelled"));
    Ok(result.success())
}

async fn rsa_hash(session: &Session, key: &russh::keys::PublicKey) -> Result<Option<HashAlg>> {
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

/// Tries the agent's keys without failing: no agent, no keys or rejected keys return `false`.
async fn try_agent(session: &mut Session, user: &str) -> Result<bool> {
    match agent(session, user).await {
        Ok(()) => Ok(true),
        Err(e) if e.downcast_ref::<Error>().is_some_and(|e| e.code() != "net.connectionLost") => Ok(false),
        Err(e) => Err(e),
    }
}

async fn agent(session: &mut Session, user: &str) -> Result<()> {
    let mut agent = connect_agent().await.context(Error::new("auth.agentConnectFailed"))?;
    let identities = agent.request_identities().await.context(Error::new("auth.agentListFailed"))?;
    ensure!(!identities.is_empty(), Error::new("auth.agentEmpty"));

    let rsa_hash = session.best_supported_rsa_hash().await?.flatten();
    for identity in identities {
        // Certificates need a separate auth flow; skip them for now.
        let AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        let hash_alg = if key.algorithm().is_rsa() { rsa_hash } else { None };
        let result = session
            .authenticate_publickey_with(user, key, hash_alg, &mut AgentSigner(&mut agent))
            .await
            .map_err(SignError::lost)?;
        if result.success() {
            return Ok(());
        }
    }
    bail!(Error::new("auth.agentAllRejected"))
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
    use russh::keys::PublicKey;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

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
        let key = PublicKey::from_openssh("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIErPvl8mnDbXiALZf/lPNK6oppUWYB4bP5AYBJmvaFkh").unwrap();
        let identity = AgentIdentity::PublicKey { key, comment: String::new() };
        let signed = AgentSigner(&mut agent).auth_sign(&identity, None, b"data".to_vec()).await.unwrap();
        let mut expected = b"data".to_vec();
        invalid_signature(Algorithm::Ed25519).encode(&mut expected).unwrap();
        assert_eq!(signed, expected);
    }
}

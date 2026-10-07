//! User authentication. Prompts (passwords, passphrases, keyboard-interactive questions)
//! are shown inline in the terminal, the way OpenSSH does it.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, ensure, Context, Result};
use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::agent::AgentIdentity;
use russh::keys::{load_secret_key, PrivateKey, PrivateKeyWithHashAlg};
use russh::MethodKind;

use super::host_key::ClientHandler;
use crate::config::{AuthMethod, Profile};
use crate::secrets;
use crate::session::TermIo;

const MAX_ATTEMPTS: usize = 3;

type Session = Handle<ClientHandler>;

pub async fn authenticate(session: &mut Session, profile: &Profile, io: &mut TermIo) -> Result<()> {
    let user = profile.username.as_str();
    match &profile.auth {
        AuthMethod::Password => password(session, profile, io).await,
        AuthMethod::PublicKey { key_path } => public_key(session, user, key_path, io).await,
        AuthMethod::Agent => agent(session, user).await,
    }
}

async fn password(session: &mut Session, profile: &Profile, io: &mut TermIo) -> Result<()> {
    let user = profile.username.as_str();
    let mut saved = secrets::get_password(&profile.id);

    // Ask the server which methods it accepts: many only allow keyboard-interactive.
    let methods = match session.authenticate_none(user).await? {
        AuthResult::Success => return Ok(()),
        AuthResult::Failure { remaining_methods, .. } => remaining_methods,
    };
    if !methods.contains(&MethodKind::Password) && methods.contains(&MethodKind::KeyboardInteractive) {
        return keyboard_interactive(session, user, saved, io).await;
    }

    if let Some(password) = saved.take() {
        if session.authenticate_password(user, password).await?.success() {
            return Ok(());
        }
        io.print("已保存的密码被服务器拒绝。\n");
    }
    for _ in 0..MAX_ATTEMPTS {
        io.print(&format!("{user}@{}'s password: ", profile.host));
        let password = io.read_line(false).await.context("已取消")?;
        if session.authenticate_password(user, password).await?.success() {
            return Ok(());
        }
        io.print("Permission denied, please try again.\n");
    }
    bail!("密码认证失败")
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
                        answers.push(io.read_line(prompt.echo).await.context("已取消")?);
                    }
                    response = session.authenticate_keyboard_interactive_respond(answers).await?;
                }
            }
        }
        io.print("Permission denied, please try again.\n");
    }
    bail!("认证失败")
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
    ensure!(result.success(), "服务器拒绝了私钥 {}", path.display());
    Ok(())
}

async fn load_key(path: &PathBuf, io: &mut TermIo) -> Result<PrivateKey> {
    match load_secret_key(path, None) {
        Ok(key) => return Ok(key),
        Err(russh::keys::Error::KeyIsEncrypted) => {}
        Err(e) => return Err(e).with_context(|| format!("无法读取私钥 {}", path.display())),
    }
    for _ in 0..MAX_ATTEMPTS {
        io.print(&format!("Enter passphrase for key '{}': ", path.display()));
        let passphrase = io.read_line(false).await.context("已取消")?;
        match load_secret_key(path, Some(&passphrase)) {
            Ok(key) => return Ok(key),
            Err(_) => io.print("密码短语错误。\n"),
        }
    }
    bail!("无法解密私钥 {}", path.display())
}

async fn agent(session: &mut Session, user: &str) -> Result<()> {
    let mut agent = connect_agent().await.context("无法连接 SSH agent")?;
    let identities = agent.request_identities().await.context("无法读取 SSH agent 中的密钥")?;
    ensure!(!identities.is_empty(), "SSH agent 中没有密钥");

    let rsa_hash = session.best_supported_rsa_hash().await?.flatten();
    for identity in identities {
        // Certificates need a separate auth flow; skip them for now.
        let AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        let hash_alg = if key.algorithm().is_rsa() { rsa_hash } else { None };
        let result = session
            .authenticate_publickey_with(user, key, hash_alg, &mut agent)
            .await
            .map_err(|e| anyhow!("SSH agent 签名失败：{e:?}"))?;
        if result.success() {
            return Ok(());
        }
    }
    bail!("服务器拒绝了 SSH agent 中的所有密钥")
}

type DynAgent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

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

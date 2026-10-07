//! SSH shell sessions on top of russh.

mod auth;
mod connections;
mod host_key;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use russh::{client, ChannelMsg};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::config::Profile;
use crate::session::{SessionEvent, SessionId, SessionInput, TermIo};
pub use connections::Connections;
use host_key::ClientHandler;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Session backend: connects, authenticates and bridges a remote shell to the terminal.
pub async fn run(profile: Profile, id: SessionId, mut io: TermIo, connections: Connections) {
    let result = shell(&profile, id, &mut io, &connections).await;
    connections.close(id);
    match &result {
        Ok(Some(code)) => io.print(&format!("\n\x1b[2m[连接已关闭，退出码 {code}]\x1b[0m\n")),
        Ok(None) => io.print("\n\x1b[2m[连接已关闭]\x1b[0m\n"),
        Err(e) => io.print(&format!("\n\x1b[31m{e:#}\x1b[0m\n")),
    }
    io.event(SessionEvent::Closed { error: result.err().map(|e| format!("{e:#}")) });
}

/// Returns the remote exit status, if the server reported one.
async fn shell(profile: &Profile, id: SessionId, io: &mut TermIo, connections: &Connections) -> Result<Option<u32>> {
    io.print(&format!(
        "\x1b[2m正在连接 {}@{}:{} ...\x1b[0m\n",
        profile.username, profile.host, profile.port
    ));
    let mut session = connect(profile, io).await?;
    auth::authenticate(&mut session, profile, io).await?;
    let session = Arc::new(session);
    connections.insert(id, session.clone());

    let channel = session.channel_open_session().await.context("无法打开会话通道")?;
    let (cols, rows) = io.size;
    channel.request_pty(false, "xterm-256color", cols.into(), rows.into(), 0, 0, &[]).await?;
    channel.request_shell(false).await?;
    io.event(SessionEvent::Connected);

    let (mut reader, writer) = channel.split();
    let mut exit_status = None;
    loop {
        tokio::select! {
            msg = reader.wait() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    io.write(data.to_vec());
                }
                Some(ChannelMsg::ExitStatus { exit_status: status }) => exit_status = Some(status),
                Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
            input = io.recv() => match input {
                Some(SessionInput::Data(data)) => writer.data_bytes(data).await?,
                Some(SessionInput::Resize { cols, rows }) => {
                    writer.window_change(cols.into(), rows.into(), 0, 0).await?;
                }
                None => break,
            },
        }
    }
    Ok(exit_status)
}

async fn connect(profile: &Profile, io: &mut TermIo) -> Result<client::Handle<ClientHandler>> {
    let target = format!("{}:{}", profile.host, profile.port);
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&target))
        .await
        .map_err(|_| anyhow!("连接 {target} 超时"))?
        .with_context(|| format!("无法连接 {target}"))?;
    stream.set_nodelay(true)?;

    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    });
    let (queries_tx, mut queries) = mpsc::channel(1);
    let handler = ClientHandler::new(profile.host.clone(), profile.port, queries_tx);

    // Drive the handshake while answering host key questions from the handler.
    let handshake = client::connect_stream(config, stream, handler);
    tokio::pin!(handshake);
    loop {
        tokio::select! {
            result = &mut handshake => {
                return result.map_err(|e| match e {
                    russh::Error::UnknownKey => anyhow!("主机密钥未被信任，已取消连接"),
                    e => anyhow!(e).context("SSH 握手失败"),
                });
            }
            Some(query) = queries.recv() => {
                let accepted = host_key::confirm(io, &profile.host, profile.port, &query).await;
                let _ = query.reply.send(accepted);
            }
        }
    }
}

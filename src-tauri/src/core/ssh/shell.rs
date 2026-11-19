//! Interactive shell channel helpers (PTY + shell).

use russh::client::{Handle, Msg};
use russh::Channel;

use crate::error::AppResult;
use super::client::ClientHandler;

/// Open a session channel, request a PTY and spawn the user's login shell.
/// When `x11` is set, also requests X11 forwarding with a random
/// MIT-MAGIC-COOKIE-1 cookie (the server provisions it via xauth).
pub async fn open_shell(
    handle: &Handle<ClientHandler>,
    term: &str,
    cols: u32,
    rows: u32,
    x11: bool,
) -> AppResult<Channel<Msg>> {
    let channel = handle.channel_open_session().await?;
    channel
        .request_pty(false, term, cols, rows, 0, 0, &[])
        .await?;
    if x11 {
        // 32-hex random cookie; sshd registers it in the session Xauthority.
        let cookie = uuid::Uuid::new_v4().simple().to_string();
        if let Err(e) = channel
            .request_x11(true, false, "MIT-MAGIC-COOKIE-1", cookie, 0)
            .await
        {
            tracing::warn!("x11 forwarding request rejected: {e}");
        }
    }
    channel.request_shell(true).await?;
    Ok(channel)
}

/// Open an SFTP subsystem channel on an existing connection.
pub async fn open_sftp(
    handle: &Handle<ClientHandler>,
) -> AppResult<russh_sftp::client::SftpSession> {
    let channel = handle.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| crate::error::AppError::Sftp(e.to_string()))?;
    Ok(sftp)
}

pub type SshHandle = Handle<ClientHandler>;

//! Minimal Rlogin client (RFC 1282): null-terminated handshake then raw
//! byte passthrough. No encryption — a warning is pushed to the terminal.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::core::session::{SessionCommand, SessionState};
use crate::core::terminal::pipe::SessionPipe;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RloginConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    #[serde(default = "default_term")]
    pub term: String,
}

fn default_port() -> u16 {
    513
}
fn default_term() -> String {
    "xterm-256color".into()
}

pub async fn run_rlogin(
    cfg: RloginConfig,
    pipe: SessionPipe,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
) -> AppResult<()> {
    pipe.push_state(SessionState::Connecting.as_str(), None);
    pipe.push_data(
        "\x1b[1;33m[OpenTerm] 警告: Rlogin 为明文协议,凭据与数据不加密。\x1b[0m\r\n".as_bytes(),
    );
    let stream = TcpStream::connect((cfg.host.as_str(), cfg.port)).await?;
    let (mut rd, mut wr) = stream.into_split();

    // Handshake: \0 client-user \0 server-user \0 term/speed \0
    let hello = format!("\0{0}\0{0}\0{1}/38400\0", cfg.username, cfg.term);
    wr.write_all(hello.as_bytes()).await?;

    // Server replies with a single NUL byte on success.
    let mut ack = [0u8; 1];
    let n = rd.read(&mut ack).await?;
    if n == 0 || ack[0] != 0 {
        return Err(AppError::Config("rlogin handshake rejected by server".into()));
    }

    pipe.push_state(SessionState::Connected.as_str(), None);

    let mut buf = [0u8; 8192];
    loop {
        tokio::select! {
            n = rd.read(&mut buf) => {
                match n {
                    Ok(0) => return Ok(()),
                    Ok(n) => pipe.push_data(&buf[..n]),
                    Err(e) => return Err(e.into()),
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCommand::Input(data)) => {
                        if wr.write_all(&data).await.is_err() {
                            return Ok(());
                        }
                    }
                    // rlogin window-size negotiation (0x80 marker) not needed
                    // for basic operation; resize is a no-op.
                    Some(SessionCommand::Resize { .. }) => {}
                    Some(SessionCommand::Close) | None => return Ok(()),
                }
            }
        }
    }
}

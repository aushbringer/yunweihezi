//! SCP (RCP over an exec channel): single-file upload/download.
//! Recursive transfers belong to SFTP; SCP is the fallback for servers
//! without an SFTP subsystem.

use std::path::Path;
use std::sync::Arc;

use russh::client::{Handle, Msg};
use russh::{Channel, ChannelMsg};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};

use super::client::ClientHandler;

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Buffered reader over a channel for the RCP control protocol.
struct Rcp {
    channel: Channel<Msg>,
    buf: std::collections::VecDeque<u8>,
}

impl Rcp {
    async fn fill_one(&mut self) -> AppResult<bool> {
        match self.channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                self.buf.extend(data.iter());
                Ok(true)
            }
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => Ok(false),
            _ => Ok(true),
        }
    }

    async fn read_byte(&mut self) -> AppResult<u8> {
        loop {
            if let Some(b) = self.buf.pop_front() {
                return Ok(b);
            }
            if !self.fill_one().await? {
                return Err(AppError::Other(anyhow::anyhow!("scp: channel closed")));
            }
        }
    }

    async fn read_line(&mut self) -> AppResult<String> {
        let mut out = Vec::new();
        loop {
            let b = self.read_byte().await?;
            if b == b'\n' {
                break;
            }
            out.push(b);
            if out.len() > 4096 {
                return Err(AppError::Other(anyhow::anyhow!("scp: line too long")));
            }
        }
        Ok(String::from_utf8_lossy(&out).to_string())
    }

    /// RCP ack: 0 = ok, 1/2 = error message (line).
    async fn expect_ack(&mut self) -> AppResult<()> {
        let code = self.read_byte().await?;
        match code {
            0 => Ok(()),
            1 | 2 => {
                let msg = self.read_line().await?;
                Err(AppError::Other(anyhow::anyhow!("scp remote error: {msg}")))
            }
            c => Err(AppError::Other(anyhow::anyhow!("scp: bad ack byte {c}"))),
        }
    }

    async fn send_ack(&mut self) -> AppResult<()> {
        self.channel.data(&[0u8][..]).await?;
        Ok(())
    }
}

/// Upload one local file to a remote path (file or directory).
pub async fn upload(
    handle: &Arc<Handle<ClientHandler>>,
    local: &Path,
    remote: &str,
    on_progress: impl Fn(u64, u64),
) -> AppResult<()> {
    let meta = tokio::fs::metadata(local).await?;
    let size = meta.len();
    let name = local
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| AppError::Config("bad local file name".into()))?;

    let channel = handle.channel_open_session().await?;
    channel
        .exec(true, format!("scp -t {}", shell_quote(remote)))
        .await?;
    let mut rcp = Rcp {
        channel,
        buf: Default::default(),
    };
    rcp.expect_ack().await?;
    let header = format!("C0644 {size} {name}\n");
    rcp.channel.data(header.as_bytes()).await?;
    rcp.expect_ack().await?;

    let mut file = tokio::fs::File::open(local).await?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut sent = 0u64;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        rcp.channel.data(&buf[..n]).await?;
        sent += n as u64;
        on_progress(sent, size);
    }
    rcp.channel.data(&[0u8][..]).await?;
    rcp.expect_ack().await?;
    let _ = rcp.channel.close().await;
    Ok(())
}

/// Download one remote file to a local path.
pub async fn download(
    handle: &Arc<Handle<ClientHandler>>,
    remote: &str,
    local: &Path,
    on_progress: impl Fn(u64, u64),
) -> AppResult<()> {
    let channel = handle.channel_open_session().await?;
    channel
        .exec(true, format!("scp -f {}", shell_quote(remote)))
        .await?;
    let mut rcp = Rcp {
        channel,
        buf: Default::default(),
    };
    rcp.send_ack().await?;

    // Expect a "C<mode> <size> <name>" header (skip T/D records for robustness).
    let (size, _name) = loop {
        let line = rcp.read_line().await?;
        if line.is_empty() {
            return Err(AppError::Other(anyhow::anyhow!("scp: empty header")));
        }
        match line.as_bytes()[0] {
            b'C' => {
                let mut parts = line[1..].splitn(3, ' ');
                let _mode = parts.next().unwrap_or("0644");
                let size: u64 = parts
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| AppError::Other(anyhow::anyhow!("scp: bad size")))?;
                let name = parts.next().unwrap_or("scp-download").to_string();
                break (size, name);
            }
            1 | 2 => {
                return Err(AppError::Other(anyhow::anyhow!(
                    "scp remote error: {}",
                    &line[1..]
                )))
            }
            _ => continue, // ignore T (times) / D (dir) records
        }
    };
    rcp.send_ack().await?;

    let mut file = tokio::fs::File::create(local).await?;
    let mut remaining = size;
    while remaining > 0 {
        let b = rcp.read_byte().await?;
        // Collect byte-by-byte is slow; drain the buffer in bulk instead.
        let mut chunk = vec![b];
        let take = (remaining - 1).min(rcp.buf.len() as u64) as usize;
        for _ in 0..take {
            chunk.push(rcp.buf.pop_front().unwrap());
        }
        file.write_all(&chunk).await?;
        remaining -= chunk.len() as u64;
        on_progress(size - remaining, size);
        if rcp.buf.is_empty() && remaining > 0 && !rcp.fill_one().await? {
            return Err(AppError::Other(anyhow::anyhow!("scp: truncated stream")));
        }
    }
    file.flush().await?;
    rcp.expect_ack().await?; // trailing zero after file data
    rcp.send_ack().await?;
    let _ = rcp.channel.close().await;
    Ok(())
}

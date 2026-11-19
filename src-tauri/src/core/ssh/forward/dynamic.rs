//! Dynamic forward (-D): a local SOCKS5 server; each CONNECT is piped
//! through a direct-tcpip channel (the SSH server resolves the target).

use std::sync::Arc;

use russh::client::Handle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::core::ssh::client::ClientHandler;
use crate::error::{AppError, AppResult};

const VERSION: u8 = 5;
const CMD_CONNECT: u8 = 1;
const ATYP_IPV4: u8 = 1;
const ATYP_DOMAIN: u8 = 3;
const ATYP_IPV6: u8 = 4;

async fn handshake(tcp: &mut TcpStream) -> AppResult<()> {
    let mut head = [0u8; 2];
    tcp.read_exact(&mut head).await?;
    if head[0] != VERSION {
        return Err(AppError::Config("not a SOCKS5 client".into()));
    }
    let nmethods = head[1] as usize;
    let mut methods = vec![0u8; nmethods];
    tcp.read_exact(&mut methods).await?;
    // No authentication.
    tcp.write_all(&[VERSION, 0]).await?;
    Ok(())
}

/// Parse the CONNECT request, returning (host, port).
async fn read_request(tcp: &mut TcpStream) -> AppResult<(String, u16)> {
    let mut head = [0u8; 4];
    tcp.read_exact(&mut head).await?;
    if head[0] != VERSION || head[1] != CMD_CONNECT {
        reply(tcp, 7).await?; // command not supported
        return Err(AppError::Config("SOCKS5: only CONNECT is supported".into()));
    }
    let host = match head[3] {
        ATYP_IPV4 => {
            let mut b = [0u8; 4];
            tcp.read_exact(&mut b).await?;
            b.map(|x| x.to_string()).join(".")
        }
        ATYP_DOMAIN => {
            let mut len = [0u8; 1];
            tcp.read_exact(&mut len).await?;
            let mut b = vec![0u8; len[0] as usize];
            tcp.read_exact(&mut b).await?;
            String::from_utf8_lossy(&b).to_string()
        }
        ATYP_IPV6 => {
            let mut b = [0u8; 16];
            tcp.read_exact(&mut b).await?;
            let segs: Vec<String> = b
                .chunks(2)
                .map(|c| format!("{:02x}{:02x}", c[0], c[1]))
                .collect();
            segs.join(":")
        }
        _ => {
            reply(tcp, 8).await?; // address type not supported
            return Err(AppError::Config("SOCKS5: bad address type".into()));
        }
    };
    let mut pb = [0u8; 2];
    tcp.read_exact(&mut pb).await?;
    Ok((host, u16::from_be_bytes(pb)))
}

async fn reply(tcp: &mut TcpStream, code: u8) -> AppResult<()> {
    // BND.ADDR/BND.PORT are informational; zeros are fine.
    let mut r = vec![VERSION, code, 0, ATYP_IPV4, 0, 0, 0, 0, 0, 0];
    tcp.write_all(&r).await?;
    r.clear();
    Ok(())
}

pub async fn run(
    handle: Arc<Handle<ClientHandler>>,
    listener: TcpListener,
    mut stop: watch::Receiver<bool>,
) -> AppResult<()> {
    tracing::info!(?listener, "dynamic (SOCKS5) forward up");

    loop {
        tokio::select! {
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let (mut tcp, _peer) = match accepted {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("socks accept: {e}");
                        continue;
                    }
                };
                let handle = handle.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve(&handle, &mut tcp).await {
                        tracing::debug!("socks conn: {e}");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn serve(handle: &Arc<Handle<ClientHandler>>, tcp: &mut TcpStream) -> AppResult<()> {
    handshake(tcp).await?;
    let (host, port) = read_request(tcp).await?;
    let ch = handle
        .channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0)
        .await;
    match ch {
        Ok(ch) => {
            reply(tcp, 0).await?;
            let mut stream = ch.into_stream();
            let _ = tokio::io::copy_bidirectional(tcp, &mut stream).await;
        }
        Err(_) => {
            reply(tcp, 5).await?; // connection refused
        }
    }
    Ok(())
}

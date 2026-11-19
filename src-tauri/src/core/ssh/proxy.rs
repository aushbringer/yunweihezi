//! Outbound proxy transports: SOCKS4a / SOCKS5 (no-auth) / HTTP CONNECT.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProxyConfig {
    #[serde(rename = "socks5")]
    Socks5 { host: String, port: u16 },
    #[serde(rename = "socks4")]
    Socks4 { host: String, port: u16 },
    #[serde(rename = "http")]
    Http { host: String, port: u16 },
}

impl ProxyConfig {
    fn addr(&self) -> (&str, u16) {
        match self {
            Self::Socks5 { host, port } | Self::Socks4 { host, port } | Self::Http { host, port } => {
                (host, *port)
            }
        }
    }
}

/// Connect to `target` through the proxy. Returns a plain TcpStream ready for
/// the SSH handshake.
pub async fn connect_via(
    proxy: &ProxyConfig,
    target_host: &str,
    target_port: u16,
) -> AppResult<TcpStream> {
    let (ph, pp) = proxy.addr();
    let mut stream = TcpStream::connect((ph, pp)).await?;
    match proxy {
        ProxyConfig::Socks5 { .. } => socks5_handshake(&mut stream, target_host, target_port).await?,
        ProxyConfig::Socks4 { .. } => socks4a_handshake(&mut stream, target_host, target_port).await?,
        ProxyConfig::Http { .. } => http_connect(&mut stream, target_host, target_port).await?,
    }
    Ok(stream)
}

async fn socks5_handshake(stream: &mut TcpStream, host: &str, port: u16) -> AppResult<()> {
    // greeting: version 5, 1 method, no-auth
    stream.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut resp = [0u8; 2];
    stream.read_exact(&mut resp).await?;
    if resp != [0x05, 0x00] {
        return Err(AppError::Config("SOCKS5: proxy requires authentication".into()));
    }
    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(AppError::Config("SOCKS5: host name too long".into()));
    }
    let mut req = Vec::with_capacity(7 + host_bytes.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
    req.extend_from_slice(host_bytes);
    req.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&req).await?;
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await?;
    if head[1] != 0x00 {
        return Err(AppError::Config(format!("SOCKS5 connect failed, code {}", head[1])));
    }
    // drain bound address
    let skip = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            len[0] as usize
        }
        t => return Err(AppError::Config(format!("SOCKS5: bad atyp {t}"))),
    };
    let mut buf = vec![0u8; skip + 2];
    stream.read_exact(&mut buf).await?;
    Ok(())
}

async fn socks4a_handshake(stream: &mut TcpStream, host: &str, port: u16) -> AppResult<()> {
    let mut req = Vec::with_capacity(9 + host.len() + 1);
    req.push(0x04); // version
    req.push(0x01); // connect
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&[0, 0, 0, 1]); // SOCKS4a marker
    req.push(0x00); // empty userid
    req.extend_from_slice(host.as_bytes());
    req.push(0x00);
    stream.write_all(&req).await?;
    let mut resp = [0u8; 8];
    stream.read_exact(&mut resp).await?;
    if resp[1] != 0x5a {
        return Err(AppError::Config(format!(
            "SOCKS4 connect failed, code {}",
            resp[1]
        )));
    }
    Ok(())
}

async fn http_connect(stream: &mut TcpStream, host: &str, port: u16) -> AppResult<()> {
    let req = format!(
        "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Connection: keep-alive\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(AppError::Config("HTTP proxy closed connection".into()));
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]);
            let ok = head
                .lines()
                .next()
                .map(|l| l.contains(" 200") || l.ends_with(" 200"))
                .unwrap_or(false);
            if !ok {
                return Err(AppError::Config(format!(
                    "HTTP CONNECT rejected: {}",
                    head.lines().next().unwrap_or("")
                )));
            }
            break;
        }
        if buf.len() > 16 * 1024 {
            return Err(AppError::Config("HTTP proxy response too large".into()));
        }
    }
    Ok(())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

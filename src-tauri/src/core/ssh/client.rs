//! russh client handler + connect & authenticate (password / key / KI / agent),
//! jump-host chains and outbound proxies.

use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle, KeyboardInteractiveAuthResponse, Msg};
use russh::keys::agent::AgentIdentity;
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{Channel, ChannelId};
use tokio::net::TcpStream;

use crate::error::{AppError, AppResult};

use super::{agent as agent_mod, proxy::ProxyConfig};

/// Authentication material for one SSH hop.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AuthMethod {
    #[serde(rename = "password")]
    Password { password: String },
    /// Keyboard-interactive; every prompt is answered with `password`
    /// (covers the common password-over-KI servers).
    #[serde(rename = "keyboardInteractive")]
    KeyboardInteractive { password: String },
    #[serde(rename = "key")]
    Key {
        /// Path to an OpenSSH / PuTTY-PPK private key.
        path: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
    #[serde(rename = "agent")]
    Agent,
}

/// One intermediate jump host.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JumpHop {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshSessionConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    /// Ordered list of jump hosts (empty = direct connection).
    #[serde(default)]
    pub jump_chain: Vec<JumpHop>,
    /// Outbound proxy for the first TCP connection.
    #[serde(default)]
    pub proxy: Option<ProxyConfig>,
    #[serde(default = "default_term")]
    pub term: String,
    #[serde(default = "default_cols")]
    pub cols: u32,
    #[serde(default = "default_rows")]
    pub rows: u32,
    /// Keepalive interval in seconds (0 = off).
    #[serde(default = "default_keepalive")]
    pub keepalive_secs: u64,
    /// Auto-reconnect with exponential backoff on unexpected disconnects.
    #[serde(default)]
    pub reconnect: bool,
    /// Request X11 forwarding (requires a local X server: VcXsrv/Xming on
    /// Windows, XQuartz on macOS).
    #[serde(default)]
    pub x11: bool,
}

fn default_port() -> u16 {
    22
}
fn default_term() -> String {
    "xterm-256color".into()
}
fn default_cols() -> u32 {
    80
}
fn default_rows() -> u32 {
    24
}
fn default_keepalive() -> u64 {
    30
}

/// russh event handler. Data events are consumed per-channel via
/// `Channel::wait()` in the session task. The router table lets forwarded-
/// tcpip channels (remote forwards) find their local target.
pub struct ClientHandler {
    pub router: super::forward::ForwardRouter,
}

impl ClientHandler {
    pub fn new(router: super::forward::ForwardRouter) -> Self {
        Self { router }
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // TODO(P2): known_hosts verification + TOFU prompt
        Ok(true)
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        _data: &[u8],
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// X11 forwarding: the server opens one x11 channel per remote X client
    /// connection; bridge it to the local X server on 127.0.0.1:6000.
    async fn server_channel_open_x11(
        &mut self,
        channel: Channel<Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        tokio::spawn(async move {
            match TcpStream::connect(("127.0.0.1", 6000u16)).await {
                Ok(tcp) => {
                    super::forward::local::bridge_channel_tcp(channel, tcp).await;
                }
                Err(e) => {
                    tracing::warn!(
                        "X11: cannot reach local X server 127.0.0.1:6000 ({e}); \
                         install/start an X server (VcXsrv, Xming, XQuartz)"
                    );
                    let _ = channel.close().await;
                }
            }
        });
        Ok(())
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let target = self.router.lookup(connected_address, connected_port);
        match target {
            Some((host, port)) => {
                reply.accept().await;
                tokio::spawn(async move {
                    match tokio::net::TcpStream::connect((host.as_str(), port)).await {
                        Ok(tcp) => {
                            super::forward::local::bridge_channel_tcp(channel, tcp).await;
                        }
                        Err(e) => {
                            tracing::warn!("remote forward target {host}:{port}: {e}");
                            let _ = channel.close().await;
                        }
                    }
                });
            }
            None => {
                tracing::warn!(
                    "forwarded-tcpip for unregistered {connected_address}:{connected_port}"
                );
                reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
            }
        }
        Ok(())
    }
}

fn client_config(keepalive_secs: u64) -> Arc<client::Config> {
    Arc::new(client::Config {
        nodelay: true,
        inactivity_timeout: None,
        keepalive_interval: if keepalive_secs > 0 {
            Some(Duration::from_secs(keepalive_secs))
        } else {
            None
        },
        keepalive_max: 3,
        ..Default::default()
    })
}

/// A fully authenticated SSH connection. Intermediate jump-host handles are
/// kept alive for the lifetime of the connection. The handle is Arc-wrapped
/// so auxiliary channels (SFTP, tunnels) can share the connection.
pub struct SshConnection {
    pub handle: Arc<Handle<ClientHandler>>,
    /// Remote-forward routing table shared with the handler.
    pub router: super::forward::ForwardRouter,
    _jump_handles: Vec<Handle<ClientHandler>>,
}

/// Connect (through proxy and/or jump chain) and authenticate.
pub async fn connect(cfg: &SshSessionConfig) -> AppResult<SshConnection> {
    let config = client_config(cfg.keepalive_secs);
    let router = super::forward::ForwardRouter::default();
    let mut jump_handles: Vec<Handle<ClientHandler>> = Vec::new();

    // Build the transport for the first hop (direct TCP or via proxy).
    let (first_host, first_port, first_user, first_auth, has_jumps) =
        match cfg.jump_chain.first() {
            Some(hop) => (hop.host.clone(), hop.port, hop.username.clone(), hop.auth.clone(), true),
            None => (
                cfg.host.clone(),
                cfg.port,
                cfg.username.clone(),
                cfg.auth.clone(),
                false,
            ),
        };

    let stream = match &cfg.proxy {
        Some(p) => super::proxy::connect_via(p, &first_host, first_port).await?,
        None => TcpStream::connect((first_host.as_str(), first_port)).await?,
    };
    let mut current =
        client::connect_stream(config.clone(), stream, ClientHandler::new(router.clone())).await?;
    authenticate(&mut current, &first_user, &first_auth).await?;

    if !has_jumps {
        return Ok(SshConnection {
            handle: Arc::new(current),
            router,
            _jump_handles: jump_handles,
        });
    }

    // Walk the remaining hops over direct-tcpip channels.
    for hop in &cfg.jump_chain[1..] {
        let ch = current
            .channel_open_direct_tcpip(hop.host.clone(), hop.port as u32, "127.0.0.1", 0)
            .await?;
        let mut handle = client::connect_stream(
            config.clone(),
            ch.into_stream(),
            ClientHandler::new(router.clone()),
        )
        .await?;
        authenticate(&mut handle, &hop.username, &hop.auth).await?;
        jump_handles.push(current);
        current = handle;
    }

    // Final hop: from the last jump host into the target.
    let ch = current
        .channel_open_direct_tcpip(cfg.host.clone(), cfg.port as u32, "127.0.0.1", 0)
        .await?;
    let mut handle =
        client::connect_stream(config, ch.into_stream(), ClientHandler::new(router.clone()))
            .await?;
    authenticate(&mut handle, &cfg.username, &cfg.auth).await?;
    jump_handles.push(current);

    Ok(SshConnection {
        handle: Arc::new(handle),
        router,
        _jump_handles: jump_handles,
    })
}

pub async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    username: &str,
    auth: &AuthMethod,
) -> AppResult<()> {
    match auth {
        AuthMethod::Password { password } => {
            let res = handle.authenticate_password(username, password).await?;
            if !res.success() {
                return Err(anyhow::anyhow!("password authentication failed").into());
            }
        }
        AuthMethod::KeyboardInteractive { password } => {
            let mut resp = handle
                .authenticate_keyboard_interactive_start(username, None::<String>)
                .await?;
            loop {
                match resp {
                    KeyboardInteractiveAuthResponse::Success => break,
                    KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                        let answers = prompts.iter().map(|_| password.clone()).collect();
                        resp = handle
                            .authenticate_keyboard_interactive_respond(answers)
                            .await?;
                    }
                    KeyboardInteractiveAuthResponse::Failure { .. } => {
                        return Err(anyhow::anyhow!("keyboard-interactive authentication failed")
                            .into());
                    }
                }
            }
        }
        AuthMethod::Key { path, passphrase } => {
            let key = russh::keys::load_secret_key(path, passphrase.as_deref())?;
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            let res = handle
                .authenticate_publickey(
                    username,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await?;
            if !res.success() {
                return Err(anyhow::anyhow!("publickey authentication failed").into());
            }
        }
        AuthMethod::Agent => {
            let mut agent = agent_mod::connect_agent().await?;
            let identities = agent
                .request_identities()
                .await
                .map_err(|e| AppError::Config(format!("agent request_identities: {e}")))?;
            if identities.is_empty() {
                return Err(AppError::Config("ssh-agent holds no identities".into()));
            }
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            let mut succeeded = false;
            for identity in &identities {
                let key = match identity {
                    AgentIdentity::PublicKey { key, .. } => key.clone(),
                    // Skip certificates for now (P2: cert auth).
                    AgentIdentity::Certificate { .. } => continue,
                };
                // AgentClient implements russh's `Signer` trait directly.
                match handle
                    .authenticate_publickey_with(username, key, hash, &mut agent)
                    .await
                {
                    Ok(res) if res.success() => {
                        succeeded = true;
                        break;
                    }
                    Ok(_) => continue,   // key rejected, try next
                    Err(_) => continue,  // signing error, try next
                }
            }
            if !succeeded {
                return Err(anyhow::anyhow!(
                    "agent authentication failed ({} identities tried)",
                    identities.len()
                )
                .into());
            }
        }
    }
    Ok(())
}

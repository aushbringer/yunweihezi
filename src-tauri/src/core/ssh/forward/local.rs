//! Local forward (-L): listen on a local TCP port, pipe each accepted
//! connection through a direct-tcpip channel to the target.

use std::sync::Arc;

use russh::client::{Handle, Msg};
use tokio::io::copy_bidirectional;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::core::ssh::client::ClientHandler;
use crate::error::AppResult;

/// Run the local-forward accept loop on a pre-bound listener until `stop`
/// flips to true. Binding happens in the caller so failures surface
/// synchronously to the UI.
pub async fn run(
    handle: Arc<Handle<ClientHandler>>,
    listener: TcpListener,
    target_host: String,
    target_port: u16,
    mut stop: watch::Receiver<bool>,
) -> AppResult<()> {
    tracing::info!(?listener, %target_host, %target_port, "local forward up");

    loop {
        tokio::select! {
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let (mut tcp, peer) = match accepted {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("local forward accept: {e}");
                        continue;
                    }
                };
                let handle = handle.clone();
                let target_host = target_host.clone();
                tokio::spawn(async move {
                    let ch = handle
                        .channel_open_direct_tcpip(
                            target_host,
                            target_port as u32,
                            peer.ip().to_string(),
                            peer.port() as u32,
                        )
                        .await;
                    let ch = match ch {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!("direct-tcpip open failed: {e}");
                            return;
                        }
                    };
                    let mut stream = ch.into_stream();
                    let _ = copy_bidirectional(&mut tcp, &mut stream).await;
                });
            }
        }
    }
    Ok(())
}

/// Bridge helper reused by remote forwarding: copy between a russh channel
/// and a TCP stream until EOF.
pub async fn bridge_channel_tcp(channel: russh::Channel<Msg>, mut tcp: tokio::net::TcpStream) {
    let mut stream = channel.into_stream();
    let _ = copy_bidirectional(&mut stream, &mut tcp).await;
}

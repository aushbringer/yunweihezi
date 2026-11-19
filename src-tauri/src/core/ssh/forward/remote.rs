//! Remote forward (-R): ask the server to listen; incoming connections
//! arrive as forwarded-tcpip channels handled by ClientHandler, which
//! consults the ForwardRouter and bridges to the local target.

use std::sync::Arc;

use russh::client::Handle;
use tokio::sync::watch;

use crate::core::ssh::client::ClientHandler;
use crate::error::AppResult;

use super::ForwardRouter;

/// Request the remote listener and keep the route registered until `stop`.
pub async fn run(
    handle: Arc<Handle<ClientHandler>>,
    router: ForwardRouter,
    bind_host: &str,
    bind_port: u16,
    target_host: &str,
    target_port: u16,
    mut stop: watch::Receiver<bool>,
) -> AppResult<()> {
    let bound = handle.tcpip_forward(bind_host, bind_port as u32).await?;
    // When bind_port == 0 the server picks the port; use the returned one.
    let effective = if bind_port == 0 { bound as u16 } else { bind_port };
    router.add(bind_host, effective as u32, target_host, target_port);
    tracing::info!(%bind_host, %effective, %target_host, %target_port, "remote forward up");

    let _ = stop.changed().await;

    router.remove(bind_host, effective as u32);
    let _ = handle.cancel_tcpip_forward(bind_host, effective as u32).await;
    Ok(())
}

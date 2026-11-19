//! SSH port forwarding: local (-L), remote (-R) and dynamic (-D, SOCKS5),
//! plus the registry of running tunnels.

pub mod dynamic;
pub mod local;
pub mod remote;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, watch};

use crate::error::{AppError, AppResult};

/// Shared routing table for remote (-R) forwards: maps the server-side
/// bound (address, port) to the local target the channel should reach.
#[derive(Clone, Default)]
pub struct ForwardRouter {
    /// (bind_addr, bind_port) → (target_host, target_port)
    routes: Arc<std::sync::Mutex<HashMap<(String, u32), (String, u16)>>>,
}

impl ForwardRouter {
    pub fn add(&self, bind_addr: &str, bind_port: u32, target_host: &str, target_port: u16) {
        self.routes.lock().unwrap().insert(
            (bind_addr.to_string(), bind_port),
            (target_host.to_string(), target_port),
        );
    }

    pub fn remove(&self, bind_addr: &str, bind_port: u32) {
        self.routes
            .lock()
            .unwrap()
            .remove(&(bind_addr.to_string(), bind_port));
    }

    pub fn lookup(&self, addr: &str, port: u32) -> Option<(String, u16)> {
        let routes = self.routes.lock().unwrap();
        routes
            .get(&(addr.to_string(), port))
            .cloned()
            // bind address may normalize (localhost → 127.0.0.1): fall back
            // to a port-only match when it is unambiguous.
            .or_else(|| {
                let mut it = routes
                    .iter()
                    .filter(move |((_, p), _)| *p == port)
                    .map(|(_, v)| v.clone());
                let first = it.next()?;
                if it.next().is_none() { Some(first) } else { None }
            })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningTunnel {
    pub id: String,
    pub session_id: String,
    /// local | remote | dynamic
    pub kind: String,
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
}

struct TunnelHandle {
    info: RunningTunnel,
    stop: watch::Sender<bool>,
}

/// Registry of live tunnels.
#[derive(Clone, Default)]
pub struct TunnelManager {
    tunnels: Arc<Mutex<HashMap<String, TunnelHandle>>>,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, info: RunningTunnel) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        self.tunnels.lock().await.insert(
            info.id.clone(),
            TunnelHandle {
                info,
                stop: tx,
            },
        );
        rx
    }

    /// Mark a tunnel stopped internally (task exited on its own).
    pub async fn unregister(&self, id: &str) {
        self.tunnels.lock().await.remove(id);
    }

    pub async fn stop(&self, id: &str) -> AppResult<()> {
        let handle = self.tunnels.lock().await.remove(id);
        match handle {
            Some(h) => {
                let _ = h.stop.send(true);
                Ok(())
            }
            None => Err(AppError::Config(format!("tunnel not found: {id}"))),
        }
    }

    pub async fn list(&self) -> Vec<RunningTunnel> {
        self.tunnels
            .lock()
            .await
            .values()
            .map(|h| h.info.clone())
            .collect()
    }

    /// Stop every tunnel bound to a session (called when it closes).
    pub async fn stop_for_session(&self, session_id: &str) {
        let ids: Vec<String> = {
            let t = self.tunnels.lock().await;
            t.iter()
                .filter(|(_, h)| h.info.session_id == session_id)
                .map(|(k, _)| k.clone())
                .collect()
        };
        for id in ids {
            let _ = self.stop(&id).await;
        }
    }
}

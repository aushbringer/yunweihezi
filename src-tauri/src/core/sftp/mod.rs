//! SFTP subsystem sessions + background transfer queue.

pub mod transfer;

use std::collections::HashMap;
use std::sync::Arc;

use russh_sftp::client::SftpSession;
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;

use crate::error::{AppError, AppResult};

/// One row in a directory listing (shared shape with local fs listings).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    /// Seconds since epoch (0 = unknown).
    pub mtime: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<u32>,
}

/// Progress / lifecycle events for one queued transfer.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransferEvent {
    #[serde(rename = "progress")]
    Progress {
        id: String,
        transferred: u64,
        total: u64,
    },
    #[serde(rename = "state")]
    State {
        id: String,
        /// queued | running | done | error | cancelled
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

#[derive(Clone, Default)]
pub struct SftpManager {
    sessions: Arc<Mutex<HashMap<String, Arc<SftpSession>>>>,
    transfers: Arc<Mutex<HashMap<String, AbortHandle>>>,
    watch: Arc<Mutex<Option<Channel<TransferEvent>>>>,
}

impl SftpManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn set_watch(&self, ch: Channel<TransferEvent>) {
        *self.watch.lock().await = Some(ch);
    }

    pub async fn emit(&self, ev: TransferEvent) {
        if let Some(ch) = self.watch.lock().await.as_ref() {
            let _ = ch.send(ev);
        }
    }

    pub async fn add(&self, id: String, session: SftpSession) {
        self.sessions.lock().await.insert(id, Arc::new(session));
    }

    pub async fn get(&self, id: &str) -> AppResult<Arc<SftpSession>> {
        self.sessions
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::SessionNotFound(format!("sftp {id}")))
    }

    pub async fn remove(&self, id: &str) {
        self.sessions.lock().await.remove(id);
    }

    pub async fn register_transfer(&self, id: String, abort: AbortHandle) {
        self.transfers.lock().await.insert(id, abort);
    }

    pub async fn unregister_transfer(&self, id: &str) {
        self.transfers.lock().await.remove(id);
    }

    pub async fn cancel(&self, id: &str) -> bool {
        if let Some(h) = self.transfers.lock().await.get(id) {
            h.abort();
            true
        } else {
            false
        }
    }
}

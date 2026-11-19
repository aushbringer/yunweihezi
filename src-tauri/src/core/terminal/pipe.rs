//! Session → frontend byte-stream pipe.
//!
//! Every interactive session (SSH / serial / local PTY / telnet) pushes its
//! output through a `SessionPipe` which forwards events to the frontend via a
//! Tauri IPC channel. Payload bytes are base64-encoded (JSON-safe); the
//! frontend decodes into `Uint8Array` and feeds xterm.js.

use base64::Engine;
use serde::Serialize;
use tauri::ipc::Channel;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OutputEvent {
    /// Terminal output bytes (base64).
    #[serde(rename = "data")]
    Data { data: String },
    /// Connection lifecycle state: connecting / connected / disconnected / closed / error.
    #[serde(rename = "state")]
    State {
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Remote process exit status (exec channels / local shell).
    #[serde(rename = "exit")]
    Exit { code: Option<u32> },
    /// Modem transfer lifecycle (serial): zrinit / start / fileStart /
    /// progress / fileDone / done / error.
    #[serde(rename = "modem")]
    Modem {
        event: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        transferred: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    /// A trigger rule fired on session output (highlight / alert actions are
    /// handled on the frontend; send / script are dispatched backend-side).
    #[serde(rename = "trigger")]
    Trigger {
        name: String,
        action: String,
        param: String,
        matched: String,
    },
}

#[derive(Clone)]
pub struct SessionPipe {
    /// Current IPC channel. Swappable so a detached window can re-attach to a
    /// running session and take over its output stream.
    channel: std::sync::Arc<std::sync::Mutex<Channel<OutputEvent>>>,
    /// Side taps for raw output bytes (audit logging, trigger engine...).
    taps: Vec<std::sync::Arc<dyn Fn(&[u8]) + Send + Sync>>,
}

impl SessionPipe {
    pub fn new(channel: Channel<OutputEvent>) -> Self {
        Self {
            channel: std::sync::Arc::new(std::sync::Mutex::new(channel)),
            taps: Vec::new(),
        }
    }

    /// Swap the output channel (detached-window re-attach).
    pub fn swap_channel(&self, new_channel: Channel<OutputEvent>) {
        if let Ok(mut guard) = self.channel.lock() {
            *guard = new_channel;
        }
    }

    fn send(&self, ev: OutputEvent) {
        if let Ok(ch) = self.channel.lock() {
            let _ = ch.send(ev);
        }
    }

    /// Attach a raw-byte tap (e.g. session audit logger, trigger engine).
    pub fn with_tap(mut self, tap: std::sync::Arc<dyn Fn(&[u8]) + Send + Sync>) -> Self {
        self.taps.push(tap);
        self
    }

    pub fn push_data(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        for tap in &self.taps {
            tap(bytes);
        }
        self.send(OutputEvent::Data {
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }

    pub fn push_state(&self, state: &str, message: Option<String>) {
        self.send(OutputEvent::State {
            state: state.to_string(),
            message,
        });
    }

    pub fn push_exit(&self, code: Option<u32>) {
        self.send(OutputEvent::Exit { code });
    }

    pub fn push_trigger(&self, name: &str, action: &str, param: &str, matched: &str) {
        self.send(OutputEvent::Trigger {
            name: name.to_string(),
            action: action.to_string(),
            param: param.to_string(),
            matched: matched.to_string(),
        });
    }

    pub fn push_modem(
        &self,
        event: &str,
        message: Option<String>,
        transferred: Option<u64>,
        total: Option<u64>,
    ) {
        self.send(OutputEvent::Modem {
            event: event.to_string(),
            message,
            transferred,
            total,
        });
    }
}

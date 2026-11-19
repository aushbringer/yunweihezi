//! Session abstraction shared by all interactive protocols.

pub mod manager;
pub mod reconnect;

/// Commands sent from the frontend/control plane into a running session task.
#[derive(Debug)]
pub enum SessionCommand {
    /// Raw input bytes (keyboard, paste, broadcast, quick commands).
    Input(Vec<u8>),
    /// Terminal size change.
    Resize { cols: u32, rows: u32 },
    /// Graceful close request.
    Close,
}

/// Session lifecycle states reported to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Connected,
    Disconnected,
    Closed,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Closed => "closed",
        }
    }
}

//! SessionManager: registry and lifecycle of all interactive sessions.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use russh::client::Msg;
use russh::{Channel, ChannelMsg};

use crate::core::local_shell::{self, LocalShellConfig};
use crate::core::rlogin::{self, RloginConfig};
use crate::core::serial::{self, SerialConfig, SerialHandle, SerialOp};
use crate::core::ssh::client::{self, ClientHandler, SshConnection, SshSessionConfig};
use crate::core::ssh::shell;
use crate::core::telnet::{self, TelnetConfig};
use crate::core::terminal::pipe::SessionPipe;
use crate::error::{AppError, AppResult};

use super::reconnect::ReconnectPolicy;
use super::{SessionCommand, SessionState};

struct SessionEntry {
    cmd_tx: mpsc::Sender<SessionCommand>,
    kind: &'static str,
    /// Output pipe (channel swappable for detached-window re-attach).
    pipe: SessionPipe,
    /// Live SSH connection handle (for SFTP / tunnels / SCP).
    ssh_handle: Option<Arc<russh::client::Handle<ClientHandler>>>,
    /// Remote-forward routing table of the SSH connection.
    router: Option<crate::core::ssh::forward::ForwardRouter>,
    /// Serial session control channel (modem transfers).
    serial: Option<SerialHandle>,
}

impl SessionEntry {
    fn plain(cmd_tx: mpsc::Sender<SessionCommand>, kind: &'static str, pipe: SessionPipe) -> Self {
        Self {
            cmd_tx,
            kind,
            pipe,
            ssh_handle: None,
            router: None,
            serial: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open an SSH session: connect, authenticate, spawn shell, start the
    /// reader/writer task. Returns the new session id.
    pub async fn open_ssh(&self, cfg: SshSessionConfig, pipe: SessionPipe) -> AppResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        pipe.push_state(SessionState::Connecting.as_str(), None);

        let conn = client::connect(&cfg).await.map_err(|e| {
            pipe.push_state("error", Some(e.to_string()));
            e
        })?;
        let channel = shell::open_shell(&conn.handle, &cfg.term, cfg.cols, cfg.rows, cfg.x11)
            .await
            .map_err(|e| {
                pipe.push_state("error", Some(e.to_string()));
                e
            })?;

        pipe.push_state(SessionState::Connected.as_str(), None);

        let (cmd_tx, cmd_rx) = mpsc::channel(128);
        self.sessions.lock().await.insert(
            id.clone(),
            SessionEntry {
                cmd_tx,
                kind: "ssh",
                pipe: pipe.clone(),
                ssh_handle: Some(conn.handle.clone()),
                router: Some(conn.router.clone()),
                serial: None,
            },
        );

        let sessions = self.sessions.clone();
        let sid = id.clone();
        let sessions2 = sessions.clone();
        let sid2 = sid.clone();
        tokio::spawn(async move {
            ssh_session_task(cfg, conn, channel, cmd_rx, pipe.clone(), sessions, sid).await;
            sessions2.lock().await.remove(&sid2);
            pipe.push_state(SessionState::Closed.as_str(), None);
        });

        Ok(id)
    }

    /// Open a local shell (portable-pty) session.
    pub async fn open_local(&self, cfg: LocalShellConfig, pipe: SessionPipe) -> AppResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let (cmd_tx, cmd_rx) = mpsc::channel(128);
        self.sessions
            .lock()
            .await
            .insert(id.clone(), SessionEntry::plain(cmd_tx, "local", pipe.clone()));

        let sessions = self.sessions.clone();
        let sid = id.clone();
        tokio::spawn(async move {
            if let Err(e) = local_shell::run_local_shell(cfg, pipe.clone(), cmd_rx).await {
                pipe.push_state("error", Some(e.to_string()));
            }
            sessions.lock().await.remove(&sid);
            pipe.push_state(SessionState::Closed.as_str(), None);
        });
        Ok(id)
    }

    /// Open a Telnet session.
    pub async fn open_telnet(&self, cfg: TelnetConfig, pipe: SessionPipe) -> AppResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let (cmd_tx, cmd_rx) = mpsc::channel(128);
        self.sessions
            .lock()
            .await
            .insert(id.clone(), SessionEntry::plain(cmd_tx, "telnet", pipe.clone()));

        let sessions = self.sessions.clone();
        let sid = id.clone();
        tokio::spawn(async move {
            match telnet::run_telnet(cfg, pipe.clone(), cmd_rx).await {
                Ok(()) => pipe.push_state(SessionState::Disconnected.as_str(), None),
                Err(e) => pipe.push_state("error", Some(e.to_string())),
            }
            sessions.lock().await.remove(&sid);
            pipe.push_state(SessionState::Closed.as_str(), None);
        });
        Ok(id)
    }

    /// Open an Rlogin session.
    pub async fn open_rlogin(&self, cfg: RloginConfig, pipe: SessionPipe) -> AppResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let (cmd_tx, cmd_rx) = mpsc::channel(128);
        self.sessions
            .lock()
            .await
            .insert(id.clone(), SessionEntry::plain(cmd_tx, "rlogin", pipe.clone()));

        let sessions = self.sessions.clone();
        let sid = id.clone();
        tokio::spawn(async move {
            match rlogin::run_rlogin(cfg, pipe.clone(), cmd_rx).await {
                Ok(()) => pipe.push_state(SessionState::Disconnected.as_str(), None),
                Err(e) => pipe.push_state("error", Some(e.to_string())),
            }
            sessions.lock().await.remove(&sid);
            pipe.push_state(SessionState::Closed.as_str(), None);
        });
        Ok(id)
    }

    /// Open a serial port session.
    pub async fn open_serial(&self, cfg: SerialConfig, pipe: SessionPipe) -> AppResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        pipe.push_state(SessionState::Connecting.as_str(), None);

        let port = serial::open_port(&cfg).map_err(|e| {
            pipe.push_state("error", Some(e.to_string()));
            e
        })?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCommand>(128);
        let (op_tx, op_rx) = std::sync::mpsc::channel::<SerialOp>();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        self.sessions.lock().await.insert(
            id.clone(),
            SessionEntry {
                cmd_tx,
                kind: "serial",
                pipe: pipe.clone(),
                ssh_handle: None,
                router: None,
                serial: Some(SerialHandle {
                    op_tx: op_tx.clone(),
                    cancel: cancel.clone(),
                }),
            },
        );

        pipe.push_state(SessionState::Connected.as_str(), None);

        // Blocking IO thread owns the port.
        {
            let pipe = pipe.clone();
            let cancel = cancel.clone();
            let op_tx2 = op_tx.clone();
            std::thread::spawn(move || {
                serial::serial_io_loop(port, cfg, pipe, op_rx, op_tx2, cancel);
            });
        }

        // Async bridge: SessionCommand → SerialOp.
        let sessions = self.sessions.clone();
        let sid = id.clone();
        tokio::spawn(async move {
            let mut cmd_rx = cmd_rx;
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    SessionCommand::Input(data) => {
                        let _ = op_tx.send(SerialOp::Input(data));
                    }
                    SessionCommand::Resize { .. } => {}
                    SessionCommand::Close => {
                        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        let _ = op_tx.send(SerialOp::Close);
                        break;
                    }
                }
            }
            sessions.lock().await.remove(&sid);
            pipe.push_state(SessionState::Closed.as_str(), None);
        });

        Ok(id)
    }

    /// Push a modem operation into a serial session.
    pub async fn serial_op(&self, session_id: &str, op: SerialOp) -> AppResult<()> {
        let sessions = self.sessions.lock().await;
        let entry = sessions
            .get(session_id)
            .ok_or_else(|| AppError::SessionNotFound(session_id.to_string()))?;
        let handle = entry
            .serial
            .as_ref()
            .ok_or_else(|| AppError::Config("not a serial session".into()))?;
        handle
            .op_tx
            .send(op)
            .map_err(|e| AppError::Config(format!("serial op: {e}")))?;
        Ok(())
    }

    pub async fn input(&self, session_id: &str, data: Vec<u8>) -> AppResult<()> {
        self.send(session_id, SessionCommand::Input(data)).await
    }

    /// Re-point a running session's output at a new IPC channel (detached
    /// window takes over the stream).
    pub async fn reattach(
        &self,
        session_id: &str,
        new_channel: tauri::ipc::Channel<crate::core::terminal::pipe::OutputEvent>,
    ) -> AppResult<()> {
        let sessions = self.sessions.lock().await;
        let entry = sessions
            .get(session_id)
            .ok_or_else(|| AppError::SessionNotFound(session_id.to_string()))?;
        entry.pipe.swap_channel(new_channel);
        entry.pipe.push_state(SessionState::Connected.as_str(), None);
        Ok(())
    }

    pub async fn resize(&self, session_id: &str, cols: u32, rows: u32) -> AppResult<()> {
        self.send(session_id, SessionCommand::Resize { cols, rows })
            .await
    }

    pub async fn close(&self, session_id: &str) -> AppResult<()> {
        self.send(session_id, SessionCommand::Close).await
    }

    pub async fn list(&self) -> Vec<(String, &'static str)> {
        self.sessions
            .lock()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.kind))
            .collect()
    }

    /// Clone the shared SSH handle of a session (for SFTP/SCP/tunnels).
    pub async fn ssh_handle(
        &self,
        session_id: &str,
    ) -> AppResult<Arc<russh::client::Handle<ClientHandler>>> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(session_id)
            .and_then(|e| e.ssh_handle.clone())
            .ok_or_else(|| AppError::SessionNotFound(session_id.to_string()))
    }

    /// Clone the shared SSH handle + forward router (for tunnels).
    pub async fn ssh_link(
        &self,
        session_id: &str,
    ) -> AppResult<(
        Arc<russh::client::Handle<ClientHandler>>,
        crate::core::ssh::forward::ForwardRouter,
    )> {
        let sessions = self.sessions.lock().await;
        let entry = sessions
            .get(session_id)
            .ok_or_else(|| AppError::SessionNotFound(session_id.to_string()))?;
        let handle = entry
            .ssh_handle
            .clone()
            .ok_or_else(|| AppError::Config("not an SSH session".into()))?;
        let router = entry
            .router
            .clone()
            .ok_or_else(|| AppError::Config("not an SSH session".into()))?;
        Ok((handle, router))
    }

    async fn send(&self, session_id: &str, cmd: SessionCommand) -> AppResult<()> {
        let sessions = self.sessions.lock().await;
        let entry = sessions
            .get(session_id)
            .ok_or_else(|| AppError::SessionNotFound(session_id.to_string()))?;
        entry
            .cmd_tx
            .try_send(cmd)
            .map_err(|e| AppError::Config(format!("session busy or closed: {e}")))?;
        Ok(())
    }
}

/// Why the channel loop ended.
enum ChannelEnd {
    /// Frontend asked us to close.
    Requested,
    /// Remote side closed / connection lost.
    Disconnected,
}

/// Full lifecycle of one SSH session, including auto-reconnect.
async fn ssh_session_task(
    cfg: SshSessionConfig,
    conn: SshConnection,
    channel: Channel<Msg>,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    pipe: SessionPipe,
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
    sid: String,
) {
    let policy = ReconnectPolicy::default();
    let mut conn = conn;
    let mut channel = channel;
    let (mut cols, mut rows) = (cfg.cols, cfg.rows);

    'outer: loop {
        match run_ssh_channel(&mut channel, &mut cmd_rx, &pipe, &mut cols, &mut rows).await {
            ChannelEnd::Requested => break 'outer,
            ChannelEnd::Disconnected => {}
        }

        if !cfg.reconnect {
            pipe.push_state(SessionState::Disconnected.as_str(), None);
            break 'outer;
        }

        // Backoff reconnect loop.
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let Some(delay) = policy.delay_for(attempt) else {
                pipe.push_state(
                    SessionState::Disconnected.as_str(),
                    Some("reconnect attempts exhausted".into()),
                );
                break 'outer;
            };
            pipe.push_data(
                format!("\r\n\x1b[33m[connection lost, reconnecting in {delay:?} ({attempt}/{})]\x1b[0m\r\n", policy.max_attempts)
                    .as_bytes(),
            );
            // Wait for the delay, but let the user abort via Close.
            let aborted = tokio::select! {
                _ = tokio::time::sleep(delay) => false,
                cmd = cmd_rx.recv() => !matches!(cmd, Some(SessionCommand::Input(_))),
            };
            if aborted {
                break 'outer;
            }
            match client::connect(&cfg).await {
                Ok(new_conn) => {
                    match shell::open_shell(&new_conn.handle, &cfg.term, cols, rows, cfg.x11).await {
                        Ok(new_channel) => {
                            pipe.push_data(b"\x1b[32m[reconnected]\x1b[0m\r\n");
                            pipe.push_state(SessionState::Connected.as_str(), None);
                            // Refresh the shared handle so SFTP/tunnels use the
                            // new connection.
                            if let Some(entry) = sessions.lock().await.get_mut(&sid) {
                                entry.ssh_handle = Some(new_conn.handle.clone());
                                entry.router = Some(new_conn.router.clone());
                            }
                            conn = new_conn;
                            channel = new_channel;
                            continue 'outer;
                        }
                        Err(e) => {
                            pipe.push_data(
                                format!("\x1b[31m[shell reopen failed: {e}]\x1b[0m\r\n").as_bytes(),
                            );
                        }
                    }
                }
                Err(e) => {
                    pipe.push_data(format!("\x1b[31m[reconnect failed: {e}]\x1b[0m\r\n").as_bytes());
                }
            }
        }
    }

    drop(conn);
}

/// Reader/writer loop for an interactive SSH shell channel.
async fn run_ssh_channel(
    channel: &mut Channel<Msg>,
    cmd_rx: &mut mpsc::Receiver<SessionCommand>,
    pipe: &SessionPipe,
    cols: &mut u32,
    rows: &mut u32,
) -> ChannelEnd {
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => pipe.push_data(&data),
                    Some(ChannelMsg::ExtendedData { data, .. }) => pipe.push_data(&data),
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        pipe.push_exit(Some(exit_status));
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        return ChannelEnd::Disconnected;
                    }
                    _ => {}
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCommand::Input(data)) => {
                        if channel.data(&data[..]).await.is_err() {
                            return ChannelEnd::Disconnected;
                        }
                    }
                    Some(SessionCommand::Resize { cols: c, rows: r }) => {
                        *cols = c;
                        *rows = r;
                        let _ = channel.window_change(c, r, 0, 0).await;
                    }
                    Some(SessionCommand::Close) | None => {
                        let _ = channel.close().await;
                        return ChannelEnd::Requested;
                    }
                }
            }
        }
    }
}

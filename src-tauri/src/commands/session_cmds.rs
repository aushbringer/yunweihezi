//! Tauri commands for interactive session lifecycle.

use base64::Engine;
use tauri::ipc::Channel;
use tauri::State;

use crate::core::local_shell::LocalShellConfig;
use crate::core::logging::{self, LogVars};
use crate::core::rlogin::RloginConfig;
use crate::core::serial::xymodem::XyVariant;
use crate::core::serial::{SerialConfig, SerialOp, SerialPortInfo};
use crate::core::session::manager::SessionManager;
use crate::core::ssh::client::SshSessionConfig;
use crate::core::telnet::TelnetConfig;
use crate::core::terminal::pipe::{OutputEvent, SessionPipe};
use crate::core::terminal::trigger::{self, TriggerEngine, TriggerFire};
use crate::state::AppState;

/// Attach the audit logger to the pipe when logging is enabled.
async fn with_logging(state: &AppState, pipe: SessionPipe, vars: LogVars) -> SessionPipe {
    match logging::start_session_logger(&state.db, &state.data_dir, &vars).await {
        Some(logger) => {
            let tap = std::sync::Arc::new(move |b: &[u8]| logger.write(b));
            pipe.with_tap(tap)
        }
        None => pipe,
    }
}

/// Load trigger rules (global + this session record), build the engine and
/// attach its scanner to the pipe. Returns the fire receiver to dispatch.
async fn with_triggers(
    state: &AppState,
    pipe: SessionPipe,
    record_id: Option<&str>,
) -> (SessionPipe, Option<tokio::sync::mpsc::UnboundedReceiver<TriggerFire>>) {
    let rules = state.db.trigger_list().await.unwrap_or_default();
    let rules: Vec<_> = rules
        .into_iter()
        .filter(|r| r.session_id.is_none() || r.session_id.as_deref() == record_id)
        .collect();
    if rules.is_empty() {
        return (pipe, None);
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    match TriggerEngine::build(&rules, tx) {
        Some(engine) => {
            let tap = std::sync::Arc::new(move |b: &[u8]| engine.feed(b));
            (pipe.with_tap(tap), Some(rx))
        }
        None => (pipe, None),
    }
}

/// Dispatch fired trigger actions: `send` writes back into the session,
/// `script` runs locally, `highlight` / `alert` go to the frontend.
fn spawn_trigger_dispatch(
    sessions: SessionManager,
    sid: String,
    pipe: SessionPipe,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TriggerFire>,
) {
    tokio::spawn(async move {
        while let Some(fire) = rx.recv().await {
            match fire.action.as_str() {
                "send" => {
                    let mut data = trigger::expand_template(&fire.param, &fire).into_bytes();
                    data.push(b'\r');
                    let _ = sessions.input(&sid, data).await;
                }
                "script" => {
                    let pipe = pipe.clone();
                    tokio::task::spawn_blocking(move || {
                        crate::core::script::run_trigger_script(&pipe, &fire);
                    });
                }
                _ => pipe.push_trigger(&fire.name, &fire.action, &fire.param, &fire.matched),
            }
        }
    });
}

/// Test an SSH connection with the given (unsaved) config: connect +
/// authenticate, then immediately drop. Returns a human-readable summary.
#[tauri::command]
pub async fn ssh_test_connect(config: SshSessionConfig) -> Result<String, String> {
    let start = std::time::Instant::now();
    let attempt = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        crate::core::ssh::client::connect(&config),
    )
    .await;
    match attempt {
        Ok(Ok(conn)) => {
            drop(conn);
            Ok(format!("连接成功,耗时 {} ms", start.elapsed().as_millis()))
        }
        Ok(Err(e)) => Err(format!("连接失败: {e}")),
        Err(_) => Err("连接超时(10 秒)".into()),
    }
}

/// Open a new SSH session. Output streams back through `on_output`.
#[tauri::command]
pub async fn session_open(
    state: State<'_, AppState>,
    sessions: State<'_, SessionManager>,
    config: SshSessionConfig,
    label: Option<String>,
    record_id: Option<String>,
    on_output: Channel<OutputEvent>,
) -> Result<String, String> {
    let vars = LogVars {
        host: config.host.clone(),
        user: config.username.clone(),
        name: label.unwrap_or_else(|| format!("{}@{}", config.username, config.host)),
        protocol: "ssh".into(),
    };
    let pipe = with_logging(&state, SessionPipe::new(on_output), vars).await;
    let (pipe, rx) = with_triggers(&state, pipe, record_id.as_deref()).await;
    let sid = sessions
        .open_ssh(config, pipe.clone())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(rx) = rx {
        spawn_trigger_dispatch(sessions.inner().clone(), sid.clone(), pipe, rx);
    }
    Ok(sid)
}

/// Open a local shell session (CMD / PowerShell / WSL / bash).
#[tauri::command]
pub async fn local_shell_open(
    state: State<'_, AppState>,
    sessions: State<'_, SessionManager>,
    config: LocalShellConfig,
    label: Option<String>,
    record_id: Option<String>,
    on_output: Channel<OutputEvent>,
) -> Result<String, String> {
    let vars = LogVars {
        host: "localhost".into(),
        user: std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_default(),
        name: label.unwrap_or_else(|| format!("local-{}", config.shell)),
        protocol: "local".into(),
    };
    let pipe = with_logging(&state, SessionPipe::new(on_output), vars).await;
    let (pipe, rx) = with_triggers(&state, pipe, record_id.as_deref()).await;
    let sid = sessions
        .open_local(config, pipe.clone())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(rx) = rx {
        spawn_trigger_dispatch(sessions.inner().clone(), sid.clone(), pipe, rx);
    }
    Ok(sid)
}

/// Open a Telnet session.
#[tauri::command]
pub async fn telnet_open(
    state: State<'_, AppState>,
    sessions: State<'_, SessionManager>,
    config: TelnetConfig,
    label: Option<String>,
    record_id: Option<String>,
    on_output: Channel<OutputEvent>,
) -> Result<String, String> {
    let vars = LogVars {
        host: config.host.clone(),
        user: String::new(),
        name: label.unwrap_or_else(|| config.host.clone()),
        protocol: "telnet".into(),
    };
    let pipe = with_logging(&state, SessionPipe::new(on_output), vars).await;
    let (pipe, rx) = with_triggers(&state, pipe, record_id.as_deref()).await;
    let sid = sessions
        .open_telnet(config, pipe.clone())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(rx) = rx {
        spawn_trigger_dispatch(sessions.inner().clone(), sid.clone(), pipe, rx);
    }
    Ok(sid)
}

/// Open an Rlogin session.
#[tauri::command]
pub async fn rlogin_open(
    state: State<'_, AppState>,
    sessions: State<'_, SessionManager>,
    config: RloginConfig,
    label: Option<String>,
    record_id: Option<String>,
    on_output: Channel<OutputEvent>,
) -> Result<String, String> {
    let vars = LogVars {
        host: config.host.clone(),
        user: config.username.clone(),
        name: label.unwrap_or_else(|| config.host.clone()),
        protocol: "rlogin".into(),
    };
    let pipe = with_logging(&state, SessionPipe::new(on_output), vars).await;
    let (pipe, rx) = with_triggers(&state, pipe, record_id.as_deref()).await;
    let sid = sessions
        .open_rlogin(config, pipe.clone())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(rx) = rx {
        spawn_trigger_dispatch(sessions.inner().clone(), sid.clone(), pipe, rx);
    }
    Ok(sid)
}

/// Open a serial port session.
#[tauri::command]
pub async fn serial_open(
    state: State<'_, AppState>,
    sessions: State<'_, SessionManager>,
    config: SerialConfig,
    label: Option<String>,
    record_id: Option<String>,
    on_output: Channel<OutputEvent>,
) -> Result<String, String> {
    let vars = LogVars {
        host: config.port.clone(),
        user: String::new(),
        name: label.unwrap_or_else(|| config.port.clone()),
        protocol: "serial".into(),
    };
    let pipe = with_logging(&state, SessionPipe::new(on_output), vars).await;
    let (pipe, rx) = with_triggers(&state, pipe, record_id.as_deref()).await;
    let sid = sessions
        .open_serial(config, pipe.clone())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(rx) = rx {
        spawn_trigger_dispatch(sessions.inner().clone(), sid.clone(), pipe, rx);
    }
    Ok(sid)
}

/// List available serial ports.
#[tauri::command]
pub fn serial_list_ports() -> Vec<SerialPortInfo> {
    crate::core::serial::list_ports()
}

/// Send files over ZModem (remote must be running `rz`).
#[tauri::command]
pub async fn serial_zsend(
    sessions: State<'_, SessionManager>,
    session_id: String,
    paths: Vec<String>,
) -> Result<(), String> {
    sessions
        .serial_op(&session_id, SerialOp::ZSend(paths))
        .await
        .map_err(|e| e.to_string())
}

/// Send one file over XModem / YModem.
#[tauri::command]
pub async fn serial_xsend(
    sessions: State<'_, SessionManager>,
    session_id: String,
    path: String,
    variant: XyVariant,
) -> Result<(), String> {
    sessions
        .serial_op(&session_id, SerialOp::XSend { path, variant })
        .await
        .map_err(|e| e.to_string())
}

/// Receive over XModem / YModem into the download directory.
#[tauri::command]
pub async fn serial_xrecv(
    sessions: State<'_, SessionManager>,
    session_id: String,
    variant: XyVariant,
) -> Result<(), String> {
    sessions
        .serial_op(&session_id, SerialOp::XRecv { variant })
        .await
        .map_err(|e| e.to_string())
}

/// Re-attach a running session's output stream to a new channel (used by a
/// detached terminal window taking over the session).
#[tauri::command]
pub async fn session_attach(
    sessions: State<'_, SessionManager>,
    session_id: String,
    on_output: Channel<OutputEvent>,
) -> Result<(), String> {
    sessions
        .reattach(&session_id, on_output)
        .await
        .map_err(|e| e.to_string())
}

/// Send keyboard/paste input (base64) to a session.
#[tauri::command]
pub async fn session_input(
    sessions: State<'_, SessionManager>,
    session_id: String,
    data_b64: String,
) -> Result<(), String> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|e| e.to_string())?;
    sessions
        .input(&session_id, data)
        .await
        .map_err(|e| e.to_string())
}

/// Notify terminal resize.
#[tauri::command]
pub async fn session_resize(
    sessions: State<'_, SessionManager>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    sessions
        .resize(&session_id, cols, rows)
        .await
        .map_err(|e| e.to_string())
}

/// Close a session.
#[tauri::command]
pub async fn session_close(
    sessions: State<'_, SessionManager>,
    session_id: String,
) -> Result<(), String> {
    sessions
        .close(&session_id)
        .await
        .map_err(|e| e.to_string())
}

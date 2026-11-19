//! Local shell sessions (CMD / PowerShell / WSL / bash) via portable-pty.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{mpsc, Notify};

use crate::core::session::{SessionCommand, SessionState};
use crate::core::terminal::pipe::SessionPipe;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalShellConfig {
    /// cmd | powershell | pwsh | wsl | bash | absolute path to an executable.
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
}

#[cfg(windows)]
fn default_shell() -> String {
    "powershell".into()
}
#[cfg(not(windows))]
fn default_shell() -> String {
    "bash".into()
}
fn default_cols() -> u16 {
    80
}
fn default_rows() -> u16 {
    24
}

/// Resolve a friendly shell name to program + args.
fn resolve_shell(shell: &str) -> (String, Vec<String>) {
    let s = shell.to_ascii_lowercase();
    match s.as_str() {
        "cmd" => ("cmd.exe".into(), vec![]),
        "powershell" => ("powershell.exe".into(), vec!["-NoLogo".into()]),
        "pwsh" => ("pwsh.exe".into(), vec!["-NoLogo".into()]),
        "wsl" => ("wsl.exe".into(), vec![]),
        "bash" => {
            if cfg!(windows) {
                // Prefer Git bash if present, else WSL bash.
                ("bash.exe".into(), vec!["-i".into()])
            } else {
                ("/bin/bash".into(), vec!["-i".into()])
            }
        }
        "sh" => ("/bin/sh".into(), vec!["-i".into()]),
        // Treat anything else as a literal executable path.
        _ => (shell.to_string(), vec![]),
    }
}

/// Spawn the local shell and drive it until the child exits or Close arrives.
pub async fn run_local_shell(
    cfg: LocalShellConfig,
    pipe: SessionPipe,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
) -> AppResult<()> {
    pipe.push_state(SessionState::Connecting.as_str(), None);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: cfg.rows,
            cols: cfg.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| AppError::Config(format!("openpty: {e}")))?;

    let (program, args) = resolve_shell(&cfg.shell);
    let mut cmd = CommandBuilder::new(&program);
    for a in &args {
        cmd.arg(a);
    }
    if let Some(cwd) = &cfg.cwd {
        if !cwd.is_empty() {
            cmd.cwd(cwd);
        }
    }
    cmd.env("TERM", "xterm-256color");

    let mut child: Box<dyn Child + Send + Sync> = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| AppError::Config(format!("spawn {program}: {e}")))?;
    drop(pair.slave);

    let master: Box<dyn MasterPty + Send> = pair.master;
    let mut reader = master
        .try_clone_reader()
        .map_err(|e| AppError::Config(format!("pty reader: {e}")))?;
    let writer = master
        .take_writer()
        .map_err(|e| AppError::Config(format!("pty writer: {e}")))?;
    let writer = Arc::new(Mutex::new(writer));
    let killer = child.clone_killer();

    pipe.push_state(SessionState::Connected.as_str(), None);

    // Signals that the child process is gone (reader EOF or wait()).
    let done = Arc::new(Notify::new());

    // Reader thread: PTY output → frontend.
    {
        let pipe = pipe.clone();
        let done = done.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => pipe.push_data(&buf[..n]),
                    Err(_) => break,
                }
            }
            done.notify_one();
        });
    }

    // Waiter thread: exit status → frontend.
    {
        let pipe = pipe.clone();
        let done = done.clone();
        std::thread::spawn(move || {
            if let Ok(status) = child.wait() {
                pipe.push_exit(status.exit_code().into());
            }
            done.notify_one();
        });
    }

    // Command loop (async side).
    loop {
        tokio::select! {
            _ = done.notified() => break,
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCommand::Input(data)) => {
                        let ok = {
                            let mut w = writer.lock().unwrap();
                            w.write_all(&data).and_then(|_| w.flush()).is_ok()
                        };
                        if !ok {
                            break;
                        }
                    }
                    Some(SessionCommand::Resize { cols, rows }) => {
                        let _ = master.resize(PtySize {
                            rows: rows as u16,
                            cols: cols as u16,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                    Some(SessionCommand::Close) | None => {
                        let mut k = killer;
                        let _ = k.kill();
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

//! Port-forwarding commands: start/stop live tunnels and CRUD for saved
//! tunnel definitions.

use tauri::State;
use tokio::net::TcpListener;

use crate::core::db::models::TunnelRecord;
use crate::core::session::manager::SessionManager;
use crate::core::ssh::forward::{dynamic, local, remote, RunningTunnel, TunnelManager};
use crate::state::AppState;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStartArgs {
    pub session_id: String,
    /// local | remote | dynamic
    pub kind: String,
    pub bind_host: String,
    pub bind_port: u16,
    #[serde(default)]
    pub target_host: String,
    #[serde(default)]
    pub target_port: u16,
}

/// Start a tunnel on an open SSH session. Returns the running tunnel.
#[tauri::command]
pub async fn tunnel_start(
    tunnels: State<'_, TunnelManager>,
    sessions: State<'_, SessionManager>,
    args: TunnelStartArgs,
) -> Result<RunningTunnel, String> {
    let (handle, router) = sessions
        .ssh_link(&args.session_id)
        .await
        .map_err(|e| e.to_string())?;

    let info = RunningTunnel {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: args.session_id.clone(),
        kind: args.kind.clone(),
        bind_host: args.bind_host.clone(),
        bind_port: args.bind_port,
        target_host: args.target_host.clone(),
        target_port: args.target_port,
    };

    match args.kind.as_str() {
        "local" | "dynamic" => {
            let listener = TcpListener::bind((args.bind_host.as_str(), args.bind_port))
                .await
                .map_err(|e| format!("bind {}:{}: {e}", args.bind_host, args.bind_port))?;
            let stop = tunnels.register(info.clone()).await;
            let tm = tunnels.inner().clone();
            let id = info.id.clone();
            if args.kind == "local" {
                tokio::spawn(async move {
                    let r = local::run(handle, listener, args.target_host, args.target_port, stop).await;
                    if let Err(e) = r {
                        tracing::warn!("local tunnel {id}: {e}");
                    }
                    tm.unregister(&id).await;
                });
            } else {
                tokio::spawn(async move {
                    let r = dynamic::run(handle, listener, stop).await;
                    if let Err(e) = r {
                        tracing::warn!("dynamic tunnel {id}: {e}");
                    }
                    tm.unregister(&id).await;
                });
            }
        }
        "remote" => {
            let stop = tunnels.register(info.clone()).await;
            let tm = tunnels.inner().clone();
            let id = info.id.clone();
            let bind_host = args.bind_host.clone();
            tokio::spawn(async move {
                let r = remote::run(
                    handle,
                    router,
                    &bind_host,
                    args.bind_port,
                    &args.target_host,
                    args.target_port,
                    stop,
                )
                .await;
                if let Err(e) = r {
                    tracing::warn!("remote tunnel {id}: {e}");
                }
                tm.unregister(&id).await;
            });
        }
        other => return Err(format!("unknown tunnel kind: {other}")),
    }

    Ok(info)
}

/// Stop a running tunnel.
#[tauri::command]
pub async fn tunnel_stop(tunnels: State<'_, TunnelManager>, id: String) -> Result<(), String> {
    tunnels.stop(&id).await.map_err(|e| e.to_string())
}

/// List running tunnels (optionally filtered by session).
#[tauri::command]
pub async fn tunnel_running(
    tunnels: State<'_, TunnelManager>,
    session_id: Option<String>,
) -> Result<Vec<RunningTunnel>, String> {
    let all = tunnels.list().await;
    Ok(match session_id {
        Some(sid) => all.into_iter().filter(|t| t.session_id == sid).collect(),
        None => all,
    })
}

/// List saved tunnel definitions.
#[tauri::command]
pub async fn tunnel_saved_list(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<Vec<TunnelRecord>, String> {
    state
        .db
        .tunnel_list(session_id)
        .await
        .map_err(|e| e.to_string())
}

/// Create or update a saved tunnel definition.
#[tauri::command]
pub async fn tunnel_saved_upsert(
    state: State<'_, AppState>,
    mut record: TunnelRecord,
) -> Result<TunnelRecord, String> {
    if record.id.is_empty() {
        record.id = uuid::Uuid::new_v4().to_string();
    }
    state
        .db
        .tunnel_upsert(&record)
        .await
        .map_err(|e| e.to_string())?;
    Ok(record)
}

/// Delete a saved tunnel definition.
#[tauri::command]
pub async fn tunnel_saved_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.tunnel_delete(&id).await.map_err(|e| e.to_string())
}

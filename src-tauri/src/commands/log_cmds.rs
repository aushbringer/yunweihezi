//! Audit log viewer commands.

use base64::Engine;
use tauri::{AppHandle, State};

use crate::core::db::models::LogFileRecord;
use crate::state::AppState;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogListItem {
    #[serde(flatten)]
    pub record: LogFileRecord,
    /// Whether the file still exists on disk.
    pub exists: bool,
}

#[tauri::command]
pub async fn log_list(state: State<'_, AppState>) -> Result<Vec<LogListItem>, String> {
    let rows = state.db.log_list().await.map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| LogListItem {
            exists: std::path::Path::new(&r.path).exists(),
            record: r,
        })
        .collect())
}

/// Delete a log record; optionally delete the file on disk too.
#[tauri::command]
pub async fn log_delete(
    state: State<'_, AppState>,
    id: String,
    delete_file: bool,
) -> Result<(), String> {
    if delete_file {
        let rows = state.db.log_list().await.map_err(|e| e.to_string())?;
        if let Some(r) = rows.iter().find(|r| r.id == id) {
            let _ = std::fs::remove_file(&r.path);
        }
    }
    state.db.log_delete(&id).await.map_err(|e| e.to_string())
}

/// Read the tail of a log file (base64, may contain escape sequences).
#[tauri::command]
pub async fn log_tail(path: String, max_bytes: Option<u64>) -> Result<String, String> {
    let max = max_bytes.unwrap_or(64 * 1024) as usize;
    let data = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
    let start = data.len().saturating_sub(max);
    Ok(base64::engine::general_purpose::STANDARD.encode(&data[start..]))
}

/// Open the containing folder of a log file in the system file manager.
#[tauri::command]
pub fn log_open_folder(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = std::path::Path::new(&path);
    let dir = p.parent().unwrap_or(std::path::Path::new("."));
    app.opener()
        .open_path(dir.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())
}

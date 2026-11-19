//! Local filesystem commands for the SFTP dual-pane (local side).

use tauri::State;

use crate::core::sftp::FileEntry;
use crate::state::AppState;

fn to_entry(name: String, path: String, m: &std::fs::Metadata) -> FileEntry {
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    FileEntry {
        name,
        path,
        is_dir: m.is_dir(),
        is_symlink: m.file_type().is_symlink(),
        size: m.len(),
        mtime,
        permissions: None,
    }
}

#[tauri::command]
pub async fn fs_list(_state: State<'_, AppState>, path: String) -> Result<Vec<FileEntry>, String> {
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(&path).await.map_err(|e| e.to_string())?;
    while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path().to_string_lossy().to_string();
        let meta = entry.metadata().await.map_err(|e| e.to_string())?;
        out.push(to_entry(name, path, &meta));
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// User home directory (fallback: current dir).
#[tauri::command]
pub async fn fs_home() -> Result<String, String> {
    if let Ok(home) = std::env::var("USERPROFILE") {
        return Ok(home);
    }
    if let Ok(home) = std::env::var("HOME") {
        return Ok(home);
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fs_mkdir(path: String) -> Result<(), String> {
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fs_delete(path: String, is_dir: bool) -> Result<(), String> {
    if is_dir {
        tokio::fs::remove_dir_all(&path).await
    } else {
        tokio::fs::remove_file(&path).await
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fs_rename(old_path: String, new_path: String) -> Result<(), String> {
    tokio::fs::rename(&old_path, &new_path)
        .await
        .map_err(|e| e.to_string())
}

/// Read a UTF-8 text file (used by config import/export).
#[tauri::command]
pub async fn text_file_read(path: String) -> Result<String, String> {
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())
}

/// Write a UTF-8 text file (used by config import/export).
#[tauri::command]
pub async fn text_file_write(path: String, content: String) -> Result<(), String> {
    tokio::fs::write(&path, content)
        .await
        .map_err(|e| e.to_string())
}

/// Roots for the local pane breadcrumb (drive letters on Windows, "/" else).
#[tauri::command]
pub async fn fs_roots() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        let mut roots = Vec::new();
        for letter in b'A'..=b'Z' {
            let drive = format!("{}:\\", letter as char);
            if std::path::Path::new(&drive).exists() {
                roots.push(drive);
            }
        }
        return Ok(roots);
    }
    #[cfg(not(windows))]
    Ok(vec!["/".to_string()])
}

//! Plain-FTP client commands.

use std::path::PathBuf;

use serde::Deserialize;
use tauri::State;

use crate::core::ftp::FtpManager;
use crate::core::sftp::FileEntry;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtpConfig {
    pub host: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Connect + login; empty username means anonymous. Returns the ftp id.
#[tauri::command]
pub async fn ftp_open(ftp: State<'_, FtpManager>, config: FtpConfig) -> Result<String, String> {
    ftp.open(
        &config.host,
        config.port.unwrap_or(21),
        config.username.as_deref(),
        config.password.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_close(ftp: State<'_, FtpManager>, ftp_id: String) -> Result<(), String> {
    ftp.close(&ftp_id).await;
    Ok(())
}

#[tauri::command]
pub async fn ftp_list(
    ftp: State<'_, FtpManager>,
    ftp_id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    ftp.list(&ftp_id, &path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_mkdir(
    ftp: State<'_, FtpManager>,
    ftp_id: String,
    path: String,
) -> Result<(), String> {
    ftp.mkdir(&ftp_id, &path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_delete(
    ftp: State<'_, FtpManager>,
    ftp_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    ftp.delete(&ftp_id, &path, is_dir)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_rename(
    ftp: State<'_, FtpManager>,
    ftp_id: String,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    ftp.rename(&ftp_id, &old_path, &new_path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_download(
    ftp: State<'_, FtpManager>,
    ftp_id: String,
    remote: String,
    local: String,
) -> Result<(), String> {
    ftp.download(&ftp_id, &remote, &PathBuf::from(local))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn ftp_upload(
    ftp: State<'_, FtpManager>,
    ftp_id: String,
    local: String,
    remote: String,
) -> Result<(), String> {
    ftp.upload(&ftp_id, PathBuf::from(local), &remote)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

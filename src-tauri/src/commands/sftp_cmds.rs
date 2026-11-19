//! SFTP / SCP / transfer-queue commands.

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::State;

use crate::core::db::new_id;
use crate::core::session::manager::SessionManager;
use crate::core::sftp::{FileEntry, SftpManager, TransferEvent};
use crate::core::ssh::{scp, shell};
use crate::error::AppError;

/// Open (or reuse) an SFTP subsystem on a live SSH session.
#[tauri::command]
pub async fn sftp_open(
    sessions: State<'_, SessionManager>,
    sftp: State<'_, SftpManager>,
    session_id: String,
) -> Result<String, String> {
    let handle = sessions.ssh_handle(&session_id).await.map_err(|e| e.to_string())?;
    let session = shell::open_sftp(&handle).await.map_err(|e| e.to_string())?;
    let id = new_id();
    sftp.add(id.clone(), session).await;
    Ok(id)
}

#[tauri::command]
pub async fn sftp_close(sftp: State<'_, SftpManager>, sftp_id: String) -> Result<(), String> {
    sftp.remove(&sftp_id).await;
    Ok(())
}

/// Canonical remote home directory ("." resolved).
#[tauri::command]
pub async fn sftp_home(sftp: State<'_, SftpManager>, sftp_id: String) -> Result<String, String> {
    let s = sftp.get(&sftp_id).await.map_err(|e| e.to_string())?;
    s.canonicalize(".")
        .await
        .map_err(|e| AppError::Sftp(e.to_string()).to_string())
}

/// Download a remote file, open it in the local default editor and upload it
/// back automatically when the editor saves. Returns the local temp path.
#[tauri::command]
pub async fn external_edit_open(
    app: tauri::AppHandle,
    sftp: State<'_, SftpManager>,
    sftp_id: String,
    remote_path: String,
) -> Result<String, String> {
    crate::core::external_editor::open_external(app, sftp.inner().clone(), &sftp_id, &remote_path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_list(
    sftp: State<'_, SftpManager>,
    sftp_id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let s = sftp.get(&sftp_id).await.map_err(|e| e.to_string())?;
    let rd = s
        .read_dir(&path)
        .await
        .map_err(|e| AppError::Sftp(e.to_string()).to_string())?;
    let mut out: Vec<FileEntry> = rd
        .map(|e| {
            let m = e.metadata();
            FileEntry {
                name: e.file_name(),
                path: e.path(),
                is_dir: m.is_dir(),
                is_symlink: m.is_symlink(),
                size: m.size.unwrap_or(0),
                mtime: m.mtime.unwrap_or(0) as i64,
                permissions: m.permissions,
            }
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

#[tauri::command]
pub async fn sftp_mkdir(
    sftp: State<'_, SftpManager>,
    sftp_id: String,
    path: String,
) -> Result<(), String> {
    let s = sftp.get(&sftp_id).await.map_err(|e| e.to_string())?;
    s.create_dir(&path)
        .await
        .map_err(|e| AppError::Sftp(e.to_string()).to_string())
}

#[tauri::command]
pub async fn sftp_delete(
    sftp: State<'_, SftpManager>,
    sftp_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    let s = sftp.get(&sftp_id).await.map_err(|e| e.to_string())?;
    let r = if is_dir {
        s.remove_dir(&path).await
    } else {
        s.remove_file(&path).await
    };
    r.map_err(|e| AppError::Sftp(e.to_string()).to_string())
}

#[tauri::command]
pub async fn sftp_rename(
    sftp: State<'_, SftpManager>,
    sftp_id: String,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    let s = sftp.get(&sftp_id).await.map_err(|e| e.to_string())?;
    s.rename(&old_path, &new_path)
        .await
        .map_err(|e| AppError::Sftp(e.to_string()).to_string())
}

/// Register the single progress channel for all transfers.
#[tauri::command]
pub async fn sftp_watch(
    sftp: State<'_, SftpManager>,
    on_event: Channel<TransferEvent>,
) -> Result<(), String> {
    sftp.set_watch(on_event).await;
    Ok(())
}

#[tauri::command]
pub async fn sftp_download(
    sftp: State<'_, SftpManager>,
    sftp_id: String,
    remote: String,
    local: String,
    #[allow(unused)] resume: Option<bool>,
) -> Result<String, String> {
    let s = sftp.get(&sftp_id).await.map_err(|e| e.to_string())?;
    let id = new_id();
    let mgr = sftp.inner().clone();
    let tid = id.clone();
    let task = tokio::spawn(async move {
        if let Err(e) = crate::core::sftp::transfer::download(
            mgr.clone(),
            tid.clone(),
            s,
            remote,
            PathBuf::from(local),
            resume.unwrap_or(true),
        )
        .await
        {
            mgr.emit(TransferEvent::State {
                id: tid.clone(),
                state: "error".into(),
                message: Some(e.to_string()),
            })
            .await;
        }
        mgr.unregister_transfer(&tid).await;
    });
    sftp.register_transfer(id.clone(), task.abort_handle()).await;
    Ok(id)
}

#[tauri::command]
pub async fn sftp_upload(
    sftp: State<'_, SftpManager>,
    sftp_id: String,
    local: String,
    remote: String,
    resume: Option<bool>,
) -> Result<String, String> {
    let s = sftp.get(&sftp_id).await.map_err(|e| e.to_string())?;
    let id = new_id();
    let mgr = sftp.inner().clone();
    let tid = id.clone();
    let task = tokio::spawn(async move {
        if let Err(e) = crate::core::sftp::transfer::upload(
            mgr.clone(),
            tid.clone(),
            s,
            PathBuf::from(local),
            remote,
            resume.unwrap_or(false),
        )
        .await
        {
            mgr.emit(TransferEvent::State {
                id: tid.clone(),
                state: "error".into(),
                message: Some(e.to_string()),
            })
            .await;
        }
        mgr.unregister_transfer(&tid).await;
    });
    sftp.register_transfer(id.clone(), task.abort_handle()).await;
    Ok(id)
}

#[tauri::command]
pub async fn sftp_cancel(sftp: State<'_, SftpManager>, transfer_id: String) -> Result<bool, String> {
    let aborted = sftp.cancel(&transfer_id).await;
    if aborted {
        sftp.emit(TransferEvent::State {
            id: transfer_id,
            state: "cancelled".into(),
            message: None,
        })
        .await;
    }
    Ok(aborted)
}

/// SCP fallback upload (single file).
#[tauri::command]
pub async fn scp_upload(
    sessions: State<'_, SessionManager>,
    session_id: String,
    local: String,
    remote: String,
) -> Result<(), String> {
    let handle = sessions.ssh_handle(&session_id).await.map_err(|e| e.to_string())?;
    scp::upload(&handle, std::path::Path::new(&local), &remote, |_, _| {})
        .await
        .map_err(|e| e.to_string())
}

/// SCP fallback download (single file).
#[tauri::command]
pub async fn scp_download(
    sessions: State<'_, SessionManager>,
    session_id: String,
    remote: String,
    local: String,
) -> Result<(), String> {
    let handle = sessions.ssh_handle(&session_id).await.map_err(|e| e.to_string())?;
    scp::download(&handle, &remote, std::path::Path::new(&local), |_, _| {})
        .await
        .map_err(|e| e.to_string())
}

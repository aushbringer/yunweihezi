//! Plain-FTP client sessions (suppaftp, tokio backend).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use suppaftp::list::ListParser;
use suppaftp::tokio::AsyncFtpStream;
use suppaftp::types::FileType;
use tokio::sync::Mutex;

use crate::core::db::new_id;
use crate::core::sftp::FileEntry;
use crate::error::{AppError, AppResult};

fn ftp_err(e: suppaftp::types::FtpError) -> AppError {
    AppError::Ftp(e.to_string())
}

#[derive(Clone, Default)]
pub struct FtpManager {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<AsyncFtpStream>>>>>,
}

impl FtpManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Connect + login (anonymous when `username` is empty), binary mode, passive (default).
    pub async fn open(
        &self,
        host: &str,
        port: u16,
        username: Option<&str>,
        password: Option<&str>,
    ) -> AppResult<String> {
        let addr = format!("{host}:{port}");
        let mut stream = AsyncFtpStream::connect(addr).await.map_err(ftp_err)?;
        let user = username.filter(|u| !u.is_empty()).unwrap_or("anonymous");
        let pass = password.filter(|p| !p.is_empty()).unwrap_or("openterm@");
        stream.login(user, pass).await.map_err(ftp_err)?;
        stream
            .transfer_type(FileType::Binary)
            .await
            .map_err(ftp_err)?;
        let id = new_id();
        self.sessions
            .lock()
            .await
            .insert(id.clone(), Arc::new(Mutex::new(stream)));
        Ok(id)
    }

    async fn get(&self, id: &str) -> AppResult<Arc<Mutex<AsyncFtpStream>>> {
        self.sessions
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::SessionNotFound(format!("ftp {id}")))
    }

    pub async fn close(&self, id: &str) {
        if let Some(s) = self.sessions.lock().await.remove(id) {
            let _ = s.lock().await.quit().await;
        }
    }

    /// Directory listing: MLSD first, falling back to LIST + parser for legacy servers.
    pub async fn list(&self, id: &str, path: &str) -> AppResult<Vec<FileEntry>> {
        let s = self.get(id).await?;
        let mut ftp = s.lock().await;
        let mut out: Vec<FileEntry> = match ftp.mlsd(Some(path)).await {
            Ok(lines) => lines
                .iter()
                .filter_map(|l| ListParser::parse_mlsd(l).ok())
                .map(|f| to_entry(path, &f))
                .collect(),
            Err(_) => {
                let lines = ftp.list(Some(path)).await.map_err(ftp_err)?;
                lines
                    .iter()
                    .filter_map(|l| {
                        ListParser::parse_posix(l)
                            .or_else(|_| ListParser::parse_dos(l))
                            .ok()
                    })
                    .map(|f| to_entry(path, &f))
                    .collect()
            }
        };
        out.retain(|e| e.name != "." && e.name != "..");
        out.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    pub async fn mkdir(&self, id: &str, path: &str) -> AppResult<()> {
        let s = self.get(id).await?;
        let mut ftp = s.lock().await;
        ftp.mkdir(path).await.map_err(ftp_err)
    }

    pub async fn delete(&self, id: &str, path: &str, is_dir: bool) -> AppResult<()> {
        let s = self.get(id).await?;
        let mut ftp = s.lock().await;
        let r = if is_dir {
            ftp.rmdir(path).await
        } else {
            ftp.rm(path).await
        };
        r.map_err(ftp_err)
    }

    pub async fn rename(&self, id: &str, old_path: &str, new_path: &str) -> AppResult<()> {
        let s = self.get(id).await?;
        let mut ftp = s.lock().await;
        ftp.rename(old_path, new_path).await.map_err(ftp_err)
    }

    /// RETR `remote` into the local file at `local`.
    pub async fn download(&self, id: &str, remote: &str, local: &Path) -> AppResult<u64> {
        let s = self.get(id).await?;
        let mut ftp = s.lock().await;
        let mut data = ftp.retr_as_stream(remote).await.map_err(ftp_err)?;
        let mut file = tokio::fs::File::create(local).await?;
        let n = tokio::io::copy(&mut data, &mut file).await?;
        data.finish().await.map_err(ftp_err)?;
        Ok(n)
    }

    /// STOR the local file at `local` to `remote`.
    pub async fn upload(&self, id: &str, local: PathBuf, remote: &str) -> AppResult<u64> {
        let s = self.get(id).await?;
        let mut file = tokio::fs::File::open(&local).await?;
        let mut ftp = s.lock().await;
        ftp.put_file(remote, &mut file).await.map_err(ftp_err)
    }
}

fn to_entry(dir: &str, f: &suppaftp::list::File) -> FileEntry {
    let sep = if dir.ends_with('/') { "" } else { "/" };
    let mtime = f
        .modified()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    FileEntry {
        name: f.name().to_string(),
        path: format!("{dir}{sep}{}", f.name()),
        is_dir: f.is_directory(),
        is_symlink: f.is_symlink(),
        size: f.size() as u64,
        mtime,
        permissions: None,
    }
}

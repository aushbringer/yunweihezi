use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("SSH error: {0}")]
    Ssh(#[from] russh::Error),
    #[error("SSH key error: {0}")]
    SshKey(#[from] russh::keys::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SFTP error: {0}")]
    Sftp(String),
    #[error("FTP error: {0}")]
    Ftp(String),
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Crypto error: {0}")]
    Crypto(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Invalid config: {0}")]
    Config(String),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}

pub type AppResult<T> = Result<T, AppError>;

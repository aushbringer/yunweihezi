//! Row models for the SQLite store.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub sort: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialMeta {
    pub id: String,
    pub name: String,
    /// password | key | agent
    pub kind: String,
    pub username: String,
    pub has_secret: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Plaintext credential secret (only returned by `credential_secret` while unlocked).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSecret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub group_id: Option<String>,
    pub name: String,
    /// ssh | telnet | rlogin | serial | local | ftp
    pub protocol: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub credential_id: Option<String>,
    /// JSON blob: jump chain, proxy, forwards, terminal options, serial params...
    pub options: String,
    pub sort: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelRecord {
    pub id: String,
    pub session_id: Option<String>,
    pub name: String,
    /// local | remote | dynamic
    pub kind: String,
    pub listen_host: String,
    pub listen_port: i64,
    pub target_host: String,
    pub target_port: i64,
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCommand {
    pub id: String,
    pub label: String,
    pub content: String,
    pub group: String,
    pub sort: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerRule {
    pub id: String,
    /// NULL = applies to all sessions.
    pub session_id: Option<String>,
    pub name: String,
    pub pattern: String,
    pub is_regex: bool,
    /// highlight | alert | send | script
    pub action: String,
    pub action_param: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileRecord {
    pub id: String,
    pub session_id: Option<String>,
    pub path: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub bytes: i64,
}

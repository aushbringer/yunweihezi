//! Groups / sessions / settings / import-export commands.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::core::db::models::{CredentialMeta, CredentialSecret, Group, SessionRecord};
use crate::core::db::new_id;
use crate::state::AppState;

// ---- groups ----

#[tauri::command]
pub async fn group_list(state: State<'_, AppState>) -> Result<Vec<Group>, String> {
    state.db.group_list().await.map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub sort: i64,
}

/// Create (id=None) or update (id=Some) a group. Returns the id.
#[tauri::command]
pub async fn group_upsert(state: State<'_, AppState>, input: GroupInput) -> Result<String, String> {
    let id = input.id.unwrap_or_else(new_id);
    state
        .db
        .group_upsert(&Group {
            id: id.clone(),
            name: input.name,
            parent_id: input.parent_id,
            sort: input.sort,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn group_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.group_delete(&id).await.map_err(|e| e.to_string())
}

// ---- sessions ----

#[tauri::command]
pub async fn session_list(state: State<'_, AppState>) -> Result<Vec<SessionRecord>, String> {
    state.db.session_list().await.map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    pub name: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: i64,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub credential_id: Option<String>,
    #[serde(default = "default_options")]
    pub options: String,
    #[serde(default)]
    pub sort: i64,
}

fn default_protocol() -> String {
    "ssh".into()
}
fn default_port() -> i64 {
    22
}
fn default_options() -> String {
    "{}".into()
}

/// Create (id=None) or update (id=Some) a session record. Returns the id.
#[tauri::command]
pub async fn session_upsert(
    state: State<'_, AppState>,
    input: SessionInput,
) -> Result<String, String> {
    let is_update = input.id.is_some();
    let id = input.id.unwrap_or_else(new_id);
    // Preserve created_at on update.
    let created_at = if is_update {
        state
            .db
            .session_list()
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.created_at)
            .unwrap_or(0)
    } else {
        0
    };
    state
        .db
        .session_upsert(&SessionRecord {
            id: id.clone(),
            group_id: input.group_id,
            name: input.name,
            protocol: input.protocol,
            host: input.host,
            port: input.port,
            username: input.username,
            credential_id: input.credential_id,
            options: input.options,
            sort: input.sort,
            created_at,
            updated_at: 0,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn session_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.session_delete(&id).await.map_err(|e| e.to_string())
}

// ---- quick commands ----

use crate::core::db::models::{QuickCommand, TriggerRule};

#[tauri::command]
pub async fn quick_command_list(state: State<'_, AppState>) -> Result<Vec<QuickCommand>, String> {
    state.db.quick_command_list().await.map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCommandInput {
    #[serde(default)]
    pub id: Option<String>,
    pub label: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub sort: i64,
}

#[tauri::command]
pub async fn quick_command_upsert(
    state: State<'_, AppState>,
    input: QuickCommandInput,
) -> Result<String, String> {
    let id = input.id.unwrap_or_else(new_id);
    state
        .db
        .quick_command_upsert(&QuickCommand {
            id: id.clone(),
            label: input.label,
            content: input.content,
            group: input.group,
            sort: input.sort,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn quick_command_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.quick_command_delete(&id).await.map_err(|e| e.to_string())
}

// ---- triggers ----

#[tauri::command]
pub async fn trigger_list(state: State<'_, AppState>) -> Result<Vec<TriggerRule>, String> {
    state.db.trigger_list().await.map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub name: String,
    pub pattern: String,
    #[serde(default = "default_true")]
    pub is_regex: bool,
    pub action: String,
    #[serde(default)]
    pub action_param: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub async fn trigger_upsert(
    state: State<'_, AppState>,
    input: TriggerInput,
) -> Result<String, String> {
    if !["highlight", "alert", "send", "script"].contains(&input.action.as_str()) {
        return Err(format!("unknown trigger action: {}", input.action));
    }
    if input.pattern.is_empty() {
        return Err("pattern must not be empty".into());
    }
    let id = input.id.unwrap_or_else(new_id);
    state
        .db
        .trigger_upsert(&TriggerRule {
            id: id.clone(),
            session_id: input.session_id,
            name: input.name,
            pattern: input.pattern,
            is_regex: input.is_regex,
            action: input.action,
            action_param: input.action_param,
            enabled: input.enabled,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn trigger_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.trigger_delete(&id).await.map_err(|e| e.to_string())
}

// ---- settings ----

#[tauri::command]
pub async fn setting_get(state: State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    state.db.setting_get(&key).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn setting_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    state.db.setting_set(&key, &value).await.map_err(|e| e.to_string())
}

// ---- encrypted import / export ----

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportCredential {
    #[serde(flatten)]
    meta: CredentialMeta,
    #[serde(default)]
    secret: Option<CredentialSecret>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportPayload {
    version: u32,
    #[serde(default)]
    groups: Vec<Group>,
    #[serde(default)]
    sessions: Vec<SessionRecord>,
    #[serde(default)]
    credentials: Vec<ExportCredential>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportEnvelope {
    v: u32,
    kdf: &'static str,
    salt: String,
    data: String,
}

/// Export groups+sessions+credentials (secrets included), encrypted with a
/// dedicated export password (Argon2id + AES-256-GCM). Requires unlocked vault.
#[tauri::command]
pub async fn config_export(
    state: State<'_, AppState>,
    password: String,
) -> Result<String, String> {
    use crate::core::crypto::master::{derive_kek, random_bytes, SALT_LEN};
    use crate::core::crypto::vault::Vault;
    use base64::Engine;

    if password.len() < 6 {
        return Err("export password must be at least 6 characters".into());
    }
    let groups = state.db.group_list().await.map_err(|e| e.to_string())?;
    let sessions = state.db.session_list().await.map_err(|e| e.to_string())?;
    let creds = state.db.credential_list().await.map_err(|e| e.to_string())?;
    let mut export_creds = Vec::with_capacity(creds.len());
    for meta in creds {
        let secret = match state
            .db
            .credential_secret_enc(&meta.id)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(blob) => {
                let plain = state.vault.decrypt_b64(&blob).map_err(|e| e.to_string())?;
                Some(serde_json::from_slice::<CredentialSecret>(&plain).map_err(|e| e.to_string())?)
            }
            None => None,
        };
        export_creds.push(ExportCredential { meta, secret });
    }
    let payload = ExportPayload {
        version: 1,
        groups,
        sessions,
        credentials: export_creds,
    };
    let plain = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;

    let salt = random_bytes::<SALT_LEN>();
    let kek = derive_kek(&password, &salt).map_err(|e| e.to_string())?;
    let tmp = Vault::new();
    tmp.set_key(kek);
    let data = tmp.encrypt_b64(&plain).map_err(|e| e.to_string())?;
    let envelope = ExportEnvelope {
        v: 1,
        kdf: "argon2id",
        salt: base64::engine::general_purpose::STANDARD.encode(salt),
        data,
    };
    serde_json::to_string_pretty(&envelope).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub groups: usize,
    pub sessions: usize,
    pub credentials: usize,
}

/// Import a file produced by `config_export`. All rows get fresh ids; group
/// and credential references are remapped.
#[tauri::command]
pub async fn config_import(
    state: State<'_, AppState>,
    content: String,
    password: String,
) -> Result<ImportReport, String> {
    use crate::core::crypto::master::derive_kek;
    use crate::core::crypto::vault::Vault;
    use base64::Engine;
    use std::collections::HashMap;

    #[derive(Deserialize)]
    struct Envelope {
        salt: String,
        data: String,
    }
    let env: Envelope = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let salt = base64::engine::general_purpose::STANDARD
        .decode(&env.salt)
        .map_err(|e| e.to_string())?;
    let kek = derive_kek(&password, &salt).map_err(|e| e.to_string())?;
    let tmp = Vault::new();
    tmp.set_key(kek);
    let plain = tmp.decrypt_b64(&env.data).map_err(|e| e.to_string())?;
    let payload: ExportPayload = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;

    let mut group_map: HashMap<String, String> = HashMap::new();
    // Insert parents before children (multi-pass for nested groups).
    let mut pending = payload.groups.clone();
    for _ in 0..8 {
        if pending.is_empty() {
            break;
        }
        let before = pending.len();
        let mut still_pending = Vec::new();
        for g in pending {
            let parent_ready = g
                .parent_id
                .as_ref()
                .map(|p| group_map.contains_key(p))
                .unwrap_or(true);
            if !parent_ready {
                still_pending.push(g);
                continue;
            }
            let rec = Group {
                id: new_id(),
                name: g.name.clone(),
                parent_id: g.parent_id.as_ref().and_then(|p| group_map.get(p).cloned()),
                sort: g.sort,
            };
            state.db.group_upsert(&rec).await.map_err(|e| e.to_string())?;
            group_map.insert(g.id.clone(), rec.id.clone());
        }
        pending = still_pending;
        if pending.len() == before {
            // Unresolvable parents: import them at root.
            for g in pending.drain(..) {
                let rec = Group {
                    id: new_id(),
                    name: g.name.clone(),
                    parent_id: None,
                    sort: g.sort,
                };
                state.db.group_upsert(&rec).await.map_err(|e| e.to_string())?;
                group_map.insert(g.id.clone(), rec.id.clone());
            }
            break;
        }
    }
    let groups_imported = group_map.len();

    let mut cred_map: HashMap<String, String> = HashMap::new();
    for c in &payload.credentials {
        let nid = new_id();
        let secret_enc = match &c.secret {
            Some(sec) => {
                let json = serde_json::to_vec(sec).map_err(|e| e.to_string())?;
                Some(state.vault.encrypt_b64(&json).map_err(|e| e.to_string())?)
            }
            None => None,
        };
        state
            .db
            .credential_insert(&nid, &c.meta.name, &c.meta.kind, &c.meta.username, secret_enc.as_deref())
            .await
            .map_err(|e| e.to_string())?;
        cred_map.insert(c.meta.id.clone(), nid);
    }

    let mut sessions_imported = 0usize;
    for s in &payload.sessions {
        let rec = SessionRecord {
            id: new_id(),
            group_id: s.group_id.as_ref().and_then(|g| group_map.get(g).cloned()),
            name: s.name.clone(),
            protocol: s.protocol.clone(),
            host: s.host.clone(),
            port: s.port,
            username: s.username.clone(),
            credential_id: s
                .credential_id
                .as_ref()
                .and_then(|c| cred_map.get(c).cloned()),
            options: s.options.clone(),
            sort: s.sort,
            created_at: 0,
            updated_at: 0,
        };
        state.db.session_upsert(&rec).await.map_err(|e| e.to_string())?;
        sessions_imported += 1;
    }

    Ok(ImportReport {
        groups: groups_imported,
        sessions: sessions_imported,
        credentials: cred_map.len(),
    })
}

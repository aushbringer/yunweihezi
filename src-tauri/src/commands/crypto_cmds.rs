//! Master-password / vault / credential commands.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;
use zeroize::Zeroizing;

use crate::core::crypto::master::{random_bytes, derive_kek, KEK_LEN, SALT_LEN};
use crate::core::crypto::vault::{make_verify_blob, unlock_with};
use crate::core::db::models::{CredentialMeta, CredentialSecret};
use crate::core::db::new_id;
use crate::error::AppError;
use crate::state::AppState;

const VERIFY_PLAINTEXT: &[u8] = b"openterm-vault-check-v1";
const META_SALT: &str = "kdf_salt";
const META_VERIFY: &str = "verify_blob";
/// Random per-install key used as KEK when no master password is set.
const KEYFILE_NAME: &str = "vault.key";

fn keyfile_path(state: &AppState) -> PathBuf {
    state.data_dir.join(KEYFILE_NAME)
}

/// Load (or create on first run) the local keyfile and unlock the vault with
/// it. This is the no-password mode: secrets are encrypted at rest with a key
/// that only exists in this user's data directory.
fn unlock_with_keyfile(state: &AppState) -> Result<(), String> {
    use base64::Engine;
    let b64 = &base64::engine::general_purpose::STANDARD;
    let path = keyfile_path(state);
    let kek_b64 = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            let raw = random_bytes::<KEK_LEN>();
            let encoded = b64.encode(raw);
            std::fs::write(&path, &encoded)
                .map_err(|e| format!("write keyfile {}: {e}", path.display()))?;
            encoded
        }
    };
    let raw = b64
        .decode(kek_b64.trim())
        .map_err(|e| format!("corrupt keyfile: {e}"))?;
    if raw.len() != KEK_LEN {
        return Err("corrupt keyfile: bad length".into());
    }
    let mut kek = Zeroizing::new([0u8; KEK_LEN]);
    kek.copy_from_slice(&raw);
    state.vault.set_key(kek);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoStatus {
    pub master_set: bool,
    pub unlocked: bool,
}

#[tauri::command]
pub async fn crypto_status(state: State<'_, AppState>) -> Result<CryptoStatus, String> {
    let master_set = state
        .db
        .meta_get(META_SALT)
        .await
        .map_err(|e| e.to_string())?
        .is_some();
    // No-password mode: transparently unlock with the local keyfile so the
    // app is usable without ever setting a master password.
    if !master_set && !state.vault.is_unlocked() {
        unlock_with_keyfile(&state)?;
    }
    Ok(CryptoStatus {
        master_set,
        unlocked: state.vault.is_unlocked(),
    })
}

/// First-time master password setup. Existing keyfile-encrypted credentials
/// are re-encrypted under the new password-derived KEK.
#[tauri::command]
pub async fn crypto_set_master(state: State<'_, AppState>, password: String) -> Result<(), String> {
    if password.len() < 6 {
        return Err("master password must be at least 6 characters".into());
    }
    if state.db.meta_get(META_SALT).await.map_err(|e| e.to_string())?.is_some() {
        return Err("master password already set".into());
    }
    // If the vault is currently unlocked (keyfile mode), snapshot all stored
    // secrets so they can be re-encrypted under the new KEK below.
    let mut plaintexts: Vec<(String, Vec<u8>)> = Vec::new();
    if state.vault.is_unlocked() {
        let metas = state.db.credential_list().await.map_err(|e| e.to_string())?;
        for m in metas.into_iter().filter(|m| m.has_secret) {
            if let Some(blob) = state
                .db
                .credential_secret_enc(&m.id)
                .await
                .map_err(|e| e.to_string())?
            {
                let plain = state.vault.decrypt_b64(&blob).map_err(|e| e.to_string())?;
                plaintexts.push((m.id, plain));
            }
        }
    }
    let salt = random_bytes::<SALT_LEN>();
    let kek = derive_kek(&password, &salt).map_err(|e| e.to_string())?;
    let verify = make_verify_blob(&kek, VERIFY_PLAINTEXT).map_err(|e| e.to_string())?;
    use base64::Engine;
    state
        .db
        .meta_set(META_SALT, &base64::engine::general_purpose::STANDARD.encode(salt))
        .await
        .map_err(|e| e.to_string())?;
    state
        .db
        .meta_set(META_VERIFY, &verify)
        .await
        .map_err(|e| e.to_string())?;
    state.vault.set_key(kek);
    // Re-encrypt the snapshot under the new (password-derived) key.
    for (id, plain) in plaintexts {
        let enc = state.vault.encrypt_b64(&plain).map_err(|e| e.to_string())?;
        state
            .db
            .credential_set_secret(&id, &enc)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn crypto_unlock(state: State<'_, AppState>, password: String) -> Result<(), String> {
    let salt = state
        .db
        .meta_get(META_SALT)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("master password not set yet")?;
    let verify = state
        .db
        .meta_get(META_VERIFY)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("vault metadata incomplete")?;
    let kek = unlock_with(&password, &salt, &verify).map_err(|e| e.to_string())?;
    state.vault.set_key(kek);
    Ok(())
}

#[tauri::command]
pub async fn crypto_lock(state: State<'_, AppState>) -> Result<(), String> {
    state.vault.lock();
    Ok(())
}

// ---- credentials ----

#[tauri::command]
pub async fn credential_list(state: State<'_, AppState>) -> Result<Vec<CredentialMeta>, String> {
    state.db.credential_list().await.map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInput {
    pub name: String,
    /// password | key | agent
    pub kind: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub secret: Option<CredentialSecret>,
}

#[tauri::command]
pub async fn credential_create(
    state: State<'_, AppState>,
    input: CredentialInput,
) -> Result<String, String> {
    let secret_enc = match &input.secret {
        Some(sec) => {
            let json = serde_json::to_vec(sec).map_err(|e| e.to_string())?;
            Some(state.vault.encrypt_b64(&json).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let id = new_id();
    state
        .db
        .credential_insert(&id, &input.name, &input.kind, &input.username, secret_enc.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn credential_update(
    state: State<'_, AppState>,
    id: String,
    input: CredentialInput,
) -> Result<(), String> {
    // secret: None = keep existing; Some = replace.
    let secret_enc = match &input.secret {
        Some(sec) => {
            let json = serde_json::to_vec(sec).map_err(|e| e.to_string())?;
            Some(Some(state.vault.encrypt_b64(&json).map_err(|e| e.to_string())?))
        }
        None => None,
    };
    state
        .db
        .credential_update(&id, &input.name, &input.kind, &input.username, secret_enc)
        .await
        .map_err(|e| e.to_string())
}

/// Decrypt and return a credential's secret. Requires unlocked vault.
#[tauri::command]
pub async fn credential_secret(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<CredentialSecret>, String> {
    let enc = state
        .db
        .credential_secret_enc(&id)
        .await
        .map_err(|e| e.to_string())?;
    match enc {
        None => Ok(None),
        Some(blob) => {
            let plain = state.vault.decrypt_b64(&blob).map_err(|e| e.to_string())?;
            let sec: CredentialSecret =
                serde_json::from_slice(&plain).map_err(AppError::from).map_err(|e| e.to_string())?;
            Ok(Some(sec))
        }
    }
}

#[tauri::command]
pub async fn credential_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.credential_delete(&id).await.map_err(|e| e.to_string())
}

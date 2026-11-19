//! Credential vault: AES-256-GCM with a KEK held in memory while unlocked.

use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

use super::master::{derive_kek, random_bytes, KEK_LEN};

const NONCE_LEN: usize = 12;

pub struct Vault {
    kek: Mutex<Option<Zeroizing<[u8; KEK_LEN]>>>,
}

impl Vault {
    pub fn new() -> Self {
        Self {
            kek: Mutex::new(None),
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.kek.lock().map(|k| k.is_some()).unwrap_or(false)
    }

    pub fn set_key(&self, kek: Zeroizing<[u8; KEK_LEN]>) {
        if let Ok(mut guard) = self.kek.lock() {
            *guard = Some(kek);
        }
    }

    pub fn lock(&self) {
        if let Ok(mut guard) = self.kek.lock() {
            // Zeroizing drop wipes the key material.
            *guard = None;
        }
    }

    /// Encrypt `plaintext`; output = nonce(12) || ciphertext+tag, base64.
    pub fn encrypt_b64(&self, plaintext: &[u8]) -> AppResult<String> {
        let guard = self
            .kek
            .lock()
            .map_err(|_| AppError::Crypto("vault poisoned".into()))?;
        let kek = guard
            .as_ref()
            .ok_or_else(|| AppError::Crypto("vault locked".into()))?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek[..]));
        let nonce_bytes = random_bytes::<NONCE_LEN>();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| AppError::Crypto("encrypt failed".into()))?;
        let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ct);
        Ok(base64::engine::general_purpose::STANDARD.encode(blob))
    }

    /// Decrypt a base64 blob produced by [`encrypt_b64`].
    pub fn decrypt_b64(&self, blob_b64: &str) -> AppResult<Vec<u8>> {
        let blob = base64::engine::general_purpose::STANDARD
            .decode(blob_b64)
            .map_err(|e| AppError::Crypto(format!("bad blob b64: {e}")))?;
        if blob.len() < NONCE_LEN + 16 {
            return Err(AppError::Crypto("blob too short".into()));
        }
        let guard = self
            .kek
            .lock()
            .map_err(|_| AppError::Crypto("vault poisoned".into()))?;
        let kek = guard
            .as_ref()
            .ok_or_else(|| AppError::Crypto("vault locked".into()))?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek[..]));
        let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
        cipher
            .decrypt(nonce, &blob[NONCE_LEN..])
            .map_err(|_| AppError::Crypto("decrypt failed (wrong password?)".into()))
    }
}

/// Derive a KEK from `password`/`salt` and verify it against `verify_blob`
/// (nonce||ciphertext of a known plaintext). Returns the KEK on success.
pub fn unlock_with(
    password: &str,
    salt_b64: &str,
    verify_b64: &str,
) -> AppResult<Zeroizing<[u8; KEK_LEN]>> {
    let b64 = &base64::engine::general_purpose::STANDARD;
    let salt = b64
        .decode(salt_b64)
        .map_err(|e| AppError::Crypto(format!("bad salt: {e}")))?;
    let kek = derive_kek(password, &salt)?;
    let blob = b64
        .decode(verify_b64)
        .map_err(|e| AppError::Crypto(format!("bad verify blob: {e}")))?;
    if blob.len() < NONCE_LEN + 16 {
        return Err(AppError::Crypto("verify blob corrupt".into()));
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek[..]));
    cipher
        .decrypt(
            Nonce::from_slice(&blob[..NONCE_LEN]),
            &blob[NONCE_LEN..],
        )
        .map_err(|_| AppError::Crypto("wrong master password".into()))?;
    Ok(kek)
}

/// Encrypt a known-plaintext check blob used to verify future unlocks.
pub fn make_verify_blob(kek: &[u8; KEK_LEN], plaintext: &[u8]) -> AppResult<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek[..]));
    let nonce_bytes = random_bytes::<NONCE_LEN>();
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|_| AppError::Crypto("encrypt failed".into()))?;
    let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ct);
    Ok(base64::engine::general_purpose::STANDARD.encode(blob))
}

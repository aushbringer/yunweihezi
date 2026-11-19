//! Master-key derivation: Argon2id(password, salt) -> 256-bit KEK.

use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

pub const KEK_LEN: usize = 32;
pub const SALT_LEN: usize = 16;

/// Argon2id parameters: m=64 MiB, t=3, p=4.
fn kdf_params() -> AppResult<Params> {
    Params::new(64 * 1024, 3, 4, Some(KEK_LEN))
        .map_err(|e| AppError::Crypto(format!("argon2 params: {e}")))
}

pub fn derive_kek(password: &str, salt: &[u8]) -> AppResult<Zeroizing<[u8; KEK_LEN]>> {
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, kdf_params()?);
    let mut kek = Zeroizing::new([0u8; KEK_LEN]);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut kek[..])
        .map_err(|e| AppError::Crypto(format!("argon2 kdf: {e}")))?;
    Ok(kek)
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    use rand::RngCore;
    let mut buf = [0u8; N];
    rand::rng().fill_bytes(&mut buf);
    buf
}

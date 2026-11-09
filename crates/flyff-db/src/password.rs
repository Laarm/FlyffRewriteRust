//! Account passwords (Plan/12 §5.2, 13 §5.2; D3/D5): the browser sends `pwhash = md5_hex_lower(salt + password)`
//! (the original `NEUZ_PHASH`), the database stores `argon2id(pwhash)` as a PHC string.
//!
//! These helpers are synchronous and CPU-heavy (about 20-40 ms per call): async callers run them in
//! `spawn_blocking` behind the 2-permit semaphore of `lobby::auth` (R14). They define the constants once so
//! that the lobby, the offline CLI and the tests agree on the parameters.

use crate::error::{DbError, DbErrorKind};
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use md5::{Digest, Md5};

/// argon2id memory cost in KiB (OWASP 2023 minimum).
pub const ARGON2_M_KIB: u32 = 19_456;
/// argon2id passes.
pub const ARGON2_T: u32 = 2;
/// argon2id lanes.
pub const ARGON2_P: u32 = 1;
/// argon2id output length in bytes.
pub const ARGON2_OUTPUT_LEN: usize = 32;
/// The PHC prefix every stored hash created by [`hash_pwhash`] starts with.
pub const PHC_PREFIX: &str = "$argon2id$v=19$m=19456,t=2,p=1$";
/// The default `auth.legacy_md5_salt`.
pub const DEFAULT_MD5_SALT: &str = "kikugalanet";

/// Result of [`verify_pwhash`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verify {
    /// The password matches. `needs_rehash` is set when the stored PHC parameters differ from the current
    /// constants: the caller computes a new hash and sends `SetAccountPassword` (transparent upgrade path).
    Ok {
        /// The stored hash uses other parameters than [`ARGON2_M_KIB`]/[`ARGON2_T`]/[`ARGON2_P`].
        needs_rehash: bool,
    },
    /// The password does not match.
    Mismatch,
}

fn hasher() -> Result<Argon2<'static>, DbError> {
    let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(ARGON2_OUTPUT_LEN))
        .map_err(|e| DbError::new(DbErrorKind::Bug, format!("argon2 parameters: {e}")))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// `lowercase_hex(md5(utf8(salt + password)))`: the value the browser (and the CLI) sends instead of the
/// password. `legacy_pwhash("kikugalanet", "test") == "89d1ed22aac58f5bbea53b2fde81a946"`.
pub fn legacy_pwhash(salt: &str, password: &str) -> String {
    let mut md5 = Md5::new();
    md5.update(salt.as_bytes());
    md5.update(password.as_bytes());
    md5.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `true` for exactly 32 lowercase hex characters (the only valid shape of a `pwhash`).
pub fn is_valid_pwhash(pwhash: &str) -> bool {
    pwhash.len() == 32
        && pwhash
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// argon2id over the 32 ASCII bytes of `pwhash` with a random 16-byte salt, as a PHC string.
pub fn hash_pwhash(pwhash: &str) -> Result<String, DbError> {
    let hash = hasher()?
        .hash_password(pwhash.as_bytes())
        .map_err(|e| DbError::new(DbErrorKind::Bug, format!("argon2 hashing failed: {e}")))?;
    Ok(hash.to_string())
}

/// Verifies `pwhash` against a stored PHC string in constant time. A stored value that is not a valid
/// argon2id PHC string is an error (`Bug`), not a mismatch.
pub fn verify_pwhash(pwhash: &str, phc: &str) -> Result<Verify, DbError> {
    let parsed = PasswordHash::new(phc).map_err(|e| {
        DbError::new(
            DbErrorKind::Bug,
            format!("stored password hash is malformed: {e}"),
        )
    })?;
    match hasher()?.verify_password(pwhash.as_bytes(), &parsed) {
        Ok(()) => {
            let current = parsed.algorithm.as_str() == "argon2id"
                && Params::try_from(&parsed).is_ok_and(|p| {
                    p.m_cost() == ARGON2_M_KIB && p.t_cost() == ARGON2_T && p.p_cost() == ARGON2_P
                });
            Ok(Verify::Ok {
                needs_rehash: !current,
            })
        }
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(Verify::Mismatch),
        Err(e) => Err(DbError::new(
            DbErrorKind::Bug,
            format!("argon2 verification failed: {e}"),
        )),
    }
}

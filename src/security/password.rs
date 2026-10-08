//! Argon2id hashing, verification on the blocking pool, and the password policy
//! (FR-001, FR-004, FR-008, research R8, R9).

use std::fmt;

/// Password length bounds in Unicode scalar values (FR-004).
pub const MIN_CHARS: usize = 12;
pub const MAX_CHARS: usize = 128;

/// Why a password is refused by the policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    /// Outside 12–128 characters; carries the actual count.
    Length(usize),
    EqualsUsername,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// Checks FR-004 before hashing.
pub fn check_policy(_username: &str, _password: &str) -> Result<(), PolicyError> {
    todo!()
}

/// Hashing failed (never expected with the fixed parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashError;

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("password hashing failed")
    }
}

impl std::error::Error for HashError {}

/// Argon2id v19, m = 19456 KiB, t = 2, p = 1, 16-byte random salt, PHC string.
pub fn hash(_password: &str) -> Result<String, HashError> {
    todo!()
}

/// The hash of a random 32-character password, computed once at startup (research R8).
pub fn dummy_hash() -> String {
    todo!()
}

/// Verification could not run: both map to `500` (research R14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    /// The stored hash is not a parsable Argon2 PHC string.
    MalformedHash,
    /// The blocking task panicked or was cancelled.
    Join,
}

/// Verifies `password` against `phc` on the blocking pool.
pub async fn verify(_password: String, _phc: String) -> Result<bool, VerifyError> {
    todo!()
}

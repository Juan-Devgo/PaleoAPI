//! Access tokens: HS256 JWT issue and verification (FR-011–FR-013, research R6, data-model §2).

use std::fmt;

use serde::{Deserialize, Serialize};

/// `iss` and `aud` of every token.
pub const ISSUER: &str = "paleo-api";
pub const AUDIENCE: &str = "paleo-api";
/// Token lifetime (FR-011).
pub const LIFETIME_SECS: u64 = 3600;
/// Clock tolerance for `exp` and `nbf` (FR-013).
pub const LEEWAY_SECS: u64 = 60;
/// Longer tokens are rejected before decoding.
pub const MAX_TOKEN_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub iat: u64,
    pub nbf: u64,
    pub exp: u64,
    /// `accounts.credentials_version` at issue (research R7).
    pub ver: i32,
}

impl Claims {
    /// Claims for `username` issued at `iat` (Unix seconds).
    pub fn new(_username: &str, _ver: i32, _iat: u64) -> Self {
        todo!()
    }

    /// Claims issued now (system clock).
    pub fn now(_username: &str, _ver: i32) -> Self {
        todo!()
    }
}

/// Signing and verification keys derived from `JWT_SECRET`.
pub struct Keys {
    _private: (),
}

impl Keys {
    pub fn new(_secret: &[u8]) -> Self {
        todo!()
    }
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Keys(..)")
    }
}

/// Why a token was rejected (research R19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Expired,
    Invalid,
}

/// Encoding failed (never expected for these claims).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueError;

pub fn issue(_keys: &Keys, _claims: &Claims) -> Result<String, IssueError> {
    todo!()
}

/// Verifies signature, algorithm, and claims; `ver` and the account are checked by the caller.
pub fn verify(_keys: &Keys, _token: &str) -> Result<Claims, Rejection> {
    todo!()
}

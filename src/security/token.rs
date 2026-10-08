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

#[cfg(test)]
mod tests {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, get_current_timestamp};
    use serde_json::{Value, json};

    use super::*;

    const SECRET: &[u8] = b"unit-test-secret-0123456789abcdef";

    fn keys() -> Keys {
        Keys::new(SECRET)
    }

    /// Signs arbitrary claims with `alg` and `secret`, bypassing [`issue`].
    fn sign(alg: Algorithm, claims: &Value, secret: &[u8]) -> String {
        encode(&Header::new(alg), claims, &EncodingKey::from_secret(secret)).unwrap()
    }

    fn valid_claims() -> Value {
        let now = get_current_timestamp();
        json!({
            "sub": "alice", "iss": ISSUER, "aud": AUDIENCE,
            "iat": now, "nbf": now, "exp": now + LIFETIME_SECS, "ver": 1
        })
    }

    #[test]
    fn claims_follow_the_data_model() {
        let c = Claims::new("alice", 3, 1_000);
        assert_eq!(
            c,
            Claims {
                sub: "alice".into(),
                iss: "paleo-api".into(),
                aud: "paleo-api".into(),
                iat: 1_000,
                nbf: 1_000,
                exp: 4_600,
                ver: 3,
            }
        );
        let now = get_current_timestamp();
        let c = Claims::now("alice", 1);
        assert!(c.iat.abs_diff(now) <= 2, "{c:?}");
        assert_eq!(c.exp, c.iat + 3600);
    }

    #[test]
    fn issued_token_round_trips() {
        let keys = keys();
        let claims = Claims::now("alice", 7);
        let token = issue(&keys, &claims).unwrap();
        assert_eq!(token.split('.').count(), 3);
        let header = jsonwebtoken::decode_header(&token).unwrap();
        assert_eq!(header.alg, Algorithm::HS256);
        assert_eq!(header.typ.as_deref(), Some("JWT"));
        assert_eq!(verify(&keys, &token), Ok(claims));
    }

    #[test]
    fn expiry_has_a_60_second_tolerance() {
        let keys = keys();
        let now = get_current_timestamp();
        let past = |secs: u64| Claims::new("alice", 1, now - LIFETIME_SECS - secs);
        let ok = past(50);
        assert_eq!(verify(&keys, &issue(&keys, &ok).unwrap()), Ok(ok));
        assert_eq!(
            verify(&keys, &issue(&keys, &past(61)).unwrap()),
            Err(Rejection::Expired)
        );
    }

    #[test]
    fn not_before_has_a_60_second_tolerance() {
        let keys = keys();
        let now = get_current_timestamp();
        let ok = Claims::new("alice", 1, now + 50);
        assert_eq!(verify(&keys, &issue(&keys, &ok).unwrap()), Ok(ok));
        let early = Claims::new("alice", 1, now + 61);
        assert_eq!(
            verify(&keys, &issue(&keys, &early).unwrap()),
            Err(Rejection::Invalid)
        );
    }

    #[test]
    fn another_secret_or_algorithm_is_invalid() {
        let claims = valid_claims();
        let other = sign(
            Algorithm::HS256,
            &claims,
            b"another-secret-0123456789abcdef!",
        );
        assert_eq!(verify(&keys(), &other), Err(Rejection::Invalid));
        for alg in [Algorithm::HS384, Algorithm::HS512] {
            let token = sign(alg, &claims, SECRET);
            assert_eq!(verify(&keys(), &token), Err(Rejection::Invalid), "{alg:?}");
        }
        assert!(verify(&keys(), &sign(Algorithm::HS256, &claims, SECRET)).is_ok());
    }

    #[test]
    fn missing_or_wrong_claims_are_invalid() {
        let mut cases: Vec<Value> = Vec::new();
        for field in ["sub", "iss", "aud", "iat", "nbf", "exp", "ver"] {
            let mut c = valid_claims();
            c.as_object_mut().unwrap().remove(field);
            cases.push(c);
        }
        for (field, value) in [
            ("iss", json!("someone-else")),
            ("aud", json!("someone-else")),
            ("ver", json!("1")),
            ("ver", json!(1u64 << 40)),
            ("exp", json!("tomorrow")),
        ] {
            let mut c = valid_claims();
            c[field] = value;
            cases.push(c);
        }
        for c in cases {
            let token = sign(Algorithm::HS256, &c, SECRET);
            assert_eq!(verify(&keys(), &token), Err(Rejection::Invalid), "{c}");
        }
    }

    #[test]
    fn extra_claims_are_ignored() {
        let mut c = valid_claims();
        c["role"] = json!("admin");
        let token = sign(Algorithm::HS256, &c, SECRET);
        assert_eq!(verify(&keys(), &token).unwrap().sub, "alice");
    }

    #[test]
    fn malformed_and_oversized_tokens_are_invalid() {
        let keys = keys();
        let token = issue(&keys, &Claims::now("alice", 1)).unwrap();
        let (unsigned, _) = token.rsplit_once('.').unwrap();
        for bad in [
            "",
            "garbage",
            "a.b.c",
            unsigned,
            &format!("{unsigned}."),
            &format!("{token}x"),
            &format!(" {token}"),
        ] {
            assert_eq!(verify(&keys, bad), Err(Rejection::Invalid), "{bad:?}");
        }
        let huge = Claims::now(&"a".repeat(MAX_TOKEN_BYTES), 1);
        let huge = issue(&keys, &huge).unwrap();
        assert!(huge.len() > MAX_TOKEN_BYTES);
        assert_eq!(verify(&keys, &huge), Err(Rejection::Invalid));
    }

    #[test]
    fn keys_debug_hides_the_secret() {
        assert!(!format!("{:?}", keys()).contains("unit-test-secret"));
    }
}

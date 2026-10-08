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

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape enforced by `accounts_password_hash_ck` (data-model §1), with the
    /// parameters of research R8.
    fn assert_phc(phc: &str) {
        let parts: Vec<&str> = phc.split('$').collect();
        assert_eq!(parts.len(), 6, "{phc}");
        assert_eq!(parts[0], "");
        assert_eq!(parts[1], "argon2id");
        assert_eq!(parts[2], "v=19");
        assert_eq!(parts[3], "m=19456,t=2,p=1");
        assert_eq!(parts[4].len(), 22, "16-byte salt: {phc}");
        assert_eq!(parts[5].len(), 43, "32-byte output: {phc}");
        let b64 = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/')
        };
        assert!(b64(parts[4]) && b64(parts[5]), "{phc}");
    }

    #[test]
    fn length_is_counted_in_characters() {
        assert_eq!(
            check_policy("alice", &"x".repeat(11)),
            Err(PolicyError::Length(11))
        );
        assert_eq!(check_policy("alice", &"x".repeat(12)), Ok(()));
        assert_eq!(check_policy("alice", &"x".repeat(128)), Ok(()));
        assert_eq!(
            check_policy("alice", &"x".repeat(129)),
            Err(PolicyError::Length(129))
        );
        assert_eq!(check_policy("alice", ""), Err(PolicyError::Length(0)));
        // Twelve two-byte characters are twelve characters (research R9).
        assert_eq!(check_policy("alice", &"é".repeat(12)), Ok(()));
        assert_eq!(
            check_policy("alice", &"é".repeat(11)),
            Err(PolicyError::Length(11))
        );
        assert_eq!(
            check_policy("alice", &"🦖".repeat(129)),
            Err(PolicyError::Length(129))
        );
    }

    #[test]
    fn password_equal_to_the_username_is_refused_ignoring_case() {
        let user = "museum-curator-1";
        assert_eq!(check_policy(user, user), Err(PolicyError::EqualsUsername));
        assert_eq!(
            check_policy(user, "MUSEUM-Curator-1"),
            Err(PolicyError::EqualsUsername)
        );
        assert_eq!(check_policy(user, "museum-curator-12"), Ok(()));
    }

    #[test]
    fn policy_messages_follow_the_tool_contract() {
        assert_eq!(
            PolicyError::Length(11).to_string(),
            "the password must be 12–128 characters (got 11)."
        );
        assert_eq!(
            PolicyError::EqualsUsername.to_string(),
            "the password must not be the username."
        );
    }

    #[actix_web::test]
    async fn hash_is_salted_argon2id_and_verifies() {
        let password = "correct horse battery staple";
        let first = hash(password).unwrap();
        let second = hash(password).unwrap();
        assert_phc(&first);
        assert_phc(&second);
        assert_ne!(first, second, "each hash has its own salt");
        assert!(!first.contains(password));

        assert_eq!(verify(password.into(), first.clone()).await, Ok(true));
        assert_eq!(verify(password.into(), second).await, Ok(true));
        assert_eq!(
            verify("correct horse battery stapl".into(), first.clone()).await,
            Ok(false)
        );
        assert_eq!(verify(String::new(), first).await, Ok(false));
    }

    #[actix_web::test]
    async fn unparsable_stored_hash_is_an_error() {
        for bad in ["", "not a hash", "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA"] {
            assert_eq!(
                verify("whatever-password".into(), bad.into()).await,
                Err(VerifyError::MalformedHash),
                "{bad}"
            );
        }
    }

    #[actix_web::test]
    async fn dummy_hash_has_the_same_parameters_and_never_verifies() {
        let dummy = dummy_hash();
        assert_phc(&dummy);
        assert_ne!(dummy, dummy_hash(), "random password and salt each time");
        for guess in [
            "",
            "change-me",
            "password1234",
            "0000000000000000000000000000000",
        ] {
            assert_eq!(
                verify(guess.into(), dummy.clone()).await,
                Ok(false),
                "{guess}"
            );
        }
    }
}

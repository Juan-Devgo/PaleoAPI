//! Test-only admin credentials (research R4). This is their only definition:
//! no build of the library or binary can accept them (`repo_hygiene.rs`).

/// Passes `TestGate` as an admin.
pub const TEST_ADMIN_TOKEN: &str = "paleo-test-admin-7c1e9a52f04b";
/// Valid credentials without the admin role: `TestGate` answers `403`.
pub const TEST_USER_TOKEN: &str = "paleo-test-user-3b8d60e1a9c7";

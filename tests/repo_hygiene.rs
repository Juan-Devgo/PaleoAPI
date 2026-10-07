//! Repository hygiene checks (AC 13, AC 15, FR-002, FR-007, FR-015).
//! Uses only `std` and the `git` binary.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PLACEHOLDER_PASSWORD: &str = "change-me";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn git(args: &[&str]) -> (bool, String) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root())
        .output()
        .expect("git must be installed to run repository hygiene tests");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// Parses `KEY=value` lines, ignoring comments and blank lines.
fn parse_env_file(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            (k.trim().to_string(), v.to_string())
        })
        .collect()
}

/// Password part of `scheme://user:password@host/...`, if any.
fn url_password(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let userinfo = rest.split_once('@')?.0;
    Some(userinfo.split_once(':')?.1)
}

/// Database name of a connection URL: the path after the last `/`, before `?`.
fn url_database(url: &str) -> &str {
    let before_query = url.split('?').next().unwrap_or(url);
    before_query.rsplit('/').next().unwrap_or("")
}

#[test]
fn gitignore_lists_dotenv() {
    let gitignore = read(".gitignore");
    assert!(
        gitignore
            .lines()
            .map(str::trim)
            .any(|l| l == ".env" || l == "/.env"),
        ".gitignore must ignore .env"
    );
}

#[test]
fn env_example_defines_placeholders_only() {
    let vars = parse_env_file(&read(".env.example"));
    for key in [
        "POSTGRES_USER",
        "POSTGRES_PASSWORD",
        "POSTGRES_DB",
        "POSTGRES_PORT",
        "DATABASE_URL",
        "TEST_DATABASE_URL",
    ] {
        assert!(vars.contains_key(key), ".env.example must define {key}");
    }
    assert_eq!(vars["POSTGRES_PASSWORD"], PLACEHOLDER_PASSWORD);
    for key in ["DATABASE_URL", "TEST_DATABASE_URL"] {
        assert_eq!(
            url_password(&vars[key]),
            Some(PLACEHOLDER_PASSWORD),
            "{key} in .env.example must use the {PLACEHOLDER_PASSWORD} placeholder password"
        );
    }
    assert_eq!(
        url_database(&vars["TEST_DATABASE_URL"]),
        "postgres",
        "TEST_DATABASE_URL must point at the postgres maintenance database"
    );
}

#[test]
fn dotenv_is_not_tracked() {
    let (ok, files) = git(&["ls-files"]);
    assert!(ok, "git ls-files failed");
    assert!(
        !files.lines().any(|f| f == ".env" || f.ends_with("/.env")),
        ".env must not be tracked by git"
    );
}

#[test]
fn no_credentialed_urls_except_placeholders() {
    // Built from parts so this file never contains a credentialed URL itself.
    let pattern = [
        "postgres(ql)?",
        "://",
        "[^:@[:space:]]+",
        ":",
        "[^@[:space:]]+",
        "@",
    ]
    .concat();
    let (_, out) = git(&["grep", "--untracked", "-nE", &pattern, "--", ".", ":!docs"]);
    let leaks: Vec<&str> = out
        .lines()
        .filter(|l| !l.contains(PLACEHOLDER_PASSWORD))
        .collect();
    assert!(
        leaks.is_empty(),
        "credentialed URLs found:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn draft_schema_is_removed() {
    assert!(!root().join("src/db/shemas_definition.sql").exists());
    assert!(
        !Path::new(&root().join("src/db")).is_dir(),
        "src/db/ must be removed"
    );
}

/// Only the binary startup tests and this file may mention the variable;
/// every other test connects through the harness (R14).
#[test]
fn database_url_only_in_allowed_test_files() {
    let var = ["DATABASE", "_URL"].concat();
    let allowed = [
        root().join("tests/startup.rs"),
        root().join("tests/repo_hygiene.rs"),
    ];
    let mut offenders = Vec::new();
    let mut stack = vec![root().join("tests")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && !allowed.contains(&path)
                && fs::read_to_string(&path).unwrap().contains(&var)
            {
                offenders.push(path.display().to_string());
            }
        }
    }
    assert!(offenders.is_empty(), "{var} mentioned in: {offenders:?}");
}

/// String literals of the `const …: &str = "…";` items in `tests/api/tokens.rs`.
fn test_token_literals() -> Vec<String> {
    let text = read("tests/api/tokens.rs");
    let tokens: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("pub const ") && l.contains(": &str"))
        .filter_map(|l| l.split('"').nth(1))
        .map(String::from)
        .collect();
    assert!(
        tokens.len() >= 2,
        "tests/api/tokens.rs must define TEST_ADMIN_TOKEN and TEST_USER_TOKEN"
    );
    tokens
}

/// `git grep` over tracked and untracked files under `src/`.
fn grep_src(fixed: &str) -> String {
    let (_, out) = git(&["grep", "--untracked", "-nF", fixed, "--", "src"]);
    out
}

/// The test-only credential never reaches the library or binary (spec §4.4, research R4).
#[test]
fn test_tokens_are_absent_from_src() {
    for token in test_token_literals() {
        let hits = grep_src(&token);
        assert!(hits.is_empty(), "test token found in src/:\n{hits}");
    }
}

/// `DenyAll` is the only admin gate shipped in a build (research R4).
#[test]
fn deny_all_is_the_only_admin_gate_in_src() {
    let hits = grep_src("impl AdminGate for");
    let impls: Vec<&str> = hits.lines().collect();
    assert_eq!(
        impls.len(),
        1,
        "expected one AdminGate impl in src/: {impls:?}"
    );
    assert!(
        impls[0].contains("impl AdminGate for DenyAll"),
        "the only AdminGate in src/ must be DenyAll: {impls:?}"
    );
}

/// Builds use the committed `.sqlx/` metadata (research R3).
#[test]
fn env_example_enables_sqlx_offline() {
    let vars = parse_env_file(&read(".env.example"));
    assert_eq!(
        vars.get("SQLX_OFFLINE").map(String::as_str),
        Some("true"),
        ".env.example must set SQLX_OFFLINE=true"
    );
}

/// The test harness must use the `postgres` maintenance database as its
/// master, never the development database (FR-015, AC 13).
#[test]
fn test_harness_uses_the_master_database() {
    let var = ["DATABASE", "_URL"].concat();
    let url = std::env::var(&var).ok().or_else(|| {
        fs::read_to_string(root().join(".env"))
            .ok()
            .and_then(|text| parse_env_file(&text).remove(&var))
    });
    let Some(url) = url else {
        return; // nothing configured: the database tests cannot run at all
    };
    assert!(
        url_database(&url) == "postgres",
        "Tests must run with the documented command DATABASE_URL=\"$TEST_DATABASE_URL\" cargo test \
         (the postgres maintenance database). This run may have written the test harness's _sqlx_test \
         bookkeeping schema into the development database; no resource data was read or changed. \
         Clean up with the step in quickstart §5."
    );
}

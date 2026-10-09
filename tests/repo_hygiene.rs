//! Repository hygiene checks (AC 13, AC 15, FR-002, FR-007, FR-015;
//! Spec 003: AC 9, FR-014, FR-016, FR-041).
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
        root().join("tests/api/security/accounts_cli.rs"),
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

/// The JWT secret in `.env.example` is a placeholder the API refuses (003 AC 9, FR-014).
#[test]
fn env_example_jwt_secret_is_a_placeholder() {
    let vars = parse_env_file(&read(".env.example"));
    let secret = vars
        .get("JWT_SECRET")
        .expect(".env.example must define JWT_SECRET");
    assert!(
        secret.contains(PLACEHOLDER_PASSWORD),
        "JWT_SECRET in .env.example must contain the {PLACEHOLDER_PASSWORD} placeholder"
    );
}

/// No test gate, test credential, or token literal ships in tracked files (003 FR-016).
/// `docs/` and this file are excluded: they name the retired items.
#[test]
fn retired_gate_and_token_literals_are_absent() {
    // Header and payload of a compact JWS both start with `{"` in base64url.
    let jwt_shaped = r"eyJ[A-Za-z0-9_-]*\.eyJ";
    let (_, out) = git(&[
        "grep",
        "-nE",
        "-e",
        "TestGate",
        "-e",
        "DenyAll",
        "-e",
        "TEST_ADMIN_TOKEN",
        "-e",
        jwt_shaped,
        "--",
        ".",
        ":!docs",
        ":!tests/repo_hygiene.rs",
    ]);
    assert!(
        out.is_empty(),
        "retired gate or token literals found:\n{out}"
    );
}

/// Advisory IDs in the `[advisories] ignore` list of `.cargo/audit.toml`.
fn audit_ignores() -> Vec<String> {
    let text: String = read(".cargo/audit.toml")
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let Some(start) = text.find("ignore") else {
        return Vec::new();
    };
    let rest = &text[start..];
    let open = rest.find('[').expect("ignore must be an array");
    let close = rest.find(']').expect("ignore array must be closed");
    rest[open + 1..close]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(String::from)
        .collect()
}

/// Every audit exception has a row (advisory, crate, reason) in 003 plan §Supply chain (FR-041).
#[test]
fn audit_ignores_are_justified_in_the_plan() {
    let plan = read("docs/specs/003-security/plan.md");
    let section = plan
        .split_once("\n## Supply chain")
        .map(|(_, rest)| rest.split("\n## ").next().unwrap_or(rest))
        .expect("003 plan must have a Supply chain section");
    for id in audit_ignores() {
        let row = section
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with(&format!("| {id} |")))
            .unwrap_or_else(|| {
                panic!("{id} is ignored in .cargo/audit.toml without a plan §Supply chain row")
            });
        let cells: Vec<&str> = row
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        assert!(
            cells.len() >= 3,
            "the §Supply chain row for {id} must name the advisory, the crate, and why it does not apply"
        );
    }
}

//! The `paleo-accounts` operator tool (003 AC 1, AC 7, AC 24; FR-002–FR-004;
//! contracts/accounts-cli.md). Runs the built binary against the per-test database.

use std::io::Write;
use std::process::{Command, Stdio};

use paleo_api::security::token;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Row};

use crate::support::*;

const BIN: &str = env!("CARGO_BIN_EXE_paleo-accounts");
const PW: &str = "operator-chosen-pw-1";
const OTHER_PW: &str = "another-chosen-pw-2";

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// The harness URL with its database replaced by the one behind `opts`.
fn url_of(opts: &PgConnectOptions) -> String {
    let harness = std::env::var("DATABASE_URL").expect("run with the documented test command");
    let db = opts.get_database().expect("test database").to_string();
    let (base, query) = match harness.split_once('?') {
        Some((b, q)) => (b.to_string(), format!("?{q}")),
        None => (harness.clone(), String::new()),
    };
    let prefix = &base[..base.rfind('/').unwrap()];
    format!("{prefix}/{db}{query}")
}

/// Runs the tool with `DATABASE_URL = url` (when given) and `stdin` as raw bytes.
fn run_raw(url: Option<&str>, args: &[&str], stdin: &[u8]) -> Out {
    let mut cmd = Command::new(BIN);
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(url) = url {
        cmd.env("DATABASE_URL", url);
    }
    let mut child = cmd.spawn().expect("start paleo-accounts");
    let mut pipe = child.stdin.take().unwrap();
    // The tool may exit without reading (usage errors): ignore a closed pipe.
    let _ = pipe.write_all(stdin);
    drop(pipe);
    let out = child.wait_with_output().unwrap();
    Out {
        code: out.status.code().expect("exit code"),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn run(opts: &PgConnectOptions, args: &[&str], stdin: &str) -> Out {
    run_raw(Some(&url_of(opts)), args, stdin.as_bytes())
}

#[track_caller]
fn assert_ok(out: &Out, stdout: &str) {
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(out.stdout.trim_end(), stdout);
    assert_eq!(out.stderr, "");
}

#[track_caller]
fn assert_fails(out: &Out, code: i32, stderr: &str) {
    assert_eq!(out.code, code, "stdout: {} stderr: {}", out.stdout, out.stderr);
    assert_eq!(out.stderr.trim_end(), format!("paleo-accounts: {stderr}"));
    assert_eq!(out.stdout, "");
}

#[track_caller]
fn assert_usage(out: &Out) {
    assert_eq!(out.code, 2, "stderr: {}", out.stderr);
    assert!(out.stderr.starts_with("paleo-accounts: "), "{}", out.stderr);
    for command in [
        "create",
        "set-password",
        "grant-admin",
        "revoke-admin",
        "disable",
        "enable",
        "list",
    ] {
        assert!(out.stderr.contains(command), "{command}: {}", out.stderr);
    }
    assert_eq!(out.stdout, "");
}

struct Row_ {
    hash: String,
    role: Option<String>,
    status: String,
    version: i32,
}

async fn row(pool: &PgPool, username: &str) -> Option<Row_> {
    sqlx::query(
        "SELECT password_hash, role, status, credentials_version FROM accounts WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    .unwrap()
    .map(|r| Row_ {
        hash: r.get(0),
        role: r.get(1),
        status: r.get(2),
        version: r.get(3),
    })
}

async fn count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn empty_app(pool_opts: PgPoolOptions, opts: PgConnectOptions) -> TestApp {
    app_with(
        pool_opts,
        opts,
        SecuritySettings {
            seed: false,
            ..SecuritySettings::default()
        },
    )
    .await
}

// AC 1: an administrator created by the tool logs in and gets a usable token.
#[sqlx::test]
async fn tool_created_admin_logs_in(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = empty_app(pool_opts, opts.clone()).await;
    let out = run(&opts, &["create", "curator", "--admin"], &format!("{PW}\n"));
    assert_ok(&out, "created account 'curator' (admin)");

    let stored = row(&app.pool, "curator").await.unwrap();
    assert_eq!(stored.role.as_deref(), Some("admin"));
    assert_eq!(stored.status, "active");
    assert_eq!(stored.version, 1);
    assert!(
        stored.hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{}",
        stored.hash
    );

    let resp = login(&app, "curator", PW, peer(1)).await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    let access = resp.json()["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    let claims = token::verify(app.security.keys(), &access).expect("a valid token");
    assert_eq!((claims.sub.as_str(), claims.ver), ("curator", 1));

    let wrong = login(&app, "curator", OTHER_PW, peer(1)).await;
    assert_error(&wrong, 401, "INVALID_CREDENTIALS");
}

#[sqlx::test]
async fn account_without_role_logs_in_but_cannot_write(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = empty_app(pool_opts, opts.clone()).await;
    assert_ok(
        &run(&opts, &["create", "viewer-1"], PW),
        "created account 'viewer-1'",
    );
    assert_eq!(row(&app.pool, "viewer-1").await.unwrap().role, None);
    let resp = login(&app, "viewer-1", PW, peer(1)).await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    let access = resp.json()["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    let denied = send_as(
        &app,
        actix_web::http::Method::POST,
        "/api/v1/eras",
        serde_json::json!({}),
        Some(&access),
    )
    .await;
    assert_error(&denied, 403, "FORBIDDEN");
}

// FR-004 / AC 7: the password policy, checked before anything is stored.
#[sqlx::test]
async fn password_policy_bounds(pool: PgPool) {
    let opts = pool.connect_options().as_ref().clone();
    let too_short = "x".repeat(11);
    let too_long = "x".repeat(129);
    assert_fails(
        &run(&opts, &["create", "alice"], &too_short),
        1,
        "the password must be 12–128 characters (got 11).",
    );
    assert_fails(
        &run(&opts, &["create", "alice"], &too_long),
        1,
        "the password must be 12–128 characters (got 129).",
    );
    assert_fails(
        &run(&opts, &["create", "alice"], ""),
        1,
        "the password must be 12–128 characters (got 0).",
    );
    assert_fails(
        &run(&opts, &["create", "museum-curator"], "Museum-Curator"),
        1,
        "the password must not be the username.",
    );
    assert_eq!(count(&pool).await, 0, "a refused password stores nothing");

    for (name, pw) in [("alice", "x".repeat(12)), ("bob", "é".repeat(128))] {
        let out = run(&opts, &["create", name], &pw);
        assert_ok(&out, &format!("created account '{name}'"));
    }
    assert_eq!(count(&pool).await, 2);
}

// The password is the bytes up to EOF minus one trailing newline.
#[sqlx::test]
async fn stdin_framing(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = empty_app(pool_opts, opts.clone()).await;
    let url = url_of(&opts);
    for (name, input, password) in [
        ("lf-user", format!("{PW}\n").into_bytes(), PW.to_string()),
        ("crlf-user", format!("{PW}\r\n").into_bytes(), PW.to_string()),
        ("bare-user", PW.as_bytes().to_vec(), PW.to_string()),
        (
            "two-user",
            format!("{PW}\n\n").into_bytes(),
            format!("{PW}\n"),
        ),
    ] {
        let out = run_raw(Some(&url), &["create", name], &input);
        assert_ok(&out, &format!("created account '{name}'"));
        let resp = login(&app, name, &password, peer(1)).await;
        assert_eq!(resp.status.as_u16(), 200, "{name}: {}", resp.text());
    }
    let out = run_raw(Some(&url), &["create", "binary-user"], &[0xff; 20]);
    assert_fails(&out, 1, "the password must be valid UTF-8.");
    assert!(row(&app.pool, "binary-user").await.is_none());
}

// AC 24: duplicate and invalid usernames.
#[sqlx::test]
async fn usernames_are_validated(pool: PgPool) {
    let opts = pool.connect_options().as_ref().clone();
    assert_ok(
        &run(&opts, &["create", "alice"], PW),
        "created account 'alice'",
    );
    let before = row(&pool, "alice").await.unwrap().hash;
    assert_fails(
        &run(&opts, &["create", "alice", "--admin"], OTHER_PW),
        1,
        "account 'alice' already exists.",
    );
    let after = row(&pool, "alice").await.unwrap();
    assert_eq!((after.hash, after.role), (before, None), "left untouched");

    assert_fails(
        &run(&opts, &["create", "Alice"], PW),
        1,
        "'Alice' is not a valid username: use 2–64 of a-z, 0-9 and single inner hyphens.",
    );
    for bad in ["a", "-ab", "ab-", "a--b", "a_b", "a b", &"a".repeat(65)] {
        let out = run(&opts, &["create", bad], PW);
        assert_eq!(out.code, 1, "{bad}: {}", out.stderr);
        assert!(out.stderr.contains("is not a valid username"), "{bad}");
    }
    assert_eq!(count(&pool).await, 1);
}

#[sqlx::test]
async fn set_password_replaces_the_credential(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = empty_app(pool_opts, opts.clone()).await;
    assert_ok(
        &run(&opts, &["create", "alice", "--admin"], PW),
        "created account 'alice' (admin)",
    );
    let before = row(&app.pool, "alice").await.unwrap();
    let old_token = {
        let resp = login(&app, "alice", PW, peer(1)).await;
        resp.json()["data"]["access_token"]
            .as_str()
            .unwrap()
            .to_string()
    };

    assert_ok(
        &run(&opts, &["set-password", "alice"], OTHER_PW),
        "password changed for 'alice'",
    );
    let after = row(&app.pool, "alice").await.unwrap();
    assert_ne!(after.hash, before.hash);
    assert_eq!(after.version, before.version + 1, "FR-005 trigger");
    assert_eq!(after.role.as_deref(), Some("admin"));
    assert_error(&login(&app, "alice", PW, peer(1)).await, 401, "INVALID_CREDENTIALS");
    assert_eq!(login(&app, "alice", OTHER_PW, peer(1)).await.status.as_u16(), 200);
    let claims = token::verify(app.security.keys(), &old_token).unwrap();
    assert_eq!(claims.ver, before.version, "the old token carries the old version");

    assert_fails(
        &run(&opts, &["set-password", "alice"], "short"),
        1,
        "the password must be 12–128 characters (got 5).",
    );
    assert_eq!(row(&app.pool, "alice").await.unwrap().hash, after.hash);
    assert_fails(
        &run(&opts, &["set-password", "nobody"], PW),
        1,
        "no account named 'nobody'.",
    );
}

#[sqlx::test]
async fn role_and_status_commands(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = empty_app(pool_opts, opts.clone()).await;
    assert_ok(&run(&opts, &["create", "alice"], PW), "created account 'alice'");
    let v1 = row(&app.pool, "alice").await.unwrap();
    assert_eq!((v1.role.as_deref(), v1.version), (None, 1));

    assert_ok(&run(&opts, &["grant-admin", "alice"], ""), "granted admin to 'alice'");
    let granted = row(&app.pool, "alice").await.unwrap();
    assert_eq!((granted.role.as_deref(), granted.version), (Some("admin"), 1));

    assert_ok(&run(&opts, &["revoke-admin", "alice"], ""), "revoked admin from 'alice'");
    let revoked = row(&app.pool, "alice").await.unwrap();
    assert_eq!((revoked.role, revoked.version), (None, 1), "role changes keep tokens");

    assert_ok(&run(&opts, &["disable", "alice"], ""), "disabled 'alice'");
    let disabled = row(&app.pool, "alice").await.unwrap();
    assert_eq!((disabled.status.as_str(), disabled.version), ("disabled", 2));
    assert_error(&login(&app, "alice", PW, peer(1)).await, 401, "INVALID_CREDENTIALS");

    assert_ok(&run(&opts, &["enable", "alice"], ""), "enabled 'alice'");
    let enabled = row(&app.pool, "alice").await.unwrap();
    assert_eq!(
        (enabled.status.as_str(), enabled.version),
        ("active", 3),
        "tokens issued before the disable stay invalid"
    );
    assert_eq!(login(&app, "alice", PW, peer(1)).await.status.as_u16(), 200);

    for command in ["grant-admin", "revoke-admin", "disable", "enable"] {
        assert_fails(
            &run(&opts, &[command, "nobody"], ""),
            1,
            "no account named 'nobody'.",
        );
    }
}

#[sqlx::test]
async fn list_prints_one_line_per_account(pool: PgPool) {
    let opts = pool.connect_options().as_ref().clone();
    assert_ok(&run(&opts, &["list"], ""), "");
    for args in [
        &["create", "zed"][..],
        &["create", "alice", "--admin"],
        &["create", "mid-user"],
    ] {
        assert_eq!(run(&opts, args, PW).code, 0);
    }
    assert_eq!(run(&opts, &["disable", "mid-user"], "").code, 0);

    let out = run(&opts, &["list"], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stderr, "");
    let lines: Vec<Vec<&str>> = out
        .stdout
        .lines()
        .map(|l| l.split(' ').collect())
        .collect();
    assert_eq!(lines.len(), 3, "{}", out.stdout);
    let shape = [("alice", "admin", "active"), ("mid-user", "-", "disabled"), ("zed", "-", "active")];
    for (line, (user, role, status)) in lines.iter().zip(shape) {
        assert_eq!(line.len(), 5, "{line:?}");
        assert_eq!(&line[..3], &[user, role, status]);
        for stamp in &line[3..] {
            let ok = stamp.len() == 20
                && stamp.ends_with('Z')
                && stamp.as_bytes()[10] == b'T'
                && stamp[..4].chars().all(|c| c.is_ascii_digit());
            assert!(ok, "timestamp {stamp:?}");
        }
    }
    assert!(!out.stdout.contains("argon2"), "a hash is never printed");
}

#[test]
fn bad_arguments_print_usage_and_exit_2() {
    let url = Some("postgres://unused.invalid/none");
    let cases: &[&[&str]] = &[
        &[],
        &["frobnicate"],
        &["create"],
        &["create", "alice", "--bogus"],
        &["create", "alice", "bob"],
        &["set-password"],
        &["set-password", "alice", "--admin"],
        &["grant-admin"],
        &["disable", "a", "b"],
        &["list", "extra"],
    ];
    for args in cases {
        assert_usage(&run_raw(url, args, PW.as_bytes()));
    }
}

#[sqlx::test(migrations = false)]
async fn startup_checks_run_first(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let _ = pool_opts;
    let out = run(&opts, &["list"], "");
    assert_eq!(out.code, 1);
    assert!(out.stderr.starts_with("paleo-accounts: "), "{}", out.stderr);
    assert!(out.stderr.contains("migration(s) pending"), "{}", out.stderr);
    assert_eq!(out.stderr.lines().count(), 1);

    let out = run_raw(None, &["list"], b"");
    assert_fails(
        &out,
        1,
        "DATABASE_URL is not set. Set it to a PostgreSQL URL (see .env.example).",
    );
}

#[sqlx::test]
async fn nothing_secret_is_printed(pool: PgPool) {
    let opts = pool.connect_options().as_ref().clone();
    let url = url_of(&opts);
    let secret = url.split("://").nth(1).unwrap().split('@').next().unwrap().to_string();
    let mut seen = String::new();
    for (args, stdin) in [
        (&["create", "alice", "--admin"][..], PW),
        (&["create", "alice"], PW),
        (&["create", "bob"], "short"),
        (&["set-password", "alice"], OTHER_PW),
        (&["set-password", "nobody"], OTHER_PW),
        (&["list"], ""),
    ] {
        let out = run_raw(Some(&url), args, stdin.as_bytes());
        seen.push_str(&out.stdout);
        seen.push_str(&out.stderr);
    }
    let stored = row(&pool, "alice").await.unwrap().hash;
    for leak in [PW, OTHER_PW, stored.as_str(), url.as_str(), secret.as_str()] {
        assert!(!seen.contains(leak), "output leaks {leak:?}");
    }
}

// An interactive stdin is refused with the recipe instead of echoing (research R10).
#[sqlx::test]
async fn terminal_stdin_is_refused(pool: PgPool) {
    let opts = pool.connect_options().as_ref().clone();
    if Command::new("script").arg("--version").output().is_err() {
        eprintln!("skipped: no `script` to allocate a terminal");
        return;
    }
    let out = Command::new("script")
        .args(["-qec", &format!("'{BIN}' create alice"), "/dev/null"])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("DATABASE_URL", url_of(&opts))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    assert!(
        text.contains("read -rs PW && printf '%s' \"$PW\" | paleo-accounts create alice; unset PW"),
        "{text}"
    );
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert_eq!(count(&pool).await, 0);
}

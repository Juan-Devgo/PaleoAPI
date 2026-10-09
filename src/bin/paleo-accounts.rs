//! Operator tool for administrator accounts (Spec 003 FR-002–FR-004; contracts/accounts-cli.md).
//! Reads `DATABASE_URL` only; the password comes from standard input, never from arguments.

use std::io::{IsTerminal, Read};
use std::process::ExitCode;

use paleo_api::api::input::is_slug;
use paleo_api::config::database_options_from_env;
use paleo_api::db::{MIGRATOR, STARTUP_WAIT, connect_with_retry, verify_migrations};
use paleo_api::security::accounts::{self, ACTIVE, ADMIN};
use paleo_api::security::password;
use sqlx::{Connection, PgConnection};

const USAGE: &str = "usage:
  paleo-accounts create <username> [--admin]   (password on stdin)
  paleo-accounts set-password <username>       (password on stdin)
  paleo-accounts grant-admin <username>
  paleo-accounts revoke-admin <username>
  paleo-accounts disable <username>
  paleo-accounts enable <username>
  paleo-accounts list";

const DISABLED: &str = "disabled";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Create { username: String, admin: bool },
    SetPassword(String),
    GrantAdmin(String),
    RevokeAdmin(String),
    Disable(String),
    Enable(String),
    List,
}

impl Command {
    fn parse(args: &[String]) -> Option<Self> {
        let (name, rest) = args.split_first()?;
        let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
        let user = |rest: &[&str]| match rest {
            [u] if !u.starts_with("--") => Some((*u).to_string()),
            _ => None,
        };
        Some(match name.as_str() {
            "create" => match rest.as_slice() {
                [u] if !u.starts_with("--") => Self::Create {
                    username: (*u).into(),
                    admin: false,
                },
                [u, "--admin"] | ["--admin", u] if !u.starts_with("--") => Self::Create {
                    username: (*u).into(),
                    admin: true,
                },
                _ => return None,
            },
            "set-password" => Self::SetPassword(user(&rest)?),
            "grant-admin" => Self::GrantAdmin(user(&rest)?),
            "revoke-admin" => Self::RevokeAdmin(user(&rest)?),
            "disable" => Self::Disable(user(&rest)?),
            "enable" => Self::Enable(user(&rest)?),
            "list" if rest.is_empty() => Self::List,
            _ => return None,
        })
    }

    fn username(&self) -> Option<&str> {
        match self {
            Self::Create { username, .. } => Some(username),
            Self::SetPassword(u)
            | Self::GrantAdmin(u)
            | Self::RevokeAdmin(u)
            | Self::Disable(u)
            | Self::Enable(u) => Some(u),
            Self::List => None,
        }
    }

    fn reads_password(&self) -> bool {
        matches!(self, Self::Create { .. } | Self::SetPassword(_))
    }

    /// The recipe printed when standard input is a terminal.
    fn recipe(&self) -> String {
        let call = match self {
            Self::Create {
                username,
                admin: true,
            } => format!("create {username} --admin"),
            Self::Create { username, .. } => format!("create {username}"),
            Self::SetPassword(username) => format!("set-password {username}"),
            _ => String::new(),
        };
        format!(
            "refusing to read a password from a terminal. Run:\n  \
             read -rs PW && printf '%s' \"$PW\" | paleo-accounts {call}; unset PW"
        )
    }
}

/// Why the tool stops: the message (without prefix) and the exit code.
struct Failure(String, u8);

fn fail(message: impl Into<String>) -> Failure {
    Failure(message.into(), 1)
}

/// Reads all of standard input, drops one trailing `\n` or `\r\n`.
fn read_password() -> Result<String, Failure> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|_| fail("could not read the password from standard input."))?;
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    String::from_utf8(bytes).map_err(|_| fail("the password must be valid UTF-8."))
}

fn db_error(err: &sqlx::Error) -> Failure {
    match err.as_database_error().and_then(|e| e.code()) {
        Some(code) => fail(format!("database error (SQLSTATE {code}).")),
        None => fail("database error."),
    }
}

async fn connect() -> Result<PgConnection, Failure> {
    let opts = database_options_from_env(|key| std::env::var(key).ok())
        .map_err(|e| fail(e.to_string()))?;
    let mut conn = connect_with_retry(&opts, STARTUP_WAIT)
        .await
        .map_err(|e| fail(e.to_string()))?;
    // Warnings are the API's concern; the tool only refuses a schema it cannot trust.
    if let Err(e) = verify_migrations(&mut conn, &MIGRATOR).await {
        let _ = conn.close().await;
        return Err(fail(e.to_string()));
    }
    Ok(conn)
}

fn no_account(username: &str) -> Failure {
    fail(format!("no account named '{username}'."))
}

async fn execute(command: &Command, password: Option<String>) -> Result<String, Failure> {
    let mut conn = connect().await?;
    let result = apply(&mut conn, command, password).await;
    let _ = conn.close().await;
    result
}

async fn apply(
    conn: &mut PgConnection,
    command: &Command,
    password: Option<String>,
) -> Result<String, Failure> {
    let hashed = |password: Option<String>| {
        password::hash(&password.unwrap_or_default()).map_err(|e| fail(format!("{e}.")))
    };
    match command {
        Command::Create { username, admin } => {
            let hash = hashed(password)?;
            let role = admin.then_some(ADMIN);
            match accounts::insert(&mut *conn, username, &hash, role).await {
                Ok(()) if *admin => Ok(format!("created account '{username}' (admin)")),
                Ok(()) => Ok(format!("created account '{username}'")),
                Err(e)
                    if e.as_database_error().and_then(|d| d.constraint())
                        == Some("accounts_pk") =>
                {
                    Err(fail(format!("account '{username}' already exists.")))
                }
                Err(e) => Err(db_error(&e)),
            }
        }
        Command::SetPassword(username) => {
            let hash = hashed(password)?;
            match accounts::set_password(&mut *conn, username, &hash).await {
                Ok(true) => Ok(format!("password changed for '{username}'")),
                Ok(false) => Err(no_account(username)),
                Err(e) => Err(db_error(&e)),
            }
        }
        Command::GrantAdmin(username) => changed(
            accounts::set_role(&mut *conn, username, Some(ADMIN)).await,
            username,
        )
        .map(|()| format!("granted admin to '{username}'")),
        Command::RevokeAdmin(username) => changed(
            accounts::set_role(&mut *conn, username, None).await,
            username,
        )
        .map(|()| format!("revoked admin from '{username}'")),
        Command::Disable(username) => changed(
            accounts::set_status(&mut *conn, username, DISABLED).await,
            username,
        )
        .map(|()| format!("disabled '{username}'")),
        Command::Enable(username) => changed(
            accounts::set_status(&mut *conn, username, ACTIVE).await,
            username,
        )
        .map(|()| format!("enabled '{username}'")),
        Command::List => {
            let rows = accounts::list(&mut *conn).await.map_err(|e| db_error(&e))?;
            Ok(rows
                .iter()
                .map(|a| {
                    format!(
                        "{} {} {} {} {}",
                        a.username,
                        a.role.as_deref().unwrap_or("-"),
                        a.status,
                        a.credentials_changed_at,
                        a.created_at
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }
}

fn changed(result: sqlx::Result<bool>, username: &str) -> Result<(), Failure> {
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(no_account(username)),
        Err(e) => Err(db_error(&e)),
    }
}

/// Everything before the database is touched: arguments, password, validation.
fn prepare(args: &[String]) -> Result<(Command, Option<String>), Failure> {
    let command = Command::parse(args).ok_or_else(|| Failure(USAGE.into(), 2))?;
    let password = if command.reads_password() {
        if std::io::stdin().is_terminal() {
            return Err(Failure(command.recipe(), 2));
        }
        Some(read_password()?)
    } else {
        None
    };
    if let Some(username) = command.username()
        && !is_slug(username)
    {
        return Err(fail(format!(
            "'{username}' is not a valid username: use 2–64 of a-z, 0-9 and single inner hyphens."
        )));
    }
    if let (Some(username), Some(password)) = (command.username(), &password) {
        password::check_policy(username, password).map_err(|e| fail(e.to_string()))?;
    }
    Ok((command, password))
}

#[actix_web::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match prepare(&args) {
        Ok((command, password)) => execute(&command, password).await,
        Err(failure) => Err(failure),
    };
    match outcome {
        Ok(text) => {
            if !text.is_empty() {
                println!("{text}");
            }
            ExitCode::SUCCESS
        }
        Err(Failure(message, code)) => {
            eprintln!("paleo-accounts: {message}");
            ExitCode::from(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Option<Command> {
        Command::parse(&args.iter().map(|a| (*a).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn commands_parse() {
        let create = |admin| Command::Create {
            username: "alice".into(),
            admin,
        };
        assert_eq!(parse(&["create", "alice"]), Some(create(false)));
        assert_eq!(parse(&["create", "alice", "--admin"]), Some(create(true)));
        assert_eq!(parse(&["create", "--admin", "alice"]), Some(create(true)));
        assert_eq!(
            parse(&["set-password", "alice"]),
            Some(Command::SetPassword("alice".into()))
        );
        assert_eq!(parse(&["list"]), Some(Command::List));
        assert_eq!(
            parse(&["revoke-admin", "alice"]),
            Some(Command::RevokeAdmin("alice".into()))
        );
    }

    #[test]
    fn bad_shapes_do_not_parse() {
        for args in [
            &[][..],
            &["nope"],
            &["create"],
            &["create", "--admin"],
            &["create", "a", "b"],
            &["create", "a", "--other"],
            &["set-password", "a", "--admin"],
            &["disable"],
            &["enable", "a", "b"],
            &["list", "x"],
        ] {
            assert_eq!(parse(args), None, "{args:?}");
        }
    }

    #[test]
    fn recipe_names_the_command() {
        let c = parse(&["create", "alice", "--admin"]).unwrap();
        assert!(
            c.recipe()
                .contains("paleo-accounts create alice --admin; unset PW")
        );
        let c = parse(&["set-password", "bob"]).unwrap();
        assert!(c.recipe().contains(
            "read -rs PW && printf '%s' \"$PW\" | paleo-accounts set-password bob; unset PW"
        ));
    }
}

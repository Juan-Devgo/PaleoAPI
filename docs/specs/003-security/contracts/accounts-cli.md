# Contract — `paleo-accounts` operator tool (FR-002–FR-004, research R10)

Binary `paleo-accounts` in this crate (`cargo run --bin paleo-accounts -- <command>`, or the built binary).

## Environment and startup

- Reads `DATABASE_URL` only. The same checks as the API run first (Spec 001 plan §Startup contract steps 1–4: URL valid, connect with the startup wait, no pending or failed migrations); a failure prints the same message and exits `1`. Never applies migrations.
- Never prints `DATABASE_URL`, a password, or a hash.

## Commands

| Command | Effect | Reads password |
|---|---|---|
| `create <username> [--admin]` | Inserts an `active` account, with the `admin` role if `--admin` | yes |
| `set-password <username>` | Replaces the hash; the trigger invalidates earlier tokens (FR-005) | yes |
| `grant-admin <username>` | `role = 'admin'` | no |
| `revoke-admin <username>` | `role = NULL`; earlier tokens get `403` on writes | no |
| `disable <username>` | `status = 'disabled'`; earlier tokens get `401` | no |
| `enable <username>` | `status = 'active'`; tokens issued before the disable stay invalid | no |
| `list` | One line per account: `username role status credentials_changed_at created_at` (role `-` when none), ordered by username | no |

## Password input

- Read from standard input: all bytes up to EOF, one trailing `\n` (or `\r\n`) removed. Must be valid UTF-8.
- If standard input is a terminal, the tool refuses (exit `2`) and prints the recipe instead of echoing the password:

  ```bash
  read -rs PW && printf '%s' "$PW" | paleo-accounts set-password alice; unset PW
  ```

- Policy (research R9), checked before hashing: 12–128 characters; not equal to the username ignoring case.
- Hash: Argon2id with the API's parameters (research R8).

## Outcomes

| Case | stderr (prefix `paleo-accounts: `) | Exit |
|---|---|---|
| Success | stdout: `created account 'alice' (admin)`, `password changed for 'alice'`, `granted admin to 'alice'`, … | 0 |
| Username not a slug | `'Alice' is not a valid username: use 2–64 of a-z, 0-9 and single inner hyphens.` | 1 |
| Username taken (`23505 accounts_pk`) | `account 'alice' already exists.` | 1 |
| Unknown account (0 rows) | `no account named 'alice'.` | 1 |
| Password too short / too long | `the password must be 12–128 characters (got 11).` | 1 |
| Password equals username | `the password must not be the username.` | 1 |
| Stdin is a terminal | the recipe above | 2 |
| Unknown command, missing or extra argument | usage text listing the commands | 2 |
| Database or startup error | the Spec 001 startup message | 1 |

Any other database error prints `database error (SQLSTATE <code>).` and exits `1`.

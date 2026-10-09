# Quickstart & Validation — Spec 003 Security

Run every command from the repository root. Spec 001 quickstart §1–§3 still apply; this file lists only what Spec 003 adds.

## 1. Prerequisites

| Tool | Version | Install | Check |
|---|---|---|---|
| cargo-audit | 0.22.x | `cargo install cargo-audit --locked` (needs a C compiler, research R18) | `cargo audit --version` |

## 2. Configure the environment

```bash
openssl rand -base64 48        # paste the output as JWT_SECRET in .env (replacing the change-me placeholder)
set -a; . ./.env; set +a
```

Optional variables and defaults: plan §Configuration. Changing `JWT_SECRET` logs every admin out.

## 3. Build the schema

```bash
sqlx migrate run
cargo sqlx prepare --check -- --all-targets
```

**Expected:** applies the `accounts` migration (seven in total). The API refuses to start until it is applied (001-plan §Startup contract). The `.sqlx/` check passes, because the committed metadata covers the account queries (plan §Technical Context Queries).

## 4. Create an admin (US2, AC 1)

```bash
read -rs PW && printf '%s' "$PW" | cargo run -q --bin paleo-accounts -- create alice --admin; unset PW
cargo run -q --bin paleo-accounts -- list
```

**Expected:** `created account 'alice' (admin)`; `list` shows `alice admin active …`. Other commands and outcomes: contracts/accounts-cli.md. Running the tool without the `read -rs` pipe refuses and prints the recipe.

## 5. Log in and write (US1, AC 1–2)

```bash
cargo run          # terminal 1; security events appear on stdout as JSON lines
```

```bash
read -rs PW
TOKEN=$(printf '{"username":"alice","password":"%s"}' "$PW" \
  | curl -s -X POST http://127.0.0.1:8000/api/v1/auth/login -H 'Content-Type: application/json' --data-binary @- \
  | sed -E 's/.*"access_token":"([^"]+)".*/\1/'); unset PW
curl -si -X POST http://127.0.0.1:8000/api/v1/eras -H 'Content-Type: application/json' \
  -d '{"id":"demo-era","name":"Demo","start_mya":9000,"end_mya":8000}'             # 401 + WWW-Authenticate
curl -si -X POST http://127.0.0.1:8000/api/v1/eras -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"id":"demo-era","name":"Demo","start_mya":9000,"end_mya":8000}'   # 201 + Cache-Control: no-store
curl -si -X DELETE http://127.0.0.1:8000/api/v1/eras/demo-era -H "Authorization: Bearer $TOKEN"        # 204
```

(The `printf` JSON assumes the password has no `"` or `\`.) Then `paleo-accounts revoke-admin alice` → the same write returns `403`; `paleo-accounts disable alice` → `401`.

## 6. Rate limits and proxies (US3, US4)

```bash
for i in $(seq 1 130); do curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8000/api/v1/eras; done | sort | uniq -c
curl -si http://127.0.0.1:8000/api/v1/eras | grep -iE '^(ratelimit|ratelimit-policy|retry-after):'
```

**Expected:** about 120 `200` and the rest `429` (burst, research R2); after `Retry-After` seconds requests succeed again. Headers: `RateLimit-Policy: "general";q=120;w=12`.

Behind a reverse proxy, set `TRUSTED_PROXIES` to the proxy's address (for example `TRUSTED_PROXIES=10.0.0.5` or `172.18.0.0/16`) and make the proxy set `X-Forwarded-For`. Without it, every client shares the proxy's limit. The proxy's upstream keep-alive must be shorter than 15 s (research R11).

## 7. Tests and quality gates (US7)

```bash
DATABASE_URL="$TEST_DATABASE_URL" cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo sqlx prepare --check -- --all-targets
cargo audit
```

**Expected:** all pass. `cargo audit` reports no vulnerabilities except the ignored ones listed in plan §Supply chain (RUSTSEC-2023-0071).

### 7.1 AC 12 — sustained load at the default limit

```bash
DATABASE_URL="$TEST_DATABASE_URL" cargo test --release --test api -- --ignored sustained_default_rate
```

**Expected:** after 10 minutes at 10 requests per second from one client, zero `429` and read p95 < 50 ms.

### 7.2 AC 23 — the gate fails on a vulnerable dependency

```bash
cargo audit --file tests/fixtures/audit/Cargo.lock; echo "exit $?"
```

**Expected:** reports RUSTSEC-2021-0003 (`smallvec` 1.6.0) and `exit 1`.

## 8. Reset

- Lost admin password: `paleo-accounts set-password <username>` (earlier tokens stop working).
- Log everyone out: change `JWT_SECRET` and restart.
- Rate-limit state: restart the API (research R1).
- Database: 001 quickstart §7.

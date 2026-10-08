# Data Model — Spec 003 Security

**Date:** 2026-10-07. Naming and shared rules follow Spec 001 data-model §1. Snippets are normative only where they name a constraint, a trigger, or a formula.

## 1. Table `accounts` (FR-001, FR-005, FR-006)

Migration **`accounts`** (the seventh, after `query_indexes`), with a reversible `down` that drops the table and its function.

| Column | Type | Rule | Constraint |
|---|---|---|---|
| `username` | `text NOT NULL` | slug, same expression as Spec 001 data-model §1.2 `id` | `accounts_username_ck` |
| `username` | — | primary key | `accounts_pk` |
| `password_hash` | `text NOT NULL` | Argon2id PHC string: `password_hash ~ '^\$argon2id\$v=19\$m=[0-9]+,t=[0-9]+,p=[0-9]+\$[A-Za-z0-9+/]+\$[A-Za-z0-9+/]+$'` | `accounts_password_hash_ck` |
| `role` | `text NULL` | `role = 'admin'` (NULL = no role) | `accounts_role_ck` |
| `status` | `text NOT NULL DEFAULT 'active'` | `status IN ('active', 'disabled')` | `accounts_status_ck` |
| `credentials_version` | `integer NOT NULL DEFAULT 1` | `credentials_version >= 1` | `accounts_credentials_version_ck` |
| `credentials_changed_at` | `timestamptz NOT NULL DEFAULT now()` | set by the trigger below | — |
| `created_at` | `timestamptz NOT NULL DEFAULT now()` | — | — |

- **Trigger `accounts_credentials_changed_trg`** (research R7): `BEFORE UPDATE OF password_hash, status ON accounts FOR EACH ROW WHEN (OLD.password_hash IS DISTINCT FROM NEW.password_hash OR OLD.status IS DISTINCT FROM NEW.status) EXECUTE FUNCTION accounts_credentials_changed_fn()`. The function sets `NEW.credentials_version := OLD.credentials_version + 1` and `NEW.credentials_changed_at := now()`. Role changes do not fire it.
- **TRUNCATE guard:** `accounts_no_truncate_trg` → `paleo_forbid_truncate()` (Spec 001 data-model §1.2), so a plain `TRUNCATE accounts` raises `23514 accounts_no_truncate_ck`.
- No cross-row rule, so no isolation guard. No index beyond the primary key: both lookups are by `username`.
- The password check is shape-only: the database never sees a plaintext and cannot verify strength; FR-004 is enforced by the operator tool (research R9).

### 1.1 Queries

| Use | Statement (bound parameters) |
|---|---|
| Login | `SELECT password_hash, status, credentials_version FROM accounts WHERE username = $1` |
| Write check (FR-015) | `SELECT role, status, credentials_version FROM accounts WHERE username = $1` |
| Operator tool | `INSERT … (username, password_hash, role)`; `UPDATE … SET password_hash` / `role` / `status WHERE username = $1` (0 rows → "no such account"); `SELECT username, role, status, credentials_changed_at, created_at … ORDER BY username` |

## 2. Access token (FR-011, FR-013)

| Claim | Value | Validation |
|---|---|---|
| `sub` | username | required; the account must exist |
| `iss`, `aud` | `"paleo-api"` | required, exact |
| `iat` | issue time (Unix seconds) | required field |
| `nbf` | `= iat` | required; rejected if `> now + 60` |
| `exp` | `iat + 3600` | required; rejected if `< now − 60` |
| `ver` | `accounts.credentials_version` at issue | required; must equal the current value |

JOSE header `{"alg":"HS256","typ":"JWT"}`; only `HS256` accepted (research R6). Extra claims are ignored. Request extension after a successful check: `AdminIdentity { username }`.

## 3. In-memory limiter state (research R1)

Owned by one `Security` value shared by all workers. Nothing here is persisted.

### 3.1 Client key (FR-019)

`ClientKey = V4([u8; 4]) | V6([u8; 8])`: an IPv4 address, or the first 64 bits of an IPv6 address. IPv4-mapped IPv6 (`::ffff:a.b.c.d`) becomes `V4`. Resolution: research R5.

### 3.2 General limit: GCRA (FR-021, FR-023, FR-024)

Per client: `tat: Instant` (theoretical arrival time). With `T = 60 s / RATE_LIMIT_PER_MINUTE` and `τ = T × RATE_LIMIT_BURST`, a request at `now`:

```
base = max(tat, now)
if base + T − now > τ:  reject;  Retry-After = max(1, ceil(base + T − τ − now));  state unchanged
else:                   accept;  tat = base + T
r = floor((τ − (tat − now)) / T)        # RateLimit r (0 on reject)
t = ceil(tat − now)                     # RateLimit t, seconds until the bucket is full
```

- A fresh client starts at `tat = now`, so `r = BURST − 1` after its first request.
- An entry with `tat ≤ now` equals a fresh one and may be dropped. Each shard drops such entries when it reaches its share of the capacity.
- **Shards:** 16 × `Mutex<HashMap<ClientKey, Entry>>`, shard = hash of the key. **Capacity:** 200,000 entries (12,500 per shard). If a shard is still full after dropping, a new client gets `503` (research R14).
- `Entry` also holds `last_logged: Option<Instant>` so `rate_limited` is logged at most once per `w` seconds per client (FR-039).

### 3.3 Login limits (FR-022)

| Table | Key | Value | Recorded when |
|---|---|---|---|
| per client | `ClientKey` | instants of the last ≤ `LOGIN_LIMIT_PER_CLIENT` login requests | every login request accepted by this check |
| per username | `u64` keyed SipHash of the submitted username | instants of the last ≤ `LOGIN_LIMIT_PER_USERNAME` failures, plus `pending` (reserved, unfinished attempts) | reserve before hashing; on `401` add an instant; on `200` or `503` drop the reservation |

- Full means `instants within the window + pending ≥ limit`. Reject → `Retry-After = max(1, ceil(oldest + window − now))`.
- Instants older than the window are dropped on access; empty entries are removed (per-client: only once both `logged` slots are also older than the window). Capacity 100,000 keys per table, otherwise `503`.
- `login_locked` is logged when a failure fills the per-username log.
- **`rate_limited` dedupe (FR-039):** the per-client entry also holds `logged: [Option<Instant>; 2]`, one slot per reason (`login_client`, `login_username`). A rejection logs `rate_limited` only when its slot is `None` or older than the window, then sets the slot to `now`: each reason at most once per client per window. The `login_username` slot sits on the per-client entry, never on the per-username entry; that entry always exists at the username check because plan §Login step 1 records the request first.

### 3.4 Concurrency permits (FR-030, FR-032)

`Permits { in_use: AtomicUsize, max }`: `try_acquire()` returns a guard that releases on drop, or `None` at the bound (no waiting). Two instances: requests (`MAX_CONCURRENT_REQUESTS`), password verifications (`MAX_CONCURRENT_PASSWORD_HASHES`).

### 3.5 Clock

`Clock` trait (`now() -> Instant`), injected into the limiter tables. Production uses `Instant::now`; tests use a manual clock (base instant + atomic offset) to step time without sleeping.

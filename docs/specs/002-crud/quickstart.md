# Quickstart & Validation — Spec 002 Core CRUD

Run every command from the repository root. Prerequisites, environment, database start, and migrations are [001 quickstart](../001-database/quickstart.md) §1–§3. New in this feature: `.env` must contain `SQLX_OFFLINE=true` (copy it from `.env.example`; research R3).

## 1. Build and run

```bash
set -a; . ./.env; set +a
cargo run                                   # listens on 127.0.0.1:8000
```

Writes are disabled in every build until the security spec ships (plan §Constitution Check II, research R4), so manual checks need rows inserted with SQL:

```bash
docker compose exec -T db psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v ON_ERROR_STOP=1 <<'SQL'
BEGIN;
INSERT INTO eras VALUES ('mesozoic', 'Mesozoic', 251.902, 66);
INSERT INTO periods (id, era_id, name, start_mya, end_mya) VALUES ('cretaceous', 'mesozoic', 'Cretaceous', 145, 66);
INSERT INTO domains VALUES ('eukaryota', 'Eukaryota');
INSERT INTO kingdoms (id, domain_id, name) VALUES ('animalia', 'eukaryota', 'Animalia');
INSERT INTO phyla (id, kingdom_id, name) VALUES ('chordata', 'animalia', 'Chordata');
INSERT INTO classes (id, phylum_id, name) VALUES ('reptilia', 'chordata', 'Reptilia');
INSERT INTO orders (id, class_id, name) VALUES ('saurischia', 'reptilia', 'Saurischia');
INSERT INTO families (id, order_id, name) VALUES ('tyrannosauridae', 'saurischia', 'Tyrannosauridae');
INSERT INTO genera (id, family_id, name) VALUES ('tyrannosaurus', 'tyrannosauridae', 'Tyrannosaurus');
INSERT INTO species (id, genus_id, name, scientific_name, diet, description, discovery_year)
  VALUES ('tyrannosaurus-rex', 'tyrannosaurus', 'Tyrannosaurus', 'Tyrannosaurus rex', 'carnivore', 'Large theropod.', 1905);
INSERT INTO species_periods VALUES ('tyrannosaurus-rex', 'cretaceous');
COMMIT;
SQL
```

Remove them afterwards with `docker compose down -v` (001 quickstart §7) or by deleting the rows in reverse order.

## 2. Manual scenarios

`B=http://127.0.0.1:8000/api/v1`. Shapes and codes are in [contracts/openapi.yaml](./contracts/openapi.yaml).

| # | Command | Expected | Spec |
|---|---|---|---|
| 1 | `curl -si $B/species` | `200`; one full card (taxonomy, periods with era); `pagination` `1/20/1/1`; headers `ETag`, `Cache-Control: public, max-age=300`, `Access-Control-Allow-Origin: *`, `Access-Control-Expose-Headers: ETag` | AC 6.1.1, 6.1.6, 6.1.8, 6.5.6 |
| 2 | `curl -si -H "If-None-Match: <ETag from 1>" $B/species` | `304`, no body, same `ETag`, `Cache-Control`, CORS headers | AC 6.1.6 |
| 3 | `curl -s "$B/species?limit=101"`, `"$B/species?page=1&page=2"`, `"$B/species?q=re"`, `"$B/species?sort=size"` | `400 INVALID_QUERY_PARAMETER`; the sort message lists allowed fields | AC 6.1.2, 6.1.4, 6.5.8 |
| 4 | `curl -s "$B/species?q=REX"` | the T. rex card | AC 6.5.8 |
| 5 | `curl -s "$B/species?page=9"` | `200`, `data: []`, `total_items: 1`, `total_pages: 1` | AC 6.1.3 |
| 6 | `curl -s $B/eras/unknown/periods`; `curl -s $B/species/NOT_A_SLUG` | `404 ERA_NOT_FOUND`; `404 SPECIES_NOT_FOUND` | AC 6.2.7, spec §4.2 |
| 7 | `curl -s $B/specie`; `curl -s $B/taxonomy/domains/eukaryota/genera` | `404 ROUTE_NOT_FOUND` | AC 6.1.15, 6.3.6 |
| 8 | `curl -si -X PUT $B/eras` | `405 METHOD_NOT_ALLOWED`, `Allow: GET, POST` | AC 6.1.15 |
| 9 | `curl -si -X OPTIONS -H "Origin: http://x" -H "Access-Control-Request-Method: GET" -H "Access-Control-Request-Headers: if-none-match" $B/species` | `204` with `Access-Control-Allow-Origin: *`, `Allow-Methods: GET`, `Allow-Headers: If-None-Match` | AC 6.1.8 |
| 10 | `curl -si -X POST -H "Authorization: Bearer anything" -H "Content-Type: application/json" -d '{"id":"x"}' $B/eras`; same with `-X DELETE $B/eras/nope` | `401 UNAUTHORIZED` in both (deny-all, even for unknown ids) | AC 6.1.9 |
| 11 | `curl -si $B/eras` | era `start_mya` is `251.902`, `end_mya` is `66` | research R1 |

## 3. Automated validation

```bash
set -a; . ./.env; set +a
DATABASE_URL="$TEST_DATABASE_URL" cargo test --test api                    # HTTP contract, every AC in spec §6
DATABASE_URL="$TEST_DATABASE_URL" cargo test --test api query_plans        # every read and write-lookup SQL is index-served (001 fixture)
DATABASE_URL="$TEST_DATABASE_URL" cargo test --test api performance        # p95 < 50 ms per read shape (001 fixture)
DATABASE_URL="$TEST_DATABASE_URL" cargo test --test api openapi            # docs/openapi.yaml matches the routes
DATABASE_URL="$TEST_DATABASE_URL" cargo test                               # everything, including Spec 001 suites
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo sqlx prepare --check -- --all-targets   # uses the dev DATABASE_URL (migrated); .sqlx/ is current
```

**Expected:** all pass. `performance` prints each shape's p95; record them in the PR description. Write behavior (create, update, delete, `409`, `422` details, check order) is validated only by the `api` suite, which installs the test-only gate (plan §Constitution Check II, research R4).

## 4. Troubleshooting

| Symptom | Fix |
|---|---|
| `error: error communicating with database` / `relation "eras" does not exist` while compiling | `SQLX_OFFLINE=true` is missing from the environment; re-export `.env` |
| `SQLX_OFFLINE=true but there is no cached data for this query` | a query changed: run `cargo sqlx prepare -- --all-targets` with the dev `DATABASE_URL` and commit `.sqlx/` |
| `too many clients` during tests | 001 quickstart §5 (`--test-threads=8`) |

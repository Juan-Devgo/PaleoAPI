# PaleoAPI

A public, high-performance REST API with data about ancient species, built in Rust with actix-web and PostgreSQL.

## Getting started

### Prerequisites

- Rust ≥ 1.94 (edition 2024)
- Docker Engine with Compose v2 (your user can run `docker ps`)
- sqlx-cli 0.9: `cargo install sqlx-cli --no-default-features --features postgres`

### 1. Configure the environment

```bash
cp .env.example .env          # then replace every "change-me" placeholder in .env
set -a; . ./.env; set +a      # export the variables into this shell (the API does not read .env itself)
```

`.env` is git-ignored. Never commit it. If port 5432 is taken on your machine, change `POSTGRES_PORT` and the port in both URLs.

### 2. Start the database and build the schema

```bash
docker compose up -d --wait db     # starts PostgreSQL 18 and waits until it is ready
sqlx migrate run                   # applies every migration in migrations/
```

Running `sqlx migrate run` again applies nothing. Schema changes are made only through new migrations.

### 3. Run the API

```bash
cargo run                          # listens on 127.0.0.1:8000
```

The API never applies migrations. It refuses to start, with a message saying why, when `DATABASE_URL` is missing, the database is unreachable, or migrations are pending.

### 4. Run the tests

```bash
DATABASE_URL="$TEST_DATABASE_URL" cargo test   # every database test gets its own throwaway database
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

`TEST_DATABASE_URL` points at the `postgres` maintenance database, so tests never touch your development data. Do not run plain `cargo test` with the development `DATABASE_URL`; the run fails, and [quickstart §5](docs/specs/001-database/quickstart.md#5-run-the-tests-us3-fr-013fr-015) explains the cleanup.

### 5. Reset

```bash
docker compose down -v             # removes the container and the data volume
```

## Documentation

- [Constitution](docs/constitution.md): mission and non-negotiable principles
- [Database quickstart and validation](docs/specs/001-database/quickstart.md)
- [AGENTS.md](AGENTS.md): workflow, conventions, and quality gates for contributors

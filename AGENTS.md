# AGENTS.md

Guidance for AI agents and contributors working on PaleoAPI.

**Read [`docs/constitution.md`](docs/constitution.md) first.** It defines the mission and non-negotiable principles. This file must stay consistent with it.

---

## 1. Workflow: Spec-Driven Development

- No code is written without an approved spec.
- Flow: **Constitution → Spec → Plan → Tasks → Implementation**.
- Specs describe *what* and *why*; plans describe *how*. Specs must not contain implementation details.
- Specs live in `docs/specs/<NNN-feature-name>/` (`spec.md`, `plan.md`, `tasks.md`).
- Any ambiguity in a spec is marked `[NEEDS CLARIFICATION]` and resolved with the user before planning.
- Every spec and plan includes a **Constitution Check** confirming compliance and justifying any deviation.

For each task:

1. Read the spec, plan, and task.
2. Write failing tests from the acceptance criteria.
3. Implement the minimum code to pass them.
4. Run the quality gates (section 5).
5. Mark the task done in `tasks.md`.

---

## 2. Tech Stack

| Area | Decision |
|---|---|
| Language | Rust (edition 2024), stable toolchain |
| Web framework | `actix-web` 4 |
| Serialization | `serde` / `serde_json` |
| Database | PostgreSQL |
| Data access | `sqlx` (async, compile-time checked queries). No ORM. |
| Migrations | `sqlx migrate`, versioned in `migrations/`. Schema changes only via migrations. |
| Admin auth | JWT (Bearer token) issued by a login endpoint; passwords hashed with Argon2 |
| Images | Stored as external URLs (`image_url`). The API does not store or serve image files. |
| Runtime | Docker + `docker-compose` (API + PostgreSQL) |
| Configuration | Environment variables only (12-factor). No secrets in the repository. |

Do not add dependencies outside this table without justifying them in the plan.

---

## 3. Commands

```bash
cargo install sqlx-cli --no-default-features --features postgres   # once: sqlx-cli 0.9
cp .env.example .env                       # once: then replace every "change-me" placeholder
set -a; . ./.env; set +a                   # export the variables (the API does not read .env)
docker compose up -d --wait db             # start PostgreSQL and wait until it is ready
sqlx migrate run                           # apply migrations (the API never applies them)
cargo run                                  # run the API
DATABASE_URL="$TEST_DATABASE_URL" cargo test   # unit + integration tests
cargo fmt --check                          # formatting
cargo clippy --all-targets -- -D warnings  # lints
```

Never run plain `cargo test` with the development `DATABASE_URL`: the test harness would write its bookkeeping schema into the development database (the run fails; cleanup in `docs/specs/001-database/quickstart.md` §5).

---

## 4. API Conventions

- All endpoints live under `/api/v1`. Resources use plural nouns; nesting expresses hierarchy (`/api/v1/eras/{era_id}/periods`).
- Breaking changes require a new version (`/api/v2`).
- `GET` is public. `POST`, `PUT`/`PATCH`, `DELETE` require an admin JWT (`401` if missing/invalid, `403` if not admin).
- Pagination: `?page=<n>&limit=<n>`, defaults `page=1`, `limit=20`, max `limit=100`. Filters and sorting via query params.
- Response shapes:

```json
{ "data": [ ... ], "pagination": { "page": 1, "limit": 20, "total_items": 342, "total_pages": 18 } }
```

```json
{ "data": { ... } }
```

```json
{ "error": { "code": "SPECIES_NOT_FOUND", "message": "No species found with id 'trex01'." } }
```

- Use correct status codes (`400`, `401`, `403`, `404`, `409`, `422`, `429`, `500`). Never leak stack traces, SQL, or internal details.
- All SQL uses bound parameters. Never log or return passwords, hashes, or tokens.

---

## 5. Quality Gates

A change is done only when:

1. It traces back to an approved spec and task.
2. Tests were written first and all pass (unit + integration against real PostgreSQL).
3. `cargo fmt --check` and `cargo clippy -- -D warnings` pass.
4. New/changed endpoints are documented (OpenAPI spec kept in sync).
5. New queries used by public endpoints are indexed and respect the performance targets.
6. No secrets, credentials, or `.env` files are committed.

---

## 6. Agent Rules

- Do not modify `docs/constitution.md` or approved specs without explicit user approval.
- Stay within the scope of the current task; note unrelated issues instead of fixing them.
- Ask the user when a decision is not covered by the constitution, spec, or plan.
- Keep commits small, one task per commit, using Conventional Commits (`feat:`, `fix:`, `test:`, `docs:`...).

---

## 7. Governance

- The constitution supersedes all other project documents, including this one.
- Constitution amendments require a written rationale, an update to the file, and a version bump:
  - **MAJOR**: a principle is removed or redefined incompatibly.
  - **MINOR**: a principle or section is added or materially expanded.
  - **PATCH**: clarifications and wording fixes.

<!-- speckit-agents:start -->
# Spec-Kit agents: shared rules

Every agent (coordinator, planner, developer, verifier, bug-fixer, idea-assessor) follows these rules.

## Artifact layering
Each artifact holds only what is new at its level. Upstream holds the context; never restate it.

| Level | Holds | Context from |
|-------|-------|--------------|
| constitution.md | project-wide principles, stack, standards | — |
| spec.md | what/why: stories, FR-/SC- IDs, edge cases | constitution |
| plan.md (+ research, data-model, contracts) | how: design decisions, structure, deviations | constitution, spec |
| tasks.md | checklist of tasks | spec, plan |
| reports (analyze, converge, bug, assessment) | findings and verdicts | the artifacts they check |

- Refer upstream by ID or section (`FR-003`, `constitution §Testing`, `plan §Data`), never by copying text.
- Don't repeat principles, stack or standards the constitution already sets; write only feature-specific choices and deviations.
- Skill templates: fill only the sections that add information at that level. Delete empty, boilerplate or restating sections; no intros, summaries, "overview" or "context" sections.
- tasks.md is the checklist only: `- [ ] T001 [P] [US1] <action> in <path>` plus phase headings. No descriptions, rationale or notes.
- Need context for a level? Read the upstream files, don't ask for it to be copied down.

## Reading
- Read only the files the step needs; prefer Grep/section reads over whole-file reads for large files.
- Don't re-read a file already given in the call input.

## Reply format
Reply with this block only. No preamble, summary or file contents; paths and IDs instead of prose. Omit empty fields.
```
STATUS: done | blocked | needs-input
SKILL: <skill run>
ARTIFACTS: <paths written>
RESULT: <verdict / Converged / counts>
FINDINGS: <one line each: ID, tag code|spec|plan|tasks, file:line or req ID, issue>
QUESTIONS: <for the user>
SKILL_REQUEST: <skill> — <why>
```

## Missing skills
- Run only skills listed in your frontmatter. Never imitate or improvise another skill's output.
- Subagents: if a step needs any other skill, return `STATUS: blocked` with `SKILL_REQUEST`.
- Coordinator: on `SKILL_REQUEST`, route the step to the agent that owns the skill and resume; if none does, ask the user to install or assign it.
<!-- speckit-agents:end -->

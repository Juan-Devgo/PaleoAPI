# Data Model — Spec 002 (delta)

**Date:** 2026-10-06. No schema change: tables, constraints, and indexes are Spec 001's ([001 data-model](../001-database/data-model.md), cited `001-DM §n`). This file adds only the API ↔ schema mapping, the write-time lookups, and the error map.

## 1. Representation ↔ columns

Only non-identity mappings are listed; every other field is the column of the same name.

| Resource | API field | Source / target |
|---|---|---|
| Period | `era` (response) | `periods.era_id` ⨝ `eras` (PK) |
| Period | `era_id` (write) | `periods.era_id`; on nested create, the path `era_id` |
| Rank ≠ domain | `<parent>` (response, singular key: `domain`, `kingdom`, `phylum`, `class`, `order`, `family`) | `<rank>.<parent>_id` ⨝ parent table (PK) |
| Country | `continents` (response) | `country_continents` ⨝ `continents`, `ORDER BY lower(name) COLLATE paleo_name_sort, id` |
| Country | `continent_ids` (write) | rows of `country_continents` (`continent_type` never written) |
| Species | `size` | six columns `length_min_m … weight_max_kg`; `null` when all six are `NULL`; a sub-object is `null` when its pair is `NULL`. Writing `size` writes all six (absent sub-object → `NULL` pair) |
| Species | `taxonomy` | lineage join `genera → families → orders → classes → phyla → kingdoms → domains` on PKs |
| Species | `periods` | `species_periods` ⨝ `periods` ⨝ `eras`, `ORDER BY p.start_mya DESC, p.id` |
| Species | `continents` | `species_continents` ⨝ `continents`, `ORDER BY (c.type = 'modern'), lower(c.name) COLLATE paleo_name_sort, c.id` |
| Species | `countries` | `species_countries` ⨝ `countries`, `ORDER BY lower(c.name) COLLATE paleo_name_sort, c.id` |
| Species | `period_ids`, `continent_ids`, `country_ids` (write) | link-table rows; a `PATCH` value replaces the set (delete missing, insert new) |

Write normalization (applied after validation, before storage):

- Text fields: trimmed (spec §4.13); the DB rejects untrimmed text (001-DM §1.2).
- Decimals: trailing zeros removed (research R1); returned as stored.
- `image_url`: scheme lowercased (001-DM §5.1 `species_image_url_ck` is case-sensitive).

## 2. PATCH merge inputs

The stored values a `PATCH` must read (under the row lock of §3) to validate the resulting resource (spec §4.10):

| Resource | Stored values read |
|---|---|
| Era | `start_mya`, `end_mya` |
| Period | `era_id`, `start_mya`, `end_mya` |
| Rank | parent id |
| Continent | `type` |
| Country | none (`continent_ids` absent → links unchanged) |
| Species | none for validation; current link sets only to compute the replacement diff |

## 3. Write-time lookups → indexes

New statement shapes (001-DM §6.1 requires re-verification of any new shape). All run in the write transaction at READ COMMITTED.

| ID | Purpose | Shape | Index |
|---|---|---|---|
| Q-W1 | lock target for `PATCH` | `SELECT … FROM <t> WHERE id = $1 FOR NO KEY UPDATE` | `<t>_pk` |
| Q-W2 | lock target for `DELETE` | `SELECT 1 FROM <t> WHERE id = $1 FOR UPDATE` (blocks concurrent FK inserts, which take `FOR KEY SHARE`) | `<t>_pk` |
| Q-W3 | nested-create parent exists (`404`) | `SELECT 1 FROM <parent> WHERE id = $1` | `<parent>_pk` |
| Q-W4 | referenced ids exist | `SELECT id FROM <t> WHERE id = ANY($1::text[])`; continents also select `type` | `<t>_pk` |
| Q-W5 | era overlap | `SELECT id FROM eras WHERE numrange(end_mya, start_mya, '[)') && numrange($1, $2, '[)') AND id <> $3 ORDER BY id LIMIT 5` | `eras_range_ex` (GiST) |
| Q-W6 | period overlap | Q-W5 on `periods` | `periods_range_ex` |
| Q-W7 | era range still contains its periods | `SELECT id, start_mya, end_mya FROM periods WHERE era_id = $1 AND (start_mya > $2 OR end_mya < $3) ORDER BY id LIMIT 5` | `periods_era_idx` |
| Q-W8 | period within target era | `SELECT start_mya, end_mya FROM eras WHERE id = $1` | `eras_pk` |
| Q-W9 | continent `modern → prehistoric` blocked | `SELECT country_id FROM country_continents WHERE continent_id = $1 ORDER BY country_id LIMIT 5` + `count(*)` | `country_continents_continent_idx` |
| Q-W10 | delete dependents | per §5: `count(*)` + `ORDER BY id LIMIT 5` | §5 |
| Q-W11 | UTC year | `SELECT extract(year FROM now() AT TIME ZONE 'UTC')::int` | — |
| Q-W12 | link replacement | `DELETE FROM <link> WHERE <owner>_id = $1 AND <target>_id <> ALL($2::text[])`; `INSERT INTO <link> SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING` | `<link>_pk` |

Reads keep the 001-DM §6.1 shapes (Q-E1 … Q-S13), including the count variants; period and rank page SQL add only the parent PK join (plan §Read path). Every nested listing first checks its parent with a PK lookup (`404`).

Read shapes that deviate from the 001-DM §6.1 text (T059; 001 artifacts unchanged, plan §Read path):

| ID | 001-DM §6.1 text | Shape run | Index |
|---|---|---|---|
| Q-S6 | `genus_id IN (SELECT g.id …)` | `s.genus_id = ANY(ARRAY(SELECT g.id FROM … WHERE <rank>.<parent>_id = $n))` (InitPlan, computed once) | unchanged: parent indexes along the chain + `species_genus_idx` |
| Q-S12 periods | `species_id = ANY($1)` | `FROM unnest($1::text[]) AS u(id) CROSS JOIN LATERAL (SELECT … FROM species_periods l WHERE l.species_id = u.id OFFSET 0)`, joined to `periods`, `eras` | `species_periods_pk` per id (not a skip scan of `species_periods_period_idx`), `periods_pk`, `eras_pk` |
| Q-S12 lineage | per species id | `FROM genera g JOIN … domains WHERE g.id = ANY($1)` on the page's distinct `genus_id`s | `genera_pk` … `domains_pk` |

## 4. Error map (SQLSTATE + constraint → response)

Applies to database errors raised during the write or at `COMMIT`. After pre-validation (research R9) only duplicates (`23505`) are expected; the other rows cover races. `DETAIL`/`MESSAGE`/`HINT` are never forwarded (001-DM §8). Field attribution for multi-field rules follows §6.

| SQLSTATE | Constraint | Statement | Response |
|---|---|---|---|
| `23505` | `<t>_pk`; `continents_id_type_uq` (implied by `continents_pk`, defensive) | insert | `409 <R>_ALREADY_EXISTS` — id |
| `23505` | `<t>_name_uq`, `species_scientific_name_uq` | insert/update | `409 <R>_ALREADY_EXISTS` — name / scientific_name, "compared ignoring case" |
| `23505` | `country_continents_pk`, `species_*_pk` | insert | `422` on the list field (duplicates are pre-rejected; defensive) |
| `23503` | any FK | `DELETE` | `409 <R>_HAS_DEPENDENTS`; the dependents are re-queried (§5) for the message |
| `23503` | `country_continents_continent_fk` | continent `UPDATE` | `422` field `type` |
| `23503` | `country_continents_continent_fk` | link insert | `422` field `continent_ids` (missing or prehistoric) |
| `23503` | `periods_era_fk`, `<rank>_<parent>_fk`, `species_genus_fk`, `species_{periods,continents,countries}_*_fk` | insert/update | `422` on `era_id`, `<parent>_id`, `genus_id`, `period_ids`/`continent_ids`/`country_ids` |
| `23514` | `<t>_id_ck`, `*_name_ck`, `species_scientific_name_ck`, `species_description_ck`, `species_image_url_ck`, `species_diet_ck`, `continents_type_ck`, `species_discovery_year_ck`, `species_discovery_year_not_future_ck` | any | `422` on the column's field |
| `23514` | `*_start_mya_ck`, `*_end_mya_ck` | any | `422` `start_mya` / `end_mya` |
| `23514` | `*_range_ck`, `periods_within_era_ck`, `eras_contains_periods_ck` | any | `422` per §6 |
| `23514` | `species_{length,height,weight}_ck` | any | `422` `size.length_m` / `size.height_m` / `size.weight_kg` |
| `23514` | `countries_min_continents_ck`, `species_min_periods_ck` | `COMMIT` | `422` `continent_ids` / `period_ids` |
| `23P01` | `eras_range_ex`, `periods_range_ex` | any | `422` per §6 |
| `23514` | `*_id_immutable_ck`, `*_no_truncate_ck`, `country_continents_continent_type_ck` | — | `500` (the API never issues these writes) |
| `23502` | NOT NULL | any | `500` (required fields are validated first; a `NULL` reaching the database is a bug) |
| `0A000`, `40001`, `40P01`, `22003`, other | — | — | `500 INTERNAL_ERROR`; one stderr line with SQLSTATE and constraint only |

`<R>` is the resource's code stem: `ERA`, `PERIOD`, `DOMAIN`, `KINGDOM`, `PHYLUM`, `CLASS`, `ORDER`, `FAMILY`, `GENUS`, `CONTINENT`, `COUNTRY`, `SPECIES`. `tests/api/error_map.rs` asserts every constraint and unique index in `pg_constraint`/`pg_index` (schema `public`) plus the trigger-raised names of 001-DM §1.1 has a row here, excluding:

- `contype = 'n'`: the NOT NULL rows PostgreSQL 18 records in `pg_constraint`; they raise `23502` → `500` (row above).
- `contype = 't'`: constraint triggers; they raise the `*_ck` names checked through the test's `trigger_names`, not their own names.
- constraints and unique indexes of `_sqlx_migrations` (migration bookkeeping, never written by the API).

## 5. Delete dependents (spec §4.10 delete protection)

| Resource | Dependents (counted, first 5 ids by `id`) | Index |
|---|---|---|
| Era | periods with `era_id` | `periods_era_idx` |
| Period | species via `species_periods` | `species_periods_period_idx` |
| Domain … Family | child-rank rows | `<child>_<parent>_idx` |
| Genus | species with `genus_id` | `species_genus_idx` |
| Continent | countries via `country_continents`, then species via `species_continents` | `country_continents_continent_idx`, `species_continents_continent_idx` |
| Country | species via `species_countries` | `species_countries_country_idx` |
| Species | none (links cascade, 001-DM §1.5) | — |

Message: `Cannot delete <r> '<id>': <n> <dependent kind(s)> depend on it (<up to 5 ids>). Delete or reassign them first.` For continents both kinds are counted; the 5 named ids are filled from countries first.

## 6. Field attribution for multi-field rules (spec §4.5)

"Sent" means present in the request body (any value, including `null`).

| Rule | Field |
|---|---|
| `start_mya > end_mya` | `end_mya`; `start_mya` if only `start_mya` was sent |
| Era or period overlap | `start_mya`; `end_mya` if only `end_mya` was sent |
| Period outside its era | `era_id` if `era_id` was sent and no range field was; else one detail per violated side: `start_mya` (period starts before the era) and/or `end_mya` (ends after) |
| Era range excludes its periods | one detail per violated side: `start_mya` and/or `end_mya`; the message names up to 5 period ids |
| Size `min > max` | `size.<measure>` |
| Country linked to a prehistoric continent | `continent_ids` |
| Continent `modern → prehistoric` with countries | `type` |
| Nested create with the parent id in the body | that parent field (`era_id`, `<parent>_id`) |

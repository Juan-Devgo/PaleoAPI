# Data Model — Spec 001 Database Foundation

**Date:** 2026-10-03. Snippets are normative only where they name a constraint or an expression.

## 1. Conventions

### 1.1 Naming

| Object | Pattern | Example |
|---|---|---|
| Table | plural `snake_case` | `eras`, `species_periods` |
| Primary key | `<table>_pk` | `eras_pk` |
| CHECK constraint | `<table>_<rule>_ck` | `eras_range_ck` |
| Foreign key | `<table>_<target>_fk` | `periods_era_fk` |
| Unique constraint or unique index | `<table>_<rule>_uq` | `eras_name_uq` |
| Exclusion constraint | `<table>_<rule>_ex` | `eras_range_ex` |
| Non-unique index | `<table>_<rule>_idx` | `periods_era_idx` |
| Trigger | `<table>_<rule>_trg` | `periods_within_era_trg` |
| Trigger function | `<table>_<rule>_fn` (one per trigger) or `paleo_<purpose>` (shared) | `periods_within_era_fn`, `paleo_forbid_id_change` |
| Helper function (non-trigger) | `<table>_assert_<rule>(…)` | `countries_assert_min_continents(text)` |
| Collation | `paleo_<purpose>` | `paleo_name_sort` |
| Rule raised by a trigger | `<table>_<rule>_ck`, sent as the error's `CONSTRAINT` field | `periods_within_era_ck` |

Every rule has a stable constraint name. Spec 002 maps database errors to API responses by **SQLSTATE plus constraint name** (§8), never by message text.

### 1.2 Shared column rules

These apply to every resource table (`eras`, `periods`, `domains`, `kingdoms`, `phyla`, `classes`, `orders`, `families`, `genera`, `continents`, `countries`, `species`).

| Column | Type | Rule | Constraint |
|---|---|---|---|
| `id` | `text NOT NULL` | slug: `char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'` | `<table>_id_ck` |
| `id` | — | primary key | `<table>_pk` |
| `id` | — | immutable: a `BEFORE UPDATE OF id` trigger raises `23514` when `NEW.id IS DISTINCT FROM OLD.id` | `<table>_id_immutable_trg` → `<table>_id_immutable_ck` |
| `name` | `text NOT NULL` | `char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s\|\s$'` | `<table>_name_ck` |
| `name` | — | unique ignoring case: `CREATE UNIQUE INDEX <table>_name_uq ON <table> (lower(name))` (default `C.UTF-8` collation, so uniqueness never depends on ICU). **Not on `species`.** | `<table>_name_uq` |
| `name` | — | linguistic case-insensitive sort (FR-016): `CREATE INDEX <table>_name_sort_idx ON <table> ((lower(name) COLLATE paleo_name_sort), id)`. On all 12 tables, `species` included. Created by migration `query_indexes` (§7). | `<table>_name_sort_idx` |

- The slug regex also rules out leading, trailing, and consecutive hyphens, and it is the same as Spec 002 §4.2. Uppercase is rejected.
- Text rule: *no leading or trailing `\s`* (Unicode whitespace under `C.UTF-8`, research R7) plus *length ≥ 1*. Untrimmed values are rejected, never trimmed.
- **Sort expression (normative):** every case-insensitive name sort is `ORDER BY lower(<col>) COLLATE paleo_name_sort [DESC], id`. An index can serve the sort only if the query repeats this expression exactly (research R7). Below, `NAME_SORT(<col>)` is shorthand for `(lower(<col>) COLLATE paleo_name_sort)`.
- **TRUNCATE guard (research R10):** all 16 tables have `<table>_no_truncate_trg` (`BEFORE TRUNCATE … FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate()`), created in the table's migration. Outcomes: plain `TRUNCATE` of a resource table → PostgreSQL's `0A000` (all are FK-referenced; fires before the guard); plain `TRUNCATE` of a link table → `23514 <table>_no_truncate_ck`; any `TRUNCATE … CASCADE` → `23514 <named table>_no_truncate_ck`.
- No column uses `varchar(n)`. Lengths are `char_length` CHECKs on `text` (Unicode characters).

### 1.3 Shared functions (migration `foundation`)

| Function | Purpose |
|---|---|
| `paleo_forbid_id_change()` | `BEFORE UPDATE OF id` row trigger body. It raises `check_violation` with `CONSTRAINT = TG_TABLE_NAME || '_id_immutable_ck'`. |
| `paleo_forbid_truncate()` | `BEFORE TRUNCATE` statement trigger body. It raises `check_violation` with `CONSTRAINT = TG_TABLE_NAME || '_no_truncate_ck'`. |
| `paleo_require_safe_isolation()` | Raises `feature_not_supported` (`0A000`) when `current_setting('transaction_isolation') = 'repeatable read'`. Every cross-row trigger calls it first (research R10). |

The `foundation` migration also checks that the database encoding is UTF8 (it raises an error otherwise), runs `CREATE EXTENSION IF NOT EXISTS pg_trgm`, and creates the sort collation:

```sql
CREATE COLLATION paleo_name_sort (provider = icu, locale = 'und');   -- deterministic ICU root collation
```

### 1.4 Collation version tracking (FR-016)

Only an image change to a new ICU major can change the sort order (research R7).

- **Detect:** `SELECT collversion, pg_collation_actual_version(oid) FROM pg_collation WHERE collname = 'paleo_name_sort'`. If the two differ, the sort indexes may be out of order. The API prints a startup warning when they differ (plan §Startup contract, step 4b).
- **Repair:** `REINDEX` every index that uses `paleo_name_sort` (find them with `SELECT indexrelid::regclass FROM pg_index WHERE 'paleo_name_sort'::regcollation = ANY (indcollation::oid[])`), then `ALTER COLLATION paleo_name_sort REFRESH VERSION`.
- No **rule** depends on this collation: PKs, `*_name_uq`, CHECKs, exclusions, FKs, and trigger checks use `C.UTF-8` or equality. `<rank>_<parent>_idx` and `species_genus_idx` hold the collated expression as a *non-leading* column; FK/delete lookups use only the leading id by equality, so drift can misorder sorted pages but never miss a row. The `pg_index` query above finds these too; REINDEX them with the sort indexes.

### 1.5 Delete behavior

- Every FK between resources uses `ON DELETE RESTRICT`.
- `ON DELETE CASCADE` is used only for owned rows: `country_continents.country_id`, `species_periods.species_id`, `species_continents.species_id`, `species_countries.species_id`. A species' size is stored in columns on `species` itself (§3.4), so it is removed together with the row.
- `ON UPDATE` stays at the default (`NO ACTION`), because ids never change. One exception: `country_continents_continent_fk` uses `ON UPDATE RESTRICT` to block changes to a continent's `type` (§3.3).

---

## 2. Geologic time

### 2.1 `eras`

| Column | Type | Null | Notes |
|---|---|---|---|
| `id` | `text` | no | §1.2 |
| `name` | `text` | no | §1.2 |
| `start_mya` | `numeric` | no | millions of years ago |
| `end_mya` | `numeric` | no | |

| Constraint / index | Definition |
|---|---|
| `eras_pk`, `eras_id_ck`, `eras_name_ck`, `eras_name_uq` | §1.2 |
| `eras_start_mya_ck` | `start_mya BETWEEN 0 AND 4600 AND start_mya = round(start_mya, 3)` |
| `eras_end_mya_ck` | `end_mya BETWEEN 0 AND 4600 AND end_mya = round(end_mya, 3)` |
| `eras_range_ck` | `start_mya > end_mya` |
| `eras_range_ex` | `EXCLUDE USING gist (numrange(end_mya, start_mya, '[)') WITH &&)`. Touching boundaries are allowed. Gaps are allowed. |
| `eras_start_mya_idx` | btree `(start_mya, id)` *(migration `query_indexes`)* |
| `eras_name_sort_idx` | btree `(NAME_SORT(name), id)` *(migration `query_indexes`)* |
| `eras_id_immutable_trg` | §1.3 |
| `eras_no_truncate_trg` | §1.2 TRUNCATE guard |
| `eras_contains_periods_trg` | `AFTER UPDATE OF start_mya, end_mya`, immediate, row-level, `EXECUTE FUNCTION eras_contains_periods_fn()`. The function calls the isolation guard, then raises `eras_contains_periods_ck` if any period of `NEW.id` falls outside `[NEW.end_mya, NEW.start_mya]`. Uses `periods_era_idx`. |

### 2.2 `periods`

| Column | Type | Null | Notes |
|---|---|---|---|
| `id` | `text` | no | §1.2 |
| `era_id` | `text` | no | owning era |
| `name` | `text` | no | §1.2 (unique across all periods, Spec 002 §5.2) |
| `start_mya` | `numeric` | no | |
| `end_mya` | `numeric` | no | |

| Constraint / index | Definition |
|---|---|
| `periods_pk`, `periods_id_ck`, `periods_name_ck`, `periods_name_uq` | §1.2 |
| `periods_start_mya_ck`, `periods_end_mya_ck`, `periods_range_ck` | same expressions as on `eras` |
| `periods_range_ex` | `EXCLUDE USING gist (numrange(end_mya, start_mya, '[)') WITH &&)`. Global: because periods sit inside eras that do not overlap, this is the same as no overlap within an era (research R9). |
| `periods_era_fk` | `FOREIGN KEY (era_id) REFERENCES eras (id) ON DELETE RESTRICT` |
| `periods_era_idx` | btree `(era_id, start_mya, id)`. Serves FK and delete lookups, the containment trigger, and the nested listing sorted by `start_mya`. |
| `periods_start_mya_idx` | btree `(start_mya, id)` *(migration `query_indexes`)* |
| `periods_name_sort_idx` | btree `(NAME_SORT(name), id)` *(migration `query_indexes`)* |
| `periods_id_immutable_trg` | §1.3 |
| `periods_no_truncate_trg` | §1.2 TRUNCATE guard |
| `periods_within_era_trg` | `AFTER INSERT OR UPDATE OF era_id, start_mya, end_mya`, immediate, row-level, `EXECUTE FUNCTION periods_within_era_fn()`. The function calls the isolation guard, takes `SELECT … FROM eras WHERE id = NEW.era_id FOR SHARE`, then raises `periods_within_era_ck` unless `NEW.start_mya <= era.start_mya AND NEW.end_mya >= era.end_mya`. |

**Ordering rule for writers:** the checks run immediately, statement by statement. To shrink an era and move its periods in one transaction, move or shrink the periods first and change the era last. To grow, change the era first.

---

## 3. Geography

### 3.1 `continents`

| Column | Type | Null | Notes |
|---|---|---|---|
| `id`, `name` | `text` | no | §1.2 |
| `type` | `text` | no | `prehistoric` or `modern` |

| Constraint / index | Definition |
|---|---|
| `continents_pk`, `continents_id_ck`, `continents_name_ck`, `continents_name_uq` | §1.2 |
| `continents_type_ck` | `type IN ('prehistoric', 'modern')` (lowercase only) |
| `continents_id_type_uq` | `UNIQUE (id, type)`. Target of the composite FK below. |
| `continents_type_idx` | btree `(type, NAME_SORT(name), id)` *(migration `query_indexes`)* |
| `continents_name_sort_idx` | btree `(NAME_SORT(name), id)` *(migration `query_indexes`)* |
| `continents_id_immutable_trg` | §1.3 |
| `continents_no_truncate_trg` | §1.2 TRUNCATE guard |

### 3.2 `countries`

| Column | Type | Null |
|---|---|---|
| `id`, `name` | `text` | no |

| Constraint / index | Definition |
|---|---|
| `countries_pk`, `countries_id_ck`, `countries_name_ck`, `countries_name_uq` | §1.2 |
| `countries_name_sort_idx` | btree `(NAME_SORT(name), id)` *(migration `query_indexes`)* |
| `countries_id_immutable_trg` | §1.3 |
| `countries_no_truncate_trg` | §1.2 TRUNCATE guard |
| `countries_min_continents_trg` | `CONSTRAINT TRIGGER … AFTER INSERT … DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION countries_min_continents_fn()`. The function calls `countries_assert_min_continents(NEW.id)`. |

`countries_assert_min_continents(country_id text)` calls the isolation guard, runs `SELECT 1 FROM countries WHERE id = $1 FOR NO KEY UPDATE`, and returns if the country no longer exists. Otherwise it raises `countries_min_continents_ck` when no `country_continents` row exists for the country.

### 3.3 `country_continents` (owned by the country)

| Column | Type | Null | Notes |
|---|---|---|---|
| `country_id` | `text` | no | |
| `continent_id` | `text` | no | |
| `continent_type` | `text` | no | `DEFAULT 'modern'`. Writers never set it. |

| Constraint / index | Definition |
|---|---|
| `country_continents_pk` | `PRIMARY KEY (country_id, continent_id)`. No duplicate links. Serves per-country lookups, the cascade, and the minimum-count check. |
| `country_continents_country_fk` | `FOREIGN KEY (country_id) REFERENCES countries (id) ON DELETE CASCADE` |
| `country_continents_continent_fk` | `FOREIGN KEY (continent_id, continent_type) REFERENCES continents (id, type) ON UPDATE RESTRICT ON DELETE RESTRICT`. Linking to a prehistoric continent fails. Changing a linked continent's `type` fails. Deleting a linked continent fails. |
| `country_continents_continent_type_ck` | `continent_type = 'modern'` |
| `country_continents_continent_idx` | btree `(continent_id, country_id)`. Serves the continent filter, the continent's country list, and FK delete lookups. |
| `country_continents_min_trg` | `CONSTRAINT TRIGGER … AFTER DELETE OR UPDATE … DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION country_continents_min_fn()`. The function calls `countries_assert_min_continents(OLD.country_id)`. It is a separate function because a trigger function on `countries` cannot reference `OLD.country_id`. |
| `country_continents_no_truncate_trg` | §1.2 TRUNCATE guard |

---

## 4. Taxonomy

There are seven tables with the same shape. Each rank except `domains` has one required parent column pointing to the rank directly above it.

| Table | Parent column | Parent FK | Parent index |
|---|---|---|---|
| `domains` | — | — | — |
| `kingdoms` | `domain_id` | `kingdoms_domain_fk → domains(id)` | `kingdoms_domain_idx (domain_id, NAME_SORT(name), id)` |
| `phyla` | `kingdom_id` | `phyla_kingdom_fk → kingdoms(id)` | `phyla_kingdom_idx (kingdom_id, NAME_SORT(name), id)` |
| `classes` | `phylum_id` | `classes_phylum_fk → phyla(id)` | `classes_phylum_idx (phylum_id, NAME_SORT(name), id)` |
| `orders` | `class_id` | `orders_class_fk → classes(id)` | `orders_class_idx (class_id, NAME_SORT(name), id)` |
| `families` | `order_id` | `families_order_fk → orders(id)` | `families_order_idx (order_id, NAME_SORT(name), id)` |
| `genera` | `family_id` | `genera_family_fk → families(id)` | `genera_family_idx (family_id, NAME_SORT(name), id)` |

Every table has `id`, `name` (§1.2: `_pk`, `_id_ck`, `_name_ck`, `_name_uq` with the name unique within the rank, `_name_sort_idx` from migration `query_indexes`, `_id_immutable_trg`, `_no_truncate_trg`), and its parent column `text NOT NULL`. Every parent FK uses `ON DELETE RESTRICT`. Each parent index serves FK and delete lookups, the `?<parent>=` filter, the children listing sorted by name, and the lineage joins behind species taxonomy filters. A table that references the wrong rank cannot exist, because each FK targets exactly one parent table.

---

## 5. Species

### 5.1 `species`

| Column | Type | Null | Notes |
|---|---|---|---|
| `id` | `text` | no | §1.2 |
| `genus_id` | `text` | no | |
| `name` | `text` | no | common name, **not unique** |
| `scientific_name` | `text` | no | |
| `diet` | `text` | no | |
| `description` | `text` | no | |
| `discovery_year` | `integer` | yes | |
| `image_url` | `text` | yes | |
| `length_min_m`, `length_max_m` | `numeric` | yes | size: length |
| `height_min_m`, `height_max_m` | `numeric` | yes | size: height |
| `weight_min_kg`, `weight_max_kg` | `numeric` | yes | size: weight |

Size is stored as six nullable columns. "No size" means all six are `NULL`. A size that is present has at least one complete pair by construction, so the empty size object `{}` cannot be stored. Absent sub-objects are a `NULL` pair. No separate table or cascade is needed.

| Constraint / index | Definition |
|---|---|
| `species_pk`, `species_id_ck`, `species_id_immutable_trg`, `species_no_truncate_trg` | §1.2 |
| `species_name_ck` | `char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s\|\s$'` (no unique index) |
| `species_scientific_name_ck` | `char_length(scientific_name) BETWEEN 1 AND 128 AND scientific_name !~ '^\s\|\s$'` |
| `species_scientific_name_uq` | unique index on `(lower(scientific_name))` (default collation) |
| `species_scientific_name_sort_idx` | btree `(NAME_SORT(scientific_name), id)`. Serves the `scientific_name` sort. *(migration `query_indexes`)* |
| `species_diet_ck` | `diet IN ('carnivore','herbivore','omnivore','piscivore','insectivore')` |
| `species_description_ck` | `char_length(description) BETWEEN 1 AND 1000 AND description !~ '^\s\|\s$'` |
| `species_image_url_ck` | `char_length(image_url) <= 2048 AND image_url ~ '^https?://' AND image_url !~ '\s$'` (case-sensitive prefix; plan §Notes for Spec 002) |
| `species_discovery_year_ck` | `discovery_year >= 1600` |
| `species_discovery_year_trg` | `BEFORE INSERT OR UPDATE OF discovery_year … EXECUTE FUNCTION species_discovery_year_fn()`. The function raises `species_discovery_year_not_future_ck` when `NEW.discovery_year > extract(year FROM now() AT TIME ZONE 'UTC')`. |
| `species_length_ck` | `(length_min_m IS NULL) = (length_max_m IS NULL) AND length_min_m > 0 AND length_max_m < 'Infinity' AND length_min_m = round(length_min_m, 6) AND length_max_m = round(length_max_m, 6) AND length_min_m <= length_max_m` |
| `species_height_ck` | same shape for `height_min_m`, `height_max_m` |
| `species_weight_ck` | same shape for `weight_min_kg`, `weight_max_kg` |
| `species_genus_fk` | `FOREIGN KEY (genus_id) REFERENCES genera (id) ON DELETE RESTRICT` |
| `species_genus_idx` | btree `(genus_id, NAME_SORT(name), id)` |
| `species_name_sort_idx` | btree `(NAME_SORT(name), id)`. Default sort. *(migration `query_indexes`)* |
| `species_diet_idx` | btree `(diet, NAME_SORT(name), id)` *(migration `query_indexes`)* |
| `species_discovery_year_asc_idx` | btree `(discovery_year ASC NULLS LAST, id)` *(migration `query_indexes`)* |
| `species_discovery_year_desc_idx` | btree `(discovery_year DESC NULLS LAST, id)` (missing years sort last in both directions) *(migration `query_indexes`)* |
| `species_name_trgm_idx` | GIN `(name gin_trgm_ops)` *(migration `query_indexes`)* |
| `species_scientific_name_trgm_idx` | GIN `(scientific_name gin_trgm_ops)` *(migration `query_indexes`)* |
| `species_min_periods_trg` | `CONSTRAINT TRIGGER … AFTER INSERT … DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION species_min_periods_fn()`. The function calls `species_assert_min_periods(NEW.id)`. |

How the size CHECKs treat NULLs: if both values are `NULL`, the expression evaluates to `NULL` and the row passes. If only one is `NULL`, the first term is false and the row fails. If both are present, every term must be true. The `< 'Infinity'` term also rejects `NaN`, because NaN sorts above Infinity, and `> 0` rejects `-Infinity`.

`species_assert_min_periods(species_id text)` works like `countries_assert_min_continents`: guard, lock with `FOR NO KEY UPDATE`, skip if the species is gone, and raise `species_min_periods_ck` if there are no periods.

### 5.2 Link tables (owned by the species)

| Table | Columns (all `text NOT NULL`) | PK | Owner FK | Target FK | Target index |
|---|---|---|---|---|---|
| `species_periods` | `species_id`, `period_id` | `species_periods_pk (species_id, period_id)` | `species_periods_species_fk … ON DELETE CASCADE` | `species_periods_period_fk → periods(id) ON DELETE RESTRICT` | `species_periods_period_idx (period_id, species_id)` |
| `species_continents` | `species_id`, `continent_id` | `species_continents_pk (species_id, continent_id)` | `species_continents_species_fk … ON DELETE CASCADE` | `species_continents_continent_fk → continents(id) ON DELETE RESTRICT` | `species_continents_continent_idx (continent_id, species_id)` |
| `species_countries` | `species_id`, `country_id` | `species_countries_pk (species_id, country_id)` | `species_countries_species_fk … ON DELETE CASCADE` | `species_countries_country_fk → countries(id) ON DELETE RESTRICT` | `species_countries_country_idx (country_id, species_id)` |

- `species_periods_min_trg`: `CONSTRAINT TRIGGER … AFTER DELETE OR UPDATE ON species_periods … DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION species_periods_min_fn()`. The function calls `species_assert_min_periods(OLD.species_id)`.
- `species_periods_no_truncate_trg`, `species_continents_no_truncate_trg`, `species_countries_no_truncate_trg`: §1.2 TRUNCATE guard.
- `species_continents` has no type column (either type allowed).

---

## 6. Index catalog

### 6.1 Public query shapes (Spec 002) → indexes (FR-011)

AC 14: each shape **can be served by an index**, verified with the query planner. `tests/db/indexes.rs` checks it on the 10,000-species dataset (§9) using the rule in research R15. In the shapes below, `NAME_SORT(col)` stands for `lower(col) COLLATE paleo_name_sort` (§1.2). "Allowed" means the plan must use at least one of the listed indexes and must contain no `Seq Scan` on a resource or link table. Each list endpoint also has a `count(*)` query with the same filter. It uses the same allowed set, except that sort-only indexes do not count. For counts, the filter indexes or the PK satisfy the check.

| ID | Spec 002 endpoint / param | Query shape | Allowed index(es) |
|---|---|---|---|
| Q-E1 | `GET /eras` sort `start_mya` / `-start_mya` (default) | `ORDER BY start_mya [DESC], id` | `eras_start_mya_idx` |
| Q-E2 | `GET /eras` sort `name` | `ORDER BY NAME_SORT(name) [DESC], id` | `eras_name_sort_idx` |
| Q-E3 | `GET /eras/{id}` | `WHERE id = $1` | `eras_pk` |
| Q-P1 | `GET /eras/{era_id}/periods`, `GET /periods?era=` | `WHERE era_id = $1 ORDER BY start_mya [DESC], id` or `ORDER BY NAME_SORT(name), id` | `periods_era_idx` |
| Q-P2 | `GET /periods` sort `start_mya` | `ORDER BY start_mya [DESC], id` | `periods_start_mya_idx` |
| Q-P3 | `GET /periods` sort `name` | `ORDER BY NAME_SORT(name) [DESC], id` | `periods_name_sort_idx` |
| Q-P4 | `GET /periods/{id}` (+ era summary) | `WHERE p.id = $1`, join `eras` on PK | `periods_pk`, `eras_pk` |
| Q-T1 | `GET /taxonomy/{rank}` | `ORDER BY NAME_SORT(name) [DESC], id` | `<rank>_name_sort_idx` |
| Q-T2 | `GET /taxonomy/{rank}?<parent>=`, `GET /taxonomy/{parent_rank}/{id}/{rank}` | `WHERE <parent>_id = $1 ORDER BY NAME_SORT(name), id` | `<rank>_<parent>_idx` |
| Q-T3 | `GET /taxonomy/{rank}/{id}` (+ parent summary) | PK lookup + parent PK | `<rank>_pk`, `<parent rank>_pk` |
| Q-C1 | `GET /continents?type=` | `WHERE type = $1 ORDER BY NAME_SORT(name), id` | `continents_type_idx` |
| Q-C2 | `GET /continents` sort `name` | `ORDER BY NAME_SORT(name) [DESC], id` | `continents_name_sort_idx` |
| Q-C3 | `GET /continents/{id}/countries`, `GET /countries?continent=` | `JOIN country_continents cc … WHERE cc.continent_id = $1 ORDER BY NAME_SORT(c.name), c.id` | `country_continents_continent_idx` (+ `countries_pk`) |
| Q-K1 | `GET /countries` sort `name` | `ORDER BY NAME_SORT(name) [DESC], id` | `countries_name_sort_idx` |
| Q-K2 | `GET /countries/{id}` (+ continents) | PK, then `country_continents WHERE country_id = ANY($1)` join `continents` | `countries_pk`, `country_continents_pk`, `continents_pk` |
| Q-S1 | `GET /species` default / `sort=name` / `-name` | `ORDER BY NAME_SORT(name) [DESC], id LIMIT/OFFSET` | `species_name_sort_idx` |
| Q-S2 | `sort=scientific_name` | `ORDER BY NAME_SORT(scientific_name) [DESC], id` | `species_scientific_name_sort_idx` |
| Q-S3 | `sort=discovery_year` / `-discovery_year` | `ORDER BY discovery_year ASC NULLS LAST, id` / `DESC NULLS LAST, id` | `species_discovery_year_asc_idx` / `species_discovery_year_desc_idx` |
| Q-S4 | `?diet=` | `WHERE diet = $1 ORDER BY NAME_SORT(name), id` | `species_diet_idx` |
| Q-S5 | `?genus=` | `WHERE genus_id = $1` | `species_genus_idx` |
| Q-S6 | `?family=` … `?domain=` | `WHERE genus_id IN (SELECT g.id FROM genera g JOIN families f … WHERE <rank>.<parent>_id = $1)`, or the equivalent at that rank | the parent indexes along the chain + `species_genus_idx` |
| Q-S7 | `?period=` | `WHERE EXISTS (SELECT 1 FROM species_periods sp WHERE sp.species_id = s.id AND sp.period_id = $1)` | `species_periods_period_idx` |
| Q-S8 | `?era=` | as Q-S7, joined to `periods p ON p.id = sp.period_id AND p.era_id = $1` | `periods_era_idx`, `species_periods_period_idx` |
| Q-S9 | `?continent=` | `EXISTS` on `species_continents` | `species_continents_continent_idx` |
| Q-S10 | `?country=` | `EXISTS` on `species_countries` | `species_countries_country_idx` |
| Q-S11 | `?q=` (Spec 002: 3–64 characters) | `name ILIKE $p ESCAPE '\' OR scientific_name ILIKE $p ESCAPE '\'` | Page: `species_name_trgm_idx`, `species_scientific_name_trgm_idx`, or `species_name_sort_idx` (the planner may walk the sort index and filter). Count: both trigram indexes (BitmapOr), **and** they must actually filter. Under `EXPLAIN (ANALYZE)` with a 3-character term and a longer term from the fixture, the rows returned by **each** trigram Bitmap Index Scan are < 10% of `species`, and `Rows Removed by Index Recheck` is absent or < 10% of `species` (research R13, R15). |
| Q-S12 | embedding for a page of species | `WHERE species_id = ANY($1)` on each link table, joined to target PKs, plus lineage PK lookups `genera` → `domains` | `species_periods_pk`, `species_continents_pk`, `species_countries_pk`, `periods_pk`, `eras_pk`, `continents_pk`, `countries_pk`, `genera_pk` … `domains_pk` |
| Q-S13 | `GET /species/{id}` | `WHERE id = $1` | `species_pk` |

The trigram indexes are on the plain columns, so the ICU sort collation does not affect `q` (research R13). For a descending sort on a `(key, id)` index, the planner scans the index backward and adds an Incremental Sort on `id` (verified). This counts as index-served. Spec 002 must keep the shapes above, or re-verify any new shape against this catalog.

### 6.2 Integrity lookups (FR-012)

| Lookup | Index |
|---|---|
| Delete protection: era → periods | `periods_era_idx` |
| Delete protection: each rank → children | `<child>_<parent>_idx` |
| Delete protection: genus → species | `species_genus_idx` |
| Delete protection: period, continent, country → species links | `species_periods_period_idx`, `species_continents_continent_idx`, `species_countries_country_idx` |
| Delete protection and type-change protection: continent → country links | `country_continents_continent_idx` |
| Cascade of owned links on species or country delete | `species_*_pk`, `country_continents_pk` (owner id is the leading column) |
| Insert/update FK checks (target exists) | target `*_pk`; `continents_id_type_uq` for the composite FK |
| Overlaps | `eras_range_ex`, `periods_range_ex` (GiST) |
| Containment (period → era, era → periods) | `eras_pk`, `periods_era_idx` |
| Minimum counts | `country_continents_pk`, `species_periods_pk` |
| Case-insensitive uniqueness | `<table>_name_uq`, `species_scientific_name_uq` |

---

## 7. Migrations

Six reversible pairs (`sqlx migrate add -r <name>`), applied in order. Migrations 2–5 hold tables, constraints, triggers, and **integrity** indexes (§6.2); migration 6 holds the **query-only** indexes (§6.1) so index tests fail first (research R3).

| # | Name | Contents | Down |
|---|---|---|---|
| 1 | `foundation` | UTF8 encoding check, `pg_trgm`, collation `paleo_name_sort`, `paleo_forbid_id_change()`, `paleo_forbid_truncate()`, `paleo_require_safe_isolation()` | drop the functions and the collation; `DROP EXTENSION IF EXISTS pg_trgm` |
| 2 | `geologic_time` | `eras`, `periods`, their constraints, integrity indexes, trigger functions, and triggers (§2) | drop in reverse dependency order |
| 3 | `taxonomy` | the seven rank tables (§4) | drop from `genera` up to `domains` |
| 4 | `geography` | `continents`, `countries`, `country_continents`, the minimum-count functions and triggers (§3) | drop in reverse |
| 5 | `species` | `species`, the three link tables, the discovery-year and minimum-period functions and triggers, the integrity indexes (§5) | drop in reverse |
| 6 | `query_indexes` | every index marked *(migration `query_indexes`)* in §2–§5: `eras_start_mya_idx`, `periods_start_mya_idx`, the 12 `<table>_name_sort_idx`, `continents_type_idx`, `species_scientific_name_sort_idx`, `species_diet_idx`, `species_discovery_year_asc_idx`, `species_discovery_year_desc_idx`, `species_name_trgm_idx`, `species_scientific_name_trgm_idx` | drop the indexes |

Migrations may be edited until this branch merges; after that they are append-only (the startup check reports edited files as checksum mismatches).

---

## 8. Error surface for Spec 002

Every rule violation leaves stored data unchanged (the statement or transaction aborts). Spec 002 maps errors by SQLSTATE plus constraint name, and **must never forward `DETAIL`, `MESSAGE`, or `HINT` text**, because `DETAIL` includes row values (verified).

| SQLSTATE | Raised by | Typical Spec 002 status |
|---|---|---|
| `23505` unique_violation | `*_pk` (duplicate id), `*_name_uq`, `species_scientific_name_uq`, link `*_pk` (duplicate link) | `409` (duplicate link: `422`, the API rejects duplicate ids in lists first) |
| `23503` foreign_key_violation | insert/update: the target is missing, or a country links to a prehistoric continent → `422`. Delete or continent type change: dependents exist → `409`, or `422` for the type change (Spec 002 §5.4) | see left |
| `23514` check_violation | every `*_ck` constraint and every trigger-raised rule (`*_id_immutable_ck`, `*_no_truncate_ck`, `periods_within_era_ck`, `eras_contains_periods_ck`, `species_discovery_year_not_future_ck`, `countries_min_continents_ck`, `species_min_periods_ck`) | `422` |
| `23P01` exclusion_violation | `eras_range_ex`, `periods_range_ex` | `422` |
| `0A000` feature_not_supported | isolation guard (REPEATABLE READ); PostgreSQL's own refusal of a plain `TRUNCATE` on an FK-referenced table (all 12 resource tables) | `500` (programming error) |
| `40001`, `40P01` | serialization failure or deadlock under concurrency | retry or `500` |

Deferred rules (`countries_min_continents_ck`, `species_min_periods_ck`) are raised at **COMMIT**, so Spec 002 must handle errors returned by the commit call.

---

## 9. Index-verification dataset (AC 14)

`tests/db/fixtures/index_dataset.sql` (test-only, not seed data): one `BEGIN … COMMIT`, deterministic `generate_series` (no `random()`); every row satisfies all constraints, so deferred minimum checks pass at `COMMIT`. The test runs `ANALYZE` afterwards.

| Entity | Rows | Shape |
|---|---|---|
| eras | 12 | contiguous, non-overlapping ranges inside 0–4600 |
| periods | 60 | 5 contiguous periods per era |
| domains / kingdoms / phyla / classes / orders / families / genera | 3 / 12 / 60 / 240 / 800 / 2,000 / 5,000 | each child picks a parent round-robin |
| continents | 16 | 8 modern, 8 prehistoric |
| countries | 200 | 1–2 modern continents each (≈ 300 links) |
| species | 10,000 | every diet; ~1/7 without `discovery_year`; ~half with a size |
| species_periods | ≈ 20,000 | 1–3 per species |
| species_continents | ≈ 15,000 | 0–3 per species |
| species_countries | ≈ 20,000 | 0–4 per species |

Names are synthetic, pronounceable syllable combinations (some accented, e.g. `Ñ`, `É`); `scientific_name` is unique. The file's header comment documents one 3-character `q` term (e.g. `rax`) and one longer term (e.g. `saurus`), each matching 1–4% of species (100–400 rows) via `name` or `scientific_name`, leaving margin under the Q-S11 < 10% check.

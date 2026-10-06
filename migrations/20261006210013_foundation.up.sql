-- Foundation: encoding check, pg_trgm, name-sort collation, shared trigger functions
-- (data-model §1.3).

DO $$
BEGIN
    IF (SELECT pg_encoding_to_char(encoding) FROM pg_database WHERE datname = current_database()) <> 'UTF8' THEN
        RAISE EXCEPTION 'PaleoAPI requires a UTF8 database (see compose.yaml POSTGRES_INITDB_ARGS)';
    END IF;
END
$$;

CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Deterministic ICU root collation, used only for linguistic name sorts (FR-016).
CREATE COLLATION paleo_name_sort (provider = icu, locale = 'und');

-- BEFORE UPDATE OF id row trigger body: ids never change.
CREATE FUNCTION paleo_forbid_id_change() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id THEN
        RAISE EXCEPTION 'the id of a % row cannot change', TG_TABLE_NAME
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = TG_TABLE_NAME || '_id_immutable_ck',
                  TABLE = TG_TABLE_NAME,
                  COLUMN = 'id';
    END IF;
    RETURN NEW;
END
$$;

-- BEFORE TRUNCATE statement trigger body: TRUNCATE bypasses row triggers and RESTRICT.
CREATE FUNCTION paleo_forbid_truncate() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'TRUNCATE is not allowed on %', TG_TABLE_NAME
        USING ERRCODE = 'check_violation',
              CONSTRAINT = TG_TABLE_NAME || '_no_truncate_ck',
              TABLE = TG_TABLE_NAME;
END
$$;

-- Cross-row trigger checks are unsafe under REPEATABLE READ (research R10).
CREATE FUNCTION paleo_require_safe_isolation() RETURNS void
LANGUAGE plpgsql AS $$
BEGIN
    IF current_setting('transaction_isolation') = 'repeatable read' THEN
        RAISE EXCEPTION 'writes under REPEATABLE READ are not supported; use READ COMMITTED or SERIALIZABLE'
            USING ERRCODE = 'feature_not_supported';
    END IF;
END
$$;

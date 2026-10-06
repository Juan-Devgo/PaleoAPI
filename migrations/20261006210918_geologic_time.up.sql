-- Geologic time: eras and periods (data-model §2).

CREATE TABLE eras (
    id        text    NOT NULL,
    name      text    NOT NULL,
    start_mya numeric NOT NULL,
    end_mya   numeric NOT NULL,
    CONSTRAINT eras_pk PRIMARY KEY (id),
    CONSTRAINT eras_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT eras_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT eras_start_mya_ck CHECK (start_mya BETWEEN 0 AND 4600 AND start_mya = round(start_mya, 3)),
    CONSTRAINT eras_end_mya_ck CHECK (end_mya BETWEEN 0 AND 4600 AND end_mya = round(end_mya, 3)),
    CONSTRAINT eras_range_ck CHECK (start_mya > end_mya),
    CONSTRAINT eras_range_ex EXCLUDE USING gist (numrange(end_mya, start_mya, '[)') WITH &&)
);

CREATE UNIQUE INDEX eras_name_uq ON eras (lower(name));

CREATE TRIGGER eras_id_immutable_trg BEFORE UPDATE OF id ON eras
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER eras_no_truncate_trg BEFORE TRUNCATE ON eras
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE periods (
    id        text    NOT NULL,
    era_id    text    NOT NULL,
    name      text    NOT NULL,
    start_mya numeric NOT NULL,
    end_mya   numeric NOT NULL,
    CONSTRAINT periods_pk PRIMARY KEY (id),
    CONSTRAINT periods_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT periods_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT periods_start_mya_ck CHECK (start_mya BETWEEN 0 AND 4600 AND start_mya = round(start_mya, 3)),
    CONSTRAINT periods_end_mya_ck CHECK (end_mya BETWEEN 0 AND 4600 AND end_mya = round(end_mya, 3)),
    CONSTRAINT periods_range_ck CHECK (start_mya > end_mya),
    -- Global: periods sit inside non-overlapping eras, so this equals "no overlap within an era".
    CONSTRAINT periods_range_ex EXCLUDE USING gist (numrange(end_mya, start_mya, '[)') WITH &&),
    CONSTRAINT periods_era_fk FOREIGN KEY (era_id) REFERENCES eras (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX periods_name_uq ON periods (lower(name));
CREATE INDEX periods_era_idx ON periods (era_id, start_mya, id);

CREATE TRIGGER periods_id_immutable_trg BEFORE UPDATE OF id ON periods
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER periods_no_truncate_trg BEFORE TRUNCATE ON periods
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

-- A period lies within its era. Locking the era FOR SHARE serializes this check
-- with concurrent era updates (research R10).
CREATE FUNCTION periods_within_era_fn() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    era_start numeric;
    era_end   numeric;
BEGIN
    PERFORM paleo_require_safe_isolation();
    SELECT start_mya, end_mya INTO era_start, era_end
      FROM eras WHERE id = NEW.era_id FOR SHARE;
    IF NOT FOUND THEN
        RETURN NULL; -- periods_era_fk reports the missing era
    END IF;
    IF NOT (NEW.start_mya <= era_start AND NEW.end_mya >= era_end) THEN
        RAISE EXCEPTION 'a period''s range must lie within its era''s range'
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'periods_within_era_ck',
                  TABLE = 'periods';
    END IF;
    RETURN NULL;
END
$$;

CREATE TRIGGER periods_within_era_trg AFTER INSERT OR UPDATE OF era_id, start_mya, end_mya ON periods
    FOR EACH ROW EXECUTE FUNCTION periods_within_era_fn();

-- An era's range contains all of its periods.
CREATE FUNCTION eras_contains_periods_fn() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM paleo_require_safe_isolation();
    IF EXISTS (
        SELECT 1 FROM periods
         WHERE era_id = NEW.id
           AND (start_mya > NEW.start_mya OR end_mya < NEW.end_mya)
    ) THEN
        RAISE EXCEPTION 'an era''s range must contain all of its periods'
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'eras_contains_periods_ck',
                  TABLE = 'eras';
    END IF;
    RETURN NULL;
END
$$;

CREATE TRIGGER eras_contains_periods_trg AFTER UPDATE OF start_mya, end_mya ON eras
    FOR EACH ROW EXECUTE FUNCTION eras_contains_periods_fn();

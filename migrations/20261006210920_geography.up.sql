-- Geography: continents, countries, and country–continent links (data-model §3).

CREATE TABLE continents (
    id   text NOT NULL,
    name text NOT NULL,
    type text NOT NULL,
    CONSTRAINT continents_pk PRIMARY KEY (id),
    CONSTRAINT continents_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT continents_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT continents_type_ck CHECK (type IN ('prehistoric', 'modern')),
    -- Target of country_continents_continent_fk (modern-only links, research R11).
    CONSTRAINT continents_id_type_uq UNIQUE (id, type)
);

CREATE UNIQUE INDEX continents_name_uq ON continents (lower(name));

CREATE TRIGGER continents_id_immutable_trg BEFORE UPDATE OF id ON continents
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER continents_no_truncate_trg BEFORE TRUNCATE ON continents
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE countries (
    id   text NOT NULL,
    name text NOT NULL,
    CONSTRAINT countries_pk PRIMARY KEY (id),
    CONSTRAINT countries_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT countries_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$')
);

CREATE UNIQUE INDEX countries_name_uq ON countries (lower(name));

CREATE TRIGGER countries_id_immutable_trg BEFORE UPDATE OF id ON countries
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER countries_no_truncate_trg BEFORE TRUNCATE ON countries
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE country_continents (
    country_id     text NOT NULL,
    continent_id   text NOT NULL,
    continent_type text NOT NULL DEFAULT 'modern',
    CONSTRAINT country_continents_pk PRIMARY KEY (country_id, continent_id),
    CONSTRAINT country_continents_country_fk FOREIGN KEY (country_id)
        REFERENCES countries (id) ON DELETE CASCADE,
    CONSTRAINT country_continents_continent_fk FOREIGN KEY (continent_id, continent_type)
        REFERENCES continents (id, type) ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT country_continents_continent_type_ck CHECK (continent_type = 'modern')
);

CREATE INDEX country_continents_continent_idx ON country_continents (continent_id, country_id);

CREATE TRIGGER country_continents_no_truncate_trg BEFORE TRUNCATE ON country_continents
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

-- A country keeps at least one continent link (checked at commit, research R10).
CREATE FUNCTION countries_assert_min_continents(p_country_id text) RETURNS void
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM paleo_require_safe_isolation();
    PERFORM 1 FROM countries WHERE id = p_country_id FOR NO KEY UPDATE;
    IF NOT FOUND THEN
        RETURN; -- the country was deleted (its links cascade)
    END IF;
    IF NOT EXISTS (SELECT 1 FROM country_continents WHERE country_id = p_country_id) THEN
        RAISE EXCEPTION 'a country must be linked to at least one continent'
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'countries_min_continents_ck',
                  TABLE = 'countries';
    END IF;
END
$$;

CREATE FUNCTION countries_min_continents_fn() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM countries_assert_min_continents(NEW.id);
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER countries_min_continents_trg AFTER INSERT ON countries
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION countries_min_continents_fn();

CREATE FUNCTION country_continents_min_fn() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM countries_assert_min_continents(OLD.country_id);
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER country_continents_min_trg AFTER DELETE OR UPDATE ON country_continents
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION country_continents_min_fn();

-- Species and their period, continent, and country links (data-model §5).

CREATE TABLE species (
    id              text    NOT NULL,
    genus_id        text    NOT NULL,
    name            text    NOT NULL,
    scientific_name text    NOT NULL,
    diet            text    NOT NULL,
    description     text    NOT NULL,
    discovery_year  integer,
    image_url       text,
    length_min_m    numeric,
    length_max_m    numeric,
    height_min_m    numeric,
    height_max_m    numeric,
    weight_min_kg   numeric,
    weight_max_kg   numeric,
    CONSTRAINT species_pk PRIMARY KEY (id),
    CONSTRAINT species_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    -- Common name: not unique.
    CONSTRAINT species_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT species_scientific_name_ck
        CHECK (char_length(scientific_name) BETWEEN 1 AND 128 AND scientific_name !~ '^\s|\s$'),
    CONSTRAINT species_diet_ck
        CHECK (diet IN ('carnivore', 'herbivore', 'omnivore', 'piscivore', 'insectivore')),
    CONSTRAINT species_description_ck
        CHECK (char_length(description) BETWEEN 1 AND 1000 AND description !~ '^\s|\s$'),
    CONSTRAINT species_image_url_ck
        CHECK (char_length(image_url) <= 2048 AND image_url ~ '^https?://' AND image_url !~ '\s$'),
    -- Upper bound (current UTC year) is species_discovery_year_trg: CHECKs must be immutable.
    CONSTRAINT species_discovery_year_ck CHECK (discovery_year >= 1600),
    -- Size: each measure is a NULL pair or a complete pair with 0 < min <= max, at most 6 decimals.
    CONSTRAINT species_length_ck CHECK (
        (length_min_m IS NULL) = (length_max_m IS NULL)
        AND length_min_m > 0 AND length_max_m < 'Infinity'
        AND length_min_m = round(length_min_m, 6) AND length_max_m = round(length_max_m, 6)
        AND length_min_m <= length_max_m),
    CONSTRAINT species_height_ck CHECK (
        (height_min_m IS NULL) = (height_max_m IS NULL)
        AND height_min_m > 0 AND height_max_m < 'Infinity'
        AND height_min_m = round(height_min_m, 6) AND height_max_m = round(height_max_m, 6)
        AND height_min_m <= height_max_m),
    CONSTRAINT species_weight_ck CHECK (
        (weight_min_kg IS NULL) = (weight_max_kg IS NULL)
        AND weight_min_kg > 0 AND weight_max_kg < 'Infinity'
        AND weight_min_kg = round(weight_min_kg, 6) AND weight_max_kg = round(weight_max_kg, 6)
        AND weight_min_kg <= weight_max_kg),
    CONSTRAINT species_genus_fk FOREIGN KEY (genus_id) REFERENCES genera (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX species_scientific_name_uq ON species (lower(scientific_name));
CREATE INDEX species_genus_idx ON species (genus_id, (lower(name) COLLATE paleo_name_sort), id);

CREATE TRIGGER species_id_immutable_trg BEFORE UPDATE OF id ON species
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER species_no_truncate_trg BEFORE TRUNCATE ON species
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

-- discovery_year is not after the current UTC year on the database clock (research R12).
CREATE FUNCTION species_discovery_year_fn() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.discovery_year > extract(year FROM now() AT TIME ZONE 'UTC') THEN
        RAISE EXCEPTION 'discovery_year must not be after the current UTC year'
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'species_discovery_year_not_future_ck',
                  TABLE = 'species',
                  COLUMN = 'discovery_year';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER species_discovery_year_trg BEFORE INSERT OR UPDATE OF discovery_year ON species
    FOR EACH ROW EXECUTE FUNCTION species_discovery_year_fn();

CREATE TABLE species_periods (
    species_id text NOT NULL,
    period_id  text NOT NULL,
    CONSTRAINT species_periods_pk PRIMARY KEY (species_id, period_id),
    CONSTRAINT species_periods_species_fk FOREIGN KEY (species_id)
        REFERENCES species (id) ON DELETE CASCADE,
    CONSTRAINT species_periods_period_fk FOREIGN KEY (period_id)
        REFERENCES periods (id) ON DELETE RESTRICT
);
CREATE INDEX species_periods_period_idx ON species_periods (period_id, species_id);
CREATE TRIGGER species_periods_no_truncate_trg BEFORE TRUNCATE ON species_periods
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE species_continents (
    species_id   text NOT NULL,
    continent_id text NOT NULL,
    CONSTRAINT species_continents_pk PRIMARY KEY (species_id, continent_id),
    CONSTRAINT species_continents_species_fk FOREIGN KEY (species_id)
        REFERENCES species (id) ON DELETE CASCADE,
    CONSTRAINT species_continents_continent_fk FOREIGN KEY (continent_id)
        REFERENCES continents (id) ON DELETE RESTRICT
);
CREATE INDEX species_continents_continent_idx ON species_continents (continent_id, species_id);
CREATE TRIGGER species_continents_no_truncate_trg BEFORE TRUNCATE ON species_continents
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE species_countries (
    species_id text NOT NULL,
    country_id text NOT NULL,
    CONSTRAINT species_countries_pk PRIMARY KEY (species_id, country_id),
    CONSTRAINT species_countries_species_fk FOREIGN KEY (species_id)
        REFERENCES species (id) ON DELETE CASCADE,
    CONSTRAINT species_countries_country_fk FOREIGN KEY (country_id)
        REFERENCES countries (id) ON DELETE RESTRICT
);
CREATE INDEX species_countries_country_idx ON species_countries (country_id, species_id);
CREATE TRIGGER species_countries_no_truncate_trg BEFORE TRUNCATE ON species_countries
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

-- A species keeps at least one period link (checked at commit, research R10).
CREATE FUNCTION species_assert_min_periods(p_species_id text) RETURNS void
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM paleo_require_safe_isolation();
    PERFORM 1 FROM species WHERE id = p_species_id FOR NO KEY UPDATE;
    IF NOT FOUND THEN
        RETURN; -- the species was deleted (its links cascade)
    END IF;
    IF NOT EXISTS (SELECT 1 FROM species_periods WHERE species_id = p_species_id) THEN
        RAISE EXCEPTION 'a species must be linked to at least one period'
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'species_min_periods_ck',
                  TABLE = 'species';
    END IF;
END
$$;

CREATE FUNCTION species_min_periods_fn() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM species_assert_min_periods(NEW.id);
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER species_min_periods_trg AFTER INSERT ON species
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION species_min_periods_fn();

CREATE FUNCTION species_periods_min_fn() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM species_assert_min_periods(OLD.species_id);
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER species_periods_min_trg AFTER DELETE OR UPDATE ON species_periods
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION species_periods_min_fn();

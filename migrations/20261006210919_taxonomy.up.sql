-- Taxonomy: seven rank tables, each with one required parent (data-model §4).

CREATE TABLE domains (
    id   text NOT NULL,
    name text NOT NULL,
    CONSTRAINT domains_pk PRIMARY KEY (id),
    CONSTRAINT domains_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT domains_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$')
);

CREATE UNIQUE INDEX domains_name_uq ON domains (lower(name));

CREATE TRIGGER domains_id_immutable_trg BEFORE UPDATE OF id ON domains
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER domains_no_truncate_trg BEFORE TRUNCATE ON domains
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE kingdoms (
    id   text NOT NULL,
    name text NOT NULL,
    domain_id text NOT NULL,
    CONSTRAINT kingdoms_pk PRIMARY KEY (id),
    CONSTRAINT kingdoms_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT kingdoms_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT kingdoms_domain_fk FOREIGN KEY (domain_id) REFERENCES domains (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX kingdoms_name_uq ON kingdoms (lower(name));
CREATE INDEX kingdoms_domain_idx ON kingdoms (domain_id, (lower(name) COLLATE paleo_name_sort), id);

CREATE TRIGGER kingdoms_id_immutable_trg BEFORE UPDATE OF id ON kingdoms
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER kingdoms_no_truncate_trg BEFORE TRUNCATE ON kingdoms
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE phyla (
    id   text NOT NULL,
    name text NOT NULL,
    kingdom_id text NOT NULL,
    CONSTRAINT phyla_pk PRIMARY KEY (id),
    CONSTRAINT phyla_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT phyla_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT phyla_kingdom_fk FOREIGN KEY (kingdom_id) REFERENCES kingdoms (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX phyla_name_uq ON phyla (lower(name));
CREATE INDEX phyla_kingdom_idx ON phyla (kingdom_id, (lower(name) COLLATE paleo_name_sort), id);

CREATE TRIGGER phyla_id_immutable_trg BEFORE UPDATE OF id ON phyla
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER phyla_no_truncate_trg BEFORE TRUNCATE ON phyla
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE classes (
    id   text NOT NULL,
    name text NOT NULL,
    phylum_id text NOT NULL,
    CONSTRAINT classes_pk PRIMARY KEY (id),
    CONSTRAINT classes_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT classes_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT classes_phylum_fk FOREIGN KEY (phylum_id) REFERENCES phyla (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX classes_name_uq ON classes (lower(name));
CREATE INDEX classes_phylum_idx ON classes (phylum_id, (lower(name) COLLATE paleo_name_sort), id);

CREATE TRIGGER classes_id_immutable_trg BEFORE UPDATE OF id ON classes
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER classes_no_truncate_trg BEFORE TRUNCATE ON classes
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE orders (
    id   text NOT NULL,
    name text NOT NULL,
    class_id text NOT NULL,
    CONSTRAINT orders_pk PRIMARY KEY (id),
    CONSTRAINT orders_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT orders_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT orders_class_fk FOREIGN KEY (class_id) REFERENCES classes (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX orders_name_uq ON orders (lower(name));
CREATE INDEX orders_class_idx ON orders (class_id, (lower(name) COLLATE paleo_name_sort), id);

CREATE TRIGGER orders_id_immutable_trg BEFORE UPDATE OF id ON orders
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER orders_no_truncate_trg BEFORE TRUNCATE ON orders
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE families (
    id   text NOT NULL,
    name text NOT NULL,
    order_id text NOT NULL,
    CONSTRAINT families_pk PRIMARY KEY (id),
    CONSTRAINT families_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT families_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT families_order_fk FOREIGN KEY (order_id) REFERENCES orders (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX families_name_uq ON families (lower(name));
CREATE INDEX families_order_idx ON families (order_id, (lower(name) COLLATE paleo_name_sort), id);

CREATE TRIGGER families_id_immutable_trg BEFORE UPDATE OF id ON families
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER families_no_truncate_trg BEFORE TRUNCATE ON families
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

CREATE TABLE genera (
    id   text NOT NULL,
    name text NOT NULL,
    family_id text NOT NULL,
    CONSTRAINT genera_pk PRIMARY KEY (id),
    CONSTRAINT genera_id_ck CHECK (char_length(id) BETWEEN 2 AND 64 AND id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT genera_name_ck CHECK (char_length(name) BETWEEN 1 AND 64 AND name !~ '^\s|\s$'),
    CONSTRAINT genera_family_fk FOREIGN KEY (family_id) REFERENCES families (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX genera_name_uq ON genera (lower(name));
CREATE INDEX genera_family_idx ON genera (family_id, (lower(name) COLLATE paleo_name_sort), id);

CREATE TRIGGER genera_id_immutable_trg BEFORE UPDATE OF id ON genera
    FOR EACH ROW EXECUTE FUNCTION paleo_forbid_id_change();
CREATE TRIGGER genera_no_truncate_trg BEFORE TRUNCATE ON genera
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

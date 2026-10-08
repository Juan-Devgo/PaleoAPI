-- Accounts managed by the operator tool (003 data-model §1, FR-001, FR-005, FR-006).

CREATE TABLE accounts (
    username               text        NOT NULL,
    password_hash          text        NOT NULL,
    role                   text,
    status                 text        NOT NULL DEFAULT 'active',
    credentials_version    integer     NOT NULL DEFAULT 1,
    credentials_changed_at timestamptz NOT NULL DEFAULT now(),
    created_at             timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT accounts_pk PRIMARY KEY (username),
    CONSTRAINT accounts_username_ck CHECK (char_length(username) BETWEEN 2 AND 64 AND username ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    -- Shape only: the database never sees a plaintext (003 research R9).
    CONSTRAINT accounts_password_hash_ck CHECK (password_hash ~ '^\$argon2id\$v=19\$m=[0-9]+,t=[0-9]+,p=[0-9]+\$[A-Za-z0-9+/]+\$[A-Za-z0-9+/]+$'),
    CONSTRAINT accounts_role_ck CHECK (role = 'admin'),
    CONSTRAINT accounts_status_ck CHECK (status IN ('active', 'disabled')),
    CONSTRAINT accounts_credentials_version_ck CHECK (credentials_version >= 1)
);

-- A password or status change invalidates earlier tokens; a role change does not (research R7).
CREATE FUNCTION accounts_credentials_changed_fn() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    NEW.credentials_version := OLD.credentials_version + 1;
    NEW.credentials_changed_at := now();
    RETURN NEW;
END
$$;

CREATE TRIGGER accounts_credentials_changed_trg BEFORE UPDATE OF password_hash, status ON accounts
    FOR EACH ROW
    WHEN (OLD.password_hash IS DISTINCT FROM NEW.password_hash OR OLD.status IS DISTINCT FROM NEW.status)
    EXECUTE FUNCTION accounts_credentials_changed_fn();
CREATE TRIGGER accounts_no_truncate_trg BEFORE TRUNCATE ON accounts
    FOR EACH STATEMENT EXECUTE FUNCTION paleo_forbid_truncate();

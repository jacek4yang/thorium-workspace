-- Initial schema.
--
-- Rules that apply throughout:
--   * ids are lowercase UUID text
--   * timestamps are Unix epoch seconds (UTC)
--   * a *_ref column holds a vault secret reference, never a secret
--   * every child row is removed with its parent via ON DELETE CASCADE

CREATE TABLE workspace_settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE accounts (
    id                TEXT PRIMARY KEY NOT NULL,
    display_name      TEXT NOT NULL,
    service_kind      TEXT NOT NULL,
    service_label     TEXT NOT NULL DEFAULT '',
    username          TEXT,
    email             TEXT,
    login_url         TEXT,
    notes             TEXT NOT NULL DEFAULT '',
    password_ref      TEXT,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_accounts_display_name ON accounts (display_name);

CREATE TABLE account_tags (
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    tag        TEXT NOT NULL,
    PRIMARY KEY (account_id, tag)
) STRICT;

CREATE TABLE browser_profiles (
    id                TEXT PRIMARY KEY NOT NULL,
    name              TEXT NOT NULL,
    thorium_mode      TEXT NOT NULL,
    thorium_version   TEXT,
    user_data_dir     TEXT NOT NULL UNIQUE,
    startup_urls      TEXT NOT NULL DEFAULT '[]',
    locale            TEXT NOT NULL,
    timezone          TEXT NOT NULL,
    notes             TEXT NOT NULL DEFAULT '',
    -- Reserved for a future release that adds network routing. v1.0.0 always
    -- writes NULL; the column exists so adding routing needs no migration.
    network_route_id  TEXT,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
) STRICT;

CREATE TABLE profile_accounts (
    profile_id TEXT NOT NULL REFERENCES browser_profiles (id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, account_id)
) STRICT;

CREATE INDEX idx_profile_accounts_account ON profile_accounts (account_id);

CREATE TABLE account_factors (
    id            TEXT PRIMARY KEY NOT NULL,
    account_id    TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    label         TEXT NOT NULL,
    kind          TEXT NOT NULL,
    otp_kind      TEXT,
    algorithm     TEXT,
    digits        INTEGER,
    period_seconds INTEGER,
    counter       INTEGER,
    issuer        TEXT,
    account_label TEXT,
    seed_ref      TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_account_factors_account ON account_factors (account_id);

CREATE TABLE recovery_codes (
    id         TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    code_ref   TEXT NOT NULL,
    position   INTEGER NOT NULL,
    used       INTEGER NOT NULL DEFAULT 0,
    used_at    INTEGER,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_recovery_codes_account ON recovery_codes (account_id, position);

CREATE TABLE thorium_installations (
    version         TEXT PRIMARY KEY NOT NULL,
    channel         TEXT NOT NULL,
    install_dir     TEXT NOT NULL,
    executable_path TEXT NOT NULL,
    installed_at    INTEGER NOT NULL,
    source_url      TEXT NOT NULL DEFAULT '',
    archive_sha256  TEXT NOT NULL DEFAULT '',
    is_current      INTEGER NOT NULL DEFAULT 0
) STRICT;

-- At most one installation may be marked current.
CREATE UNIQUE INDEX idx_thorium_single_current
    ON thorium_installations (is_current)
    WHERE is_current = 1;

-- Observed runtime state. Rebuilt at startup; never the sole copy of anything
-- the user configured.
CREATE TABLE runtime_sessions (
    profile_id      TEXT PRIMARY KEY NOT NULL REFERENCES browser_profiles (id) ON DELETE CASCADE,
    status          TEXT NOT NULL,
    pid             INTEGER,
    cdp_port        INTEGER,
    thorium_version TEXT,
    started_at      INTEGER,
    updated_at      INTEGER NOT NULL
) STRICT;

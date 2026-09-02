-- Schema version 1: initial workspace metadata schema.
--
-- Only non-secret metadata is persisted here. Secret values (passwords,
-- OTP seeds, recovery code texts) live exclusively in the encrypted vault
-- and are referenced by structured `SecretRef` strings.

CREATE TABLE workspace_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    -- JSON-serialized WorkspaceSettings (self-describing, versioned via
    -- serde defaults).
    data TEXT NOT NULL
);

CREATE TABLE profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    selection TEXT NOT NULL CHECK (selection IN ('current', 'pinned')),
    pinned_version TEXT,
    user_data_rel_path TEXT NOT NULL UNIQUE,
    startup_urls TEXT NOT NULL,
    locale TEXT,
    timezone TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_launched_at TEXT,
    CHECK (selection != 'pinned' OR (pinned_version IS NOT NULL AND pinned_version <> '')),
    CHECK (selection != 'current' OR pinned_version IS NULL)
);

CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    display_name TEXT NOT NULL,
    service_id TEXT NOT NULL,
    -- Present only for the `custom` service kind.
    service_label TEXT,
    username TEXT,
    email TEXT,
    login_url TEXT,
    -- JSON array of non-secret tag strings.
    tags TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    -- Structured vault reference (never a secret value).
    password_secret_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_accounts_profile_id ON accounts (profile_id);

CREATE TABLE factors (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('totp', 'hotp', 'external')),
    label TEXT,
    issuer TEXT,
    account_label TEXT,
    algorithm TEXT,
    digits INTEGER,
    period_seconds INTEGER,
    counter INTEGER,
    -- Structured vault reference for the seed (TOTP/HOTP only).
    secret_ref TEXT,
    -- Description of an external authenticator (kind = 'external' only).
    external_note TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_factors_account_id ON factors (account_id);

CREATE TABLE recovery_codes (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0 CHECK (used IN (0, 1)),
    marked_used_at TEXT,
    -- Structured vault reference for the code value.
    secret_ref TEXT NOT NULL,
    UNIQUE (account_id, position)
);

CREATE TABLE thorium_installs (
    version TEXT NOT NULL,
    variant TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    installed_at TEXT NOT NULL,
    PRIMARY KEY (version, variant)
);

CREATE TABLE runtime_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

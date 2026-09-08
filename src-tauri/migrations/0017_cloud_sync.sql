-- Local state for account-scoped incremental cloud synchronization.
-- Credentials are never stored here; refresh tokens live in a DPAPI-protected file.

CREATE TABLE cloud_database_binding (
    singleton_id INTEGER NOT NULL PRIMARY KEY CHECK (singleton_id = 1),
    account_id TEXT NOT NULL CHECK (length(account_id) = 36),
    username TEXT NOT NULL CHECK (trim(username) <> ''),
    bound_at_ms INTEGER NOT NULL CHECK (bound_at_ms >= 0)
) STRICT;

CREATE TABLE cloud_sync_meta (
    account_id TEXT NOT NULL PRIMARY KEY CHECK (length(account_id) = 36),
    pull_cursor INTEGER NOT NULL DEFAULT 0 CHECK (pull_cursor >= 0),
    last_sync_at_ms INTEGER CHECK (last_sync_at_ms IS NULL OR last_sync_at_ms >= 0),
    last_error TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;

CREATE TABLE cloud_sync_state (
    account_id TEXT NOT NULL CHECK (length(account_id) = 36),
    entity_kind TEXT NOT NULL CHECK (
        entity_kind IN ('question_type', 'subject', 'chapter', 'tag', 'resource', 'question')
    ),
    entity_id TEXT NOT NULL CHECK (trim(entity_id) <> ''),
    server_revision INTEGER NOT NULL CHECK (server_revision >= 1),
    deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    synced_hash TEXT NOT NULL CHECK (length(synced_hash) = 64),
    synced_payload_json TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (account_id, entity_kind, entity_id)
) STRICT;

CREATE TABLE cloud_sync_conflicts (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    account_id TEXT NOT NULL CHECK (length(account_id) = 36),
    entity_kind TEXT NOT NULL CHECK (
        entity_kind IN ('question_type', 'subject', 'chapter', 'tag', 'resource', 'question')
    ),
    entity_id TEXT NOT NULL CHECK (trim(entity_id) <> ''),
    local_payload_json TEXT,
    remote_payload_json TEXT,
    detected_at_ms INTEGER NOT NULL CHECK (detected_at_ms >= 0),
    resolved_at_ms INTEGER CHECK (resolved_at_ms IS NULL OR resolved_at_ms >= 0),
    message TEXT NOT NULL CHECK (trim(message) <> '')
) STRICT;

CREATE INDEX cloud_sync_state_account_kind_idx
    ON cloud_sync_state(account_id, entity_kind, entity_id);

CREATE INDEX cloud_sync_conflicts_open_idx
    ON cloud_sync_conflicts(account_id, detected_at_ms DESC)
    WHERE resolved_at_ms IS NULL;

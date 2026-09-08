-- A local SQLite identifier is intentionally not required to equal the
-- canonical identifier chosen by the cloud.  This lets two offline computers
-- create the same subject/chapter/tag with different UUIDs and later converge
-- without rewriting the local question-bank primary keys.

CREATE TABLE cloud_entity_mappings (
    account_id TEXT NOT NULL CHECK (length(account_id) = 36),
    entity_kind TEXT NOT NULL CHECK (
        entity_kind IN ('question_type', 'subject', 'chapter', 'tag', 'resource', 'question')
    ),
    local_entity_id TEXT NOT NULL CHECK (trim(local_entity_id) <> ''),
    cloud_entity_id TEXT NOT NULL CHECK (trim(cloud_entity_id) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (account_id, entity_kind, local_entity_id),
    UNIQUE (account_id, entity_kind, cloud_entity_id)
) STRICT;

CREATE INDEX cloud_entity_mappings_cloud_idx
    ON cloud_entity_mappings(account_id, entity_kind, cloud_entity_id);

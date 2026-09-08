-- Automatic local backups. Only records explicitly marked as automatic are
-- eligible for retention cleanup; manual and safety backups are never pruned.

ALTER TABLE app_settings
    ADD COLUMN automatic_backup_enabled INTEGER NOT NULL DEFAULT 1
        CHECK (automatic_backup_enabled IN (0, 1));

ALTER TABLE app_settings
    ADD COLUMN automatic_backup_interval_days INTEGER NOT NULL DEFAULT 7
        CHECK (automatic_backup_interval_days BETWEEN 1 AND 365);

ALTER TABLE app_settings
    ADD COLUMN automatic_backup_retention_count INTEGER NOT NULL DEFAULT 5
        CHECK (automatic_backup_retention_count BETWEEN 1 AND 50);

CREATE TABLE backup_records_v13 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    backup_kind TEXT NOT NULL CHECK (
        backup_kind IN ('manual', 'automatic', 'pre_restore', 'pre_migration', 'data_move')
    ),
    status TEXT NOT NULL CHECK (
        status IN ('creating', 'ready', 'failed', 'missing', 'invalid')
    ),
    archive_rel_path TEXT,
    display_filename TEXT NOT NULL CHECK (trim(display_filename) <> ''),
    format_version INTEGER NOT NULL CHECK (format_version >= 1),
    database_schema_version INTEGER NOT NULL CHECK (database_schema_version >= 0),
    source_database_uuid TEXT NOT NULL CHECK (length(source_database_uuid) = 36),
    source_app_version TEXT NOT NULL CHECK (trim(source_app_version) <> ''),
    archive_sha256 BLOB CHECK (archive_sha256 IS NULL OR length(archive_sha256) = 32),
    manifest_sha256 BLOB CHECK (manifest_sha256 IS NULL OR length(manifest_sha256) = 32),
    archive_byte_size INTEGER CHECK (archive_byte_size IS NULL OR archive_byte_size >= 0),
    contents_summary_json TEXT NOT NULL CHECK (json_valid(contents_summary_json)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    completed_at_ms INTEGER CHECK (completed_at_ms IS NULL OR completed_at_ms >= 0),
    last_verified_at_ms INTEGER CHECK (last_verified_at_ms IS NULL OR last_verified_at_ms >= 0),
    error_code TEXT,
    error_message TEXT
) STRICT;

INSERT INTO backup_records_v13 SELECT * FROM backup_records;
DROP TABLE backup_records;
ALTER TABLE backup_records_v13 RENAME TO backup_records;

CREATE INDEX idx_backup_records_created
    ON backup_records(created_at_ms DESC, id);

CREATE INDEX idx_backup_records_status
    ON backup_records(status, created_at_ms DESC);

CREATE INDEX idx_backup_records_automatic_ready
    ON backup_records(completed_at_ms DESC, id DESC)
    WHERE backup_kind = 'automatic' AND status = 'ready';

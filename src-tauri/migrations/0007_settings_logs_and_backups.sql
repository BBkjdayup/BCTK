-- Strongly typed application settings, diagnostics and backup catalogue metadata.

CREATE TABLE app_settings (
    singleton_id INTEGER NOT NULL PRIMARY KEY CHECK (singleton_id = 1),
    default_export_directory TEXT,
    default_template_id TEXT CHECK (default_template_id IS NULL OR length(default_template_id) = 36),
    export_filename_pattern TEXT NOT NULL DEFAULT '{title}_{date}'
        CHECK (trim(export_filename_pattern) <> ''),
    optional_confirmations_json TEXT NOT NULL DEFAULT '{}'
        CHECK (json_valid(optional_confirmations_json)),
    recent_unused_days INTEGER CHECK (recent_unused_days IS NULL OR recent_unused_days > 0),
    recent_added_days INTEGER CHECK (recent_added_days IS NULL OR recent_added_days > 0),
    recent_used_days INTEGER CHECK (recent_used_days IS NULL OR recent_used_days > 0),
    recycle_retention_days INTEGER NOT NULL DEFAULT 30 CHECK (recycle_retention_days > 0),
    recycle_policy TEXT NOT NULL DEFAULT 'remind_only'
        CHECK (recycle_policy IN ('manual_only', 'remind_only')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT fk_app_settings_default_template
        FOREIGN KEY (default_template_id) REFERENCES word_templates(id) ON DELETE SET NULL
) STRICT;

CREATE TABLE operation_logs (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms >= 0),
    level TEXT NOT NULL CHECK (level IN ('info', 'warning', 'error')),
    action TEXT NOT NULL CHECK (trim(action) <> ''),
    entity_type TEXT,
    entity_id TEXT,
    correlation_id TEXT,
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'partial', 'failure')),
    summary TEXT NOT NULL,
    details_json TEXT NOT NULL CHECK (json_valid(details_json)),
    error_code TEXT,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    app_version TEXT NOT NULL CHECK (trim(app_version) <> '')
) STRICT;

CREATE TABLE backup_records (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    backup_kind TEXT NOT NULL CHECK (
        backup_kind IN ('manual', 'pre_restore', 'pre_migration', 'data_move')
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

INSERT INTO app_settings (
    singleton_id,
    default_export_directory,
    default_template_id,
    export_filename_pattern,
    optional_confirmations_json,
    recent_unused_days,
    recent_added_days,
    recent_used_days,
    recycle_retention_days,
    recycle_policy,
    created_at_ms,
    updated_at_ms
) VALUES (
    1,
    NULL,
    NULL,
    '{title}_{date}',
    '{}',
    NULL,
    NULL,
    NULL,
    30,
    'remind_only',
    CAST(strftime('%s', 'now') AS INTEGER) * 1000,
    CAST(strftime('%s', 'now') AS INTEGER) * 1000
);

CREATE INDEX idx_operation_logs_time
    ON operation_logs(occurred_at_ms DESC, id);

CREATE INDEX idx_operation_logs_action_time
    ON operation_logs(action, occurred_at_ms DESC);

CREATE INDEX idx_operation_logs_correlation
    ON operation_logs(correlation_id)
    WHERE correlation_id IS NOT NULL;

CREATE INDEX idx_backup_records_created
    ON backup_records(created_at_ms DESC, id);

CREATE INDEX idx_backup_records_status
    ON backup_records(status, created_at_ms DESC);

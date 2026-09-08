-- Managed Word template files and independently versioned replacement regions.

CREATE TABLE word_templates (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    name_key TEXT NOT NULL UNIQUE CHECK (trim(name_key) <> ''),
    file_rel_path TEXT NOT NULL UNIQUE CHECK (trim(file_rel_path) <> ''),
    file_sha256 BLOB NOT NULL CHECK (length(file_sha256) = 32),
    file_byte_size INTEGER NOT NULL CHECK (file_byte_size >= 0),
    analysis_status TEXT NOT NULL CHECK (
        analysis_status IN ('pending', 'ready', 'warning', 'failed')
    ),
    analysis_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (analysis_schema_version >= 1),
    analysis_json TEXT NOT NULL CHECK (json_valid(analysis_json)),
    parser_version TEXT NOT NULL CHECK (trim(parser_version) <> ''),
    row_version INTEGER NOT NULL DEFAULT 1 CHECK (row_version >= 1),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    last_verified_at_ms INTEGER CHECK (last_verified_at_ms IS NULL OR last_verified_at_ms >= 0)
) STRICT;

CREATE TABLE template_regions (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    template_id TEXT NOT NULL CHECK (length(template_id) = 36),
    region_kind TEXT NOT NULL CHECK (region_kind IN ('questions', 'answers', 'explanations')),
    detection_method TEXT NOT NULL CHECK (detection_method IN ('auto', 'manual')),
    locator_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (locator_schema_version >= 1),
    start_locator_json TEXT NOT NULL CHECK (json_valid(start_locator_json)),
    end_locator_json TEXT NOT NULL CHECK (json_valid(end_locator_json)),
    confidence REAL CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT fk_template_regions_template
        FOREIGN KEY (template_id) REFERENCES word_templates(id) ON DELETE CASCADE
) STRICT;

CREATE UNIQUE INDEX uq_template_active_region
    ON template_regions(template_id, region_kind)
    WHERE is_active = 1;

CREATE INDEX idx_word_templates_hash
    ON word_templates(file_sha256);

CREATE INDEX idx_word_templates_updated
    ON word_templates(updated_at_ms DESC, id);


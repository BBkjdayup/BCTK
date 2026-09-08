-- Stable database identity and the subject/chapter/tag taxonomy.
-- All timestamps are UTC Unix milliseconds and are supplied by Rust.

CREATE TABLE app_meta (
    singleton_id INTEGER NOT NULL PRIMARY KEY CHECK (singleton_id = 1),
    database_uuid TEXT NOT NULL UNIQUE CHECK (length(database_uuid) = 36),
    created_by_app_version TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
) STRICT;

CREATE TABLE subjects (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    name_key TEXT NOT NULL UNIQUE CHECK (trim(name_key) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0,
    last_accessed_at_ms INTEGER CHECK (last_accessed_at_ms IS NULL OR last_accessed_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;

CREATE TABLE chapters (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    subject_id TEXT NOT NULL CHECK (length(subject_id) = 36),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    name_key TEXT NOT NULL CHECK (trim(name_key) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0,
    last_accessed_at_ms INTEGER CHECK (last_accessed_at_ms IS NULL OR last_accessed_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT fk_chapters_subject
        FOREIGN KEY (subject_id) REFERENCES subjects(id) ON DELETE RESTRICT,
    CONSTRAINT uq_chapters_subject_name UNIQUE (subject_id, name_key),
    CONSTRAINT uq_chapters_id_subject UNIQUE (id, subject_id)
) STRICT;

CREATE TABLE tags (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    name_key TEXT NOT NULL UNIQUE CHECK (trim(name_key) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;

CREATE INDEX idx_subjects_sort
    ON subjects(sort_order, id);

CREATE INDEX idx_chapters_subject_sort
    ON chapters(subject_id, sort_order, id);


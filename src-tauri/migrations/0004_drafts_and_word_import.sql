-- Recoverable autosave drafts and per-item Word import state.

CREATE TABLE drafts (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    draft_key TEXT NOT NULL UNIQUE CHECK (trim(draft_key) <> ''),
    draft_kind TEXT NOT NULL CHECK (
        draft_kind IN ('question_create', 'question_edit', 'word_import_preview')
    ),
    target_question_id TEXT CHECK (target_question_id IS NULL OR length(target_question_id) = 36),
    base_content_version INTEGER CHECK (base_content_version IS NULL OR base_content_version >= 1),
    payload_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (payload_schema_version >= 1),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    source_title TEXT,
    autosaved_at_ms INTEGER NOT NULL CHECK (autosaved_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT fk_drafts_target_question
        FOREIGN KEY (target_question_id) REFERENCES questions(id) ON DELETE SET NULL
) STRICT;

CREATE TABLE draft_resource_refs (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    draft_id TEXT NOT NULL CHECK (length(draft_id) = 36),
    resource_id TEXT NOT NULL CHECK (length(resource_id) = 36),
    node_id TEXT NOT NULL CHECK (trim(node_id) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CONSTRAINT fk_draft_resource_draft
        FOREIGN KEY (draft_id) REFERENCES drafts(id) ON DELETE CASCADE,
    CONSTRAINT fk_draft_resource_resource
        FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE RESTRICT,
    CONSTRAINT uq_draft_resource_node UNIQUE (draft_id, node_id)
) STRICT;

CREATE TABLE word_import_sessions (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    draft_id TEXT NOT NULL UNIQUE CHECK (length(draft_id) = 36),
    source_filename TEXT NOT NULL CHECK (trim(source_filename) <> ''),
    source_sha256 BLOB NOT NULL CHECK (length(source_sha256) = 32),
    source_byte_size INTEGER NOT NULL CHECK (source_byte_size >= 0),
    parser_version TEXT NOT NULL CHECK (trim(parser_version) <> ''),
    status TEXT NOT NULL CHECK (
        status IN ('parsing', 'reviewing', 'importing', 'partial', 'completed', 'failed')
    ),
    diagnostics_json TEXT NOT NULL CHECK (json_valid(diagnostics_json)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    completed_at_ms INTEGER CHECK (completed_at_ms IS NULL OR completed_at_ms >= 0),
    CONSTRAINT fk_word_import_session_draft
        FOREIGN KEY (draft_id) REFERENCES drafts(id) ON DELETE CASCADE
) STRICT;

CREATE TABLE word_import_items (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    session_id TEXT NOT NULL CHECK (length(session_id) = 36),
    source_ordinal INTEGER NOT NULL CHECK (source_ordinal >= 0),
    selected INTEGER NOT NULL DEFAULT 1 CHECK (selected IN (0, 1)),
    item_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (item_schema_version >= 1),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    subject_id TEXT CHECK (subject_id IS NULL OR length(subject_id) = 36),
    chapter_id TEXT CHECK (chapter_id IS NULL OR length(chapter_id) = 36),
    validation_json TEXT NOT NULL CHECK (json_valid(validation_json)),
    duplicate_kind TEXT NOT NULL DEFAULT 'none'
        CHECK (duplicate_kind IN ('none', 'exact', 'suspected')),
    duplicate_question_id TEXT CHECK (
        duplicate_question_id IS NULL OR length(duplicate_question_id) = 36
    ),
    duplicate_action TEXT CHECK (
        duplicate_action IS NULL OR duplicate_action IN ('skip', 'overwrite', 'keep')
    ),
    item_status TEXT NOT NULL CHECK (
        item_status IN ('review', 'removed', 'imported', 'failed', 'skipped')
    ),
    imported_question_id TEXT CHECK (
        imported_question_id IS NULL OR length(imported_question_id) = 36
    ),
    error_code TEXT,
    error_message TEXT,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT ck_word_import_item_chapter_subject CHECK (
        chapter_id IS NULL OR subject_id IS NOT NULL
    ),
    CONSTRAINT fk_word_import_item_session
        FOREIGN KEY (session_id) REFERENCES word_import_sessions(id) ON DELETE CASCADE,
    CONSTRAINT fk_word_import_item_subject
        FOREIGN KEY (subject_id) REFERENCES subjects(id) ON DELETE RESTRICT,
    CONSTRAINT fk_word_import_item_chapter_subject
        FOREIGN KEY (chapter_id, subject_id)
        REFERENCES chapters(id, subject_id)
        ON DELETE RESTRICT,
    CONSTRAINT fk_word_import_item_duplicate_question
        FOREIGN KEY (duplicate_question_id) REFERENCES questions(id) ON DELETE SET NULL,
    CONSTRAINT fk_word_import_item_imported_question
        FOREIGN KEY (imported_question_id) REFERENCES questions(id) ON DELETE SET NULL,
    CONSTRAINT uq_word_import_item_ordinal UNIQUE (session_id, source_ordinal)
) STRICT;

CREATE TABLE word_import_item_resource_refs (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    import_item_id TEXT NOT NULL CHECK (length(import_item_id) = 36),
    resource_id TEXT NOT NULL CHECK (length(resource_id) = 36),
    node_id TEXT NOT NULL CHECK (trim(node_id) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CONSTRAINT fk_word_import_item_resource_item
        FOREIGN KEY (import_item_id) REFERENCES word_import_items(id) ON DELETE CASCADE,
    CONSTRAINT fk_word_import_item_resource_resource
        FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE RESTRICT,
    CONSTRAINT uq_word_import_item_resource_node UNIQUE (import_item_id, node_id)
) STRICT;

CREATE INDEX idx_drafts_autosaved
    ON drafts(autosaved_at_ms DESC, id);

CREATE INDEX idx_draft_resource_refs_resource
    ON draft_resource_refs(resource_id, draft_id);

CREATE INDEX idx_word_import_sessions_status
    ON word_import_sessions(status, updated_at_ms DESC);

CREATE INDEX idx_word_import_items_session_status
    ON word_import_items(session_id, item_status, source_ordinal);

CREATE INDEX idx_word_import_item_resources_resource
    ON word_import_item_resource_refs(resource_id, import_item_id);


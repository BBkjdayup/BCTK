-- Recoverable multi-question document-entry drafts.

CREATE TABLE document_question_drafts (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    draft_key TEXT NOT NULL UNIQUE CHECK (draft_key = 'question_document:active'),
    payload_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (payload_schema_version = 1),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    autosaved_at_ms INTEGER NOT NULL CHECK (autosaved_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;

CREATE INDEX idx_document_question_drafts_autosaved
    ON document_question_drafts(autosaved_at_ms DESC, id);

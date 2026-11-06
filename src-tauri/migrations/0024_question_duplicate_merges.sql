CREATE TABLE question_duplicate_merges (
    id TEXT PRIMARY KEY NOT NULL,
    keeper_id TEXT NOT NULL,
    undo_json TEXT NOT NULL CHECK (json_valid(undo_json)),
    created_at_ms INTEGER NOT NULL,
    undone_at_ms INTEGER
);
CREATE INDEX idx_question_duplicate_merges_recent ON question_duplicate_merges(created_at_ms DESC, id DESC)
    WHERE undone_at_ms IS NULL;

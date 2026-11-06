CREATE TABLE question_history (
 id TEXT PRIMARY KEY NOT NULL,
 question_id TEXT NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
 content_version INTEGER NOT NULL, saved_at_ms INTEGER NOT NULL,
 snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
 UNIQUE(question_id,content_version)
) STRICT;
CREATE INDEX idx_question_history_recent ON question_history(question_id,content_version DESC);
CREATE TABLE question_history_resource_refs (
 history_id TEXT NOT NULL REFERENCES question_history(id) ON DELETE CASCADE,
 resource_id TEXT NOT NULL REFERENCES resources(id) ON DELETE RESTRICT,
 PRIMARY KEY(history_id,resource_id)
) STRICT;
CREATE TABLE question_batch_edits (
 id TEXT PRIMARY KEY NOT NULL, created_at_ms INTEGER NOT NULL,
 undo_json TEXT NOT NULL CHECK(json_valid(undo_json)), undone_at_ms INTEGER
) STRICT;
CREATE TABLE saved_question_filters (
 id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL UNIQUE,
 filters_json TEXT NOT NULL CHECK(json_valid(filters_json)), created_at_ms INTEGER NOT NULL
) STRICT;
DROP VIEW resource_reference_counts;
DROP VIEW resource_all_refs;
CREATE VIEW resource_all_refs AS
 SELECT 'question' AS owner_kind,id AS reference_id,question_id AS owner_id,resource_id FROM question_resource_refs
 UNION ALL SELECT 'paper_item',id,paper_item_id,resource_id FROM paper_item_resource_refs
 UNION ALL SELECT 'draft',id,draft_id,resource_id FROM draft_resource_refs
 UNION ALL SELECT 'word_import_item',id,import_item_id,resource_id FROM word_import_item_resource_refs
 UNION ALL SELECT 'paper_recovery',resource_id,CAST(slot AS TEXT),resource_id FROM paper_recovery_resource_refs
 UNION ALL SELECT 'question_history',history_id||':'||resource_id,history_id,resource_id FROM question_history_resource_refs;
CREATE VIEW resource_reference_counts AS SELECT resource_id,COUNT(*) AS reference_count FROM resource_all_refs GROUP BY resource_id;

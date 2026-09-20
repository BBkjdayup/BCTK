-- A local working copy, independent of saved paper history and cloud sync.
CREATE TABLE paper_recovery (
    slot INTEGER NOT NULL PRIMARY KEY CHECK (slot = 1),
    revision TEXT NOT NULL CHECK (length(revision) = 36),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    autosaved_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE paper_recovery_resource_refs (
    resource_id TEXT NOT NULL PRIMARY KEY REFERENCES resources(id) ON DELETE RESTRICT,
    slot INTEGER NOT NULL DEFAULT 1 REFERENCES paper_recovery(slot) ON DELETE CASCADE
) STRICT;

DROP VIEW resource_reference_counts;
DROP VIEW resource_all_refs;
CREATE VIEW resource_all_refs AS
    SELECT 'question' AS owner_kind, id AS reference_id, question_id AS owner_id, resource_id FROM question_resource_refs
    UNION ALL SELECT 'paper_item', id, paper_item_id, resource_id FROM paper_item_resource_refs
    UNION ALL SELECT 'draft', id, draft_id, resource_id FROM draft_resource_refs
    UNION ALL SELECT 'word_import_item', id, import_item_id, resource_id FROM word_import_item_resource_refs
    UNION ALL SELECT 'paper_recovery', resource_id, CAST(slot AS TEXT), resource_id FROM paper_recovery_resource_refs;
CREATE VIEW resource_reference_counts AS
    SELECT resource_id, COUNT(*) AS reference_count FROM resource_all_refs GROUP BY resource_id;

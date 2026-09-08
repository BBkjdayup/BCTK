-- A missing parent entity during paged pull is a retryable dependency problem,
-- not a user-facing edit conflict. Keep those payloads durable across crashes
-- and hide them from the manual local/cloud choice until automatic replay has
-- had a chance to restore their dependencies.

ALTER TABLE cloud_sync_conflicts
ADD COLUMN conflict_kind TEXT NOT NULL DEFAULT 'content'
CHECK (conflict_kind IN ('content', 'deferred_apply'));

-- v0.1.75 recorded paged foreign-key failures as ordinary conflicts. They all
-- have no local payload because the remote-only entity could not be inserted.
UPDATE cloud_sync_conflicts
SET conflict_kind = 'deferred_apply'
WHERE resolved_at_ms IS NULL
  AND local_payload_json IS NULL
  AND (
      instr(message, 'FOREIGN KEY constraint failed') > 0
      OR instr(message, '(code: 787)') > 0
  );

CREATE INDEX cloud_sync_conflicts_kind_open_idx
    ON cloud_sync_conflicts(account_id, conflict_kind, detected_at_ms DESC, id)
    WHERE resolved_at_ms IS NULL;

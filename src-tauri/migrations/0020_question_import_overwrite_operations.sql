-- Durable idempotency receipts for atomic imported-question overwrite batches.

CREATE TABLE question_import_overwrite_operations (
    operation_id TEXT NOT NULL PRIMARY KEY CHECK (length(operation_id) = 36),
    request_sha256_hex TEXT NOT NULL CHECK (length(request_sha256_hex) = 64),
    result_json TEXT CHECK (result_json IS NULL OR json_valid(result_json)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    completed_at_ms INTEGER CHECK (completed_at_ms IS NULL OR completed_at_ms >= created_at_ms)
) STRICT;

CREATE INDEX idx_question_import_overwrite_operations_created
    ON question_import_overwrite_operations(created_at_ms DESC, operation_id);

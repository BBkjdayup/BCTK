-- The Rust migration stage rebuilds version 1 fingerprints and searchable
-- formula text on the candidate database before atomically switching files.
CREATE INDEX idx_questions_fingerprint_upgrade ON questions(fingerprint_version, id);
ALTER TABLE questions ADD COLUMN fingerprint_comparable INTEGER NOT NULL DEFAULT 0 CHECK (fingerprint_comparable IN (0, 1));

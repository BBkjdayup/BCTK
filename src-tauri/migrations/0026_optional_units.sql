-- Optional third-level classification. Existing rows remain unassigned.
CREATE TABLE units (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE RESTRICT,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    name_key TEXT NOT NULL CHECK (trim(name_key) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    UNIQUE (chapter_id, name_key)
) STRICT;

ALTER TABLE questions ADD COLUMN unit_id TEXT DEFAULT NULL
    REFERENCES units(id) ON DELETE RESTRICT
    CHECK (unit_id IS NULL OR length(unit_id) = 36);

CREATE INDEX idx_units_chapter_sort ON units(chapter_id, sort_order, id);
CREATE INDEX idx_questions_unit ON questions(unit_id, deleted_at_ms, updated_at_ms DESC, id);

CREATE TRIGGER questions_unit_insert BEFORE INSERT ON questions
WHEN NEW.unit_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'unit does not belong to question chapter')
    WHERE NOT EXISTS (SELECT 1 FROM units WHERE id = NEW.unit_id AND chapter_id = NEW.chapter_id);
END;

CREATE TRIGGER questions_unit_update BEFORE UPDATE OF unit_id, chapter_id ON questions
WHEN NEW.unit_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'unit does not belong to question chapter')
    WHERE NOT EXISTS (SELECT 1 FROM units WHERE id = NEW.unit_id AND chapter_id = NEW.chapter_id);
END;

CREATE TRIGGER units_chapter_update BEFORE UPDATE OF chapter_id ON units
WHEN NEW.chapter_id <> OLD.chapter_id
BEGIN
    SELECT RAISE(ABORT, 'move unit questions before changing its chapter')
    WHERE EXISTS (SELECT 1 FROM questions WHERE unit_id = OLD.id);
END;

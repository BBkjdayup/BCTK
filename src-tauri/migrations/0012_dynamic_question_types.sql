-- User-managed question types. Stable codes are stored on questions and paper snapshots;
-- names, aliases and editor behaviour are managed independently.

PRAGMA defer_foreign_keys = ON;

CREATE TABLE question_types (
    code TEXT NOT NULL PRIMARY KEY CHECK (
        length(code) BETWEEN 2 AND 64
        AND code NOT GLOB '*[^a-z0-9_]*'
    ),
    name TEXT NOT NULL CHECK (trim(name) <> '' AND length(name) <= 40),
    name_key TEXT NOT NULL UNIQUE CHECK (trim(name_key) <> ''),
    behavior TEXT NOT NULL CHECK (
        behavior IN ('single_choice', 'multiple_choice', 'fill_blank', 'open_response')
    ),
    is_builtin INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
    is_enabled INTEGER NOT NULL DEFAULT 1 CHECK (is_enabled IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0,
    default_options_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(default_options_json)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;

CREATE TABLE question_type_aliases (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    question_type_code TEXT NOT NULL,
    alias TEXT NOT NULL CHECK (trim(alias) <> '' AND length(alias) <= 40),
    alias_key TEXT NOT NULL UNIQUE CHECK (trim(alias_key) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CONSTRAINT fk_question_type_aliases_type
        FOREIGN KEY (question_type_code) REFERENCES question_types(code) ON DELETE CASCADE
) STRICT;

CREATE INDEX idx_question_types_order
    ON question_types(is_enabled DESC, sort_order, code);

CREATE INDEX idx_question_type_aliases_type
    ON question_type_aliases(question_type_code, alias_key);

INSERT INTO question_types (
    code, name, name_key, behavior, is_builtin, is_enabled, sort_order,
    default_options_json, created_at_ms, updated_at_ms
) VALUES
    ('single_choice',   '单选题',     '单选题',     'single_choice',   1, 1, 10, '[]', 0, 0),
    ('multiple_choice', '多选题',     '多选题',     'multiple_choice', 1, 1, 20, '[]', 0, 0),
    ('fill_blank',      '填空题',     '填空题',     'fill_blank',      1, 1, 30, '[]', 0, 0),
    ('short_answer',    '简答题',     '简答题',     'open_response',   1, 1, 40, '[]', 0, 0),
    ('application',     '综合应用题', '综合应用题', 'open_response',   1, 1, 50, '[]', 0, 0),
    ('case_analysis',   '案例分析题', '案例分析题', 'open_response',   1, 1, 60, '[]', 0, 0);

INSERT INTO question_type_aliases (
    id, question_type_code, alias, alias_key, created_at_ms
) VALUES
    ('00000000-0000-7000-8000-000000000001', 'single_choice',   '单项选择题', '单项选择题', 0),
    ('00000000-0000-7000-8000-000000000002', 'single_choice',   '选择题',     '选择题',     0),
    ('00000000-0000-7000-8000-000000000003', 'multiple_choice', '多项选择题', '多项选择题', 0),
    ('00000000-0000-7000-8000-000000000004', 'fill_blank',      '填空',       '填空',       0),
    ('00000000-0000-7000-8000-000000000005', 'short_answer',    '问答题',     '问答题',     0),
    ('00000000-0000-7000-8000-000000000006', 'short_answer',    '解答题',     '解答题',     0),
    ('00000000-0000-7000-8000-000000000007', 'application',     '综合题',     '综合题',     0),
    ('00000000-0000-7000-8000-000000000008', 'case_analysis',   '案例题',     '案例题',     0);

-- Rebuild the two tables whose original CHECK constraints only allowed six codes.
CREATE TABLE questions_dynamic (
    local_id INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE CHECK (length(id) = 36),
    question_type TEXT NOT NULL,
    subject_id TEXT NOT NULL CHECK (length(subject_id) = 36),
    chapter_id TEXT NOT NULL CHECK (length(chapter_id) = 36),
    content_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (content_schema_version >= 1),
    stem_json TEXT NOT NULL CHECK (json_valid(stem_json)),
    answer_json TEXT NOT NULL CHECK (json_valid(answer_json)),
    explanation_json TEXT NOT NULL CHECK (json_valid(explanation_json)),
    stem_plain TEXT NOT NULL,
    options_plain TEXT NOT NULL DEFAULT '',
    answer_plain TEXT NOT NULL,
    explanation_plain TEXT NOT NULL DEFAULT '',
    tags_plain TEXT NOT NULL DEFAULT '',
    fingerprint_version INTEGER NOT NULL DEFAULT 1 CHECK (fingerprint_version >= 1),
    exact_fingerprint BLOB NOT NULL CHECK (length(exact_fingerprint) = 32),
    content_version INTEGER NOT NULL DEFAULT 1 CHECK (content_version >= 1),
    last_used_at_ms INTEGER CHECK (last_used_at_ms IS NULL OR last_used_at_ms >= 0),
    deleted_at_ms INTEGER CHECK (deleted_at_ms IS NULL OR deleted_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT fk_questions_type
        FOREIGN KEY (question_type) REFERENCES question_types(code) ON DELETE RESTRICT,
    CONSTRAINT fk_questions_subject
        FOREIGN KEY (subject_id) REFERENCES subjects(id) ON DELETE RESTRICT,
    CONSTRAINT fk_questions_chapter_subject
        FOREIGN KEY (chapter_id, subject_id)
        REFERENCES chapters(id, subject_id)
        ON DELETE RESTRICT
) STRICT;

INSERT INTO questions_dynamic SELECT * FROM questions;
DROP TABLE questions;
ALTER TABLE questions_dynamic RENAME TO questions;

CREATE INDEX idx_questions_active_updated
    ON questions(updated_at_ms DESC, id) WHERE deleted_at_ms IS NULL;
CREATE INDEX idx_questions_active_filter
    ON questions(subject_id, chapter_id, question_type, updated_at_ms DESC, id)
    WHERE deleted_at_ms IS NULL;
CREATE INDEX idx_questions_active_created
    ON questions(created_at_ms DESC, id) WHERE deleted_at_ms IS NULL;
CREATE INDEX idx_questions_active_last_used
    ON questions(last_used_at_ms, id) WHERE deleted_at_ms IS NULL;
CREATE INDEX idx_questions_recycle_bin
    ON questions(deleted_at_ms DESC, id) WHERE deleted_at_ms IS NOT NULL;
CREATE INDEX idx_questions_exact_fingerprint
    ON questions(fingerprint_version, exact_fingerprint) WHERE deleted_at_ms IS NULL;

CREATE TRIGGER question_fts_after_insert
AFTER INSERT ON questions
BEGIN
    INSERT INTO question_fts(rowid, stem_plain, options_plain, answer_plain, explanation_plain, tags_plain)
    VALUES (new.local_id, new.stem_plain, new.options_plain, new.answer_plain, new.explanation_plain, new.tags_plain);
END;

CREATE TRIGGER question_fts_after_delete
AFTER DELETE ON questions
BEGIN
    INSERT INTO question_fts(question_fts, rowid, stem_plain, options_plain, answer_plain, explanation_plain, tags_plain)
    VALUES ('delete', old.local_id, old.stem_plain, old.options_plain, old.answer_plain, old.explanation_plain, old.tags_plain);
END;

CREATE TRIGGER question_fts_after_search_update
AFTER UPDATE OF stem_plain, options_plain, answer_plain, explanation_plain, tags_plain ON questions
BEGIN
    INSERT INTO question_fts(question_fts, rowid, stem_plain, options_plain, answer_plain, explanation_plain, tags_plain)
    VALUES ('delete', old.local_id, old.stem_plain, old.options_plain, old.answer_plain, old.explanation_plain, old.tags_plain);
    INSERT INTO question_fts(rowid, stem_plain, options_plain, answer_plain, explanation_plain, tags_plain)
    VALUES (new.local_id, new.stem_plain, new.options_plain, new.answer_plain, new.explanation_plain, new.tags_plain);
END;

CREATE TABLE paper_items_dynamic (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    paper_id TEXT NOT NULL CHECK (length(paper_id) = 36),
    source_question_id TEXT CHECK (source_question_id IS NULL OR length(source_question_id) = 36),
    position INTEGER NOT NULL CHECK (position >= 0),
    question_type TEXT NOT NULL,
    subject_id_snapshot TEXT NOT NULL CHECK (length(subject_id_snapshot) = 36),
    chapter_id_snapshot TEXT NOT NULL CHECK (length(chapter_id_snapshot) = 36),
    tag_ids_snapshot_json TEXT NOT NULL CHECK (json_valid(tag_ids_snapshot_json)),
    snapshot_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (snapshot_schema_version >= 1),
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    source_content_version INTEGER CHECK (source_content_version IS NULL OR source_content_version >= 1),
    source_exact_fingerprint BLOB CHECK (source_exact_fingerprint IS NULL OR length(source_exact_fingerprint) = 32),
    usage_recorded_at_ms INTEGER CHECK (usage_recorded_at_ms IS NULL OR usage_recorded_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT fk_paper_items_type
        FOREIGN KEY (question_type) REFERENCES question_types(code) ON DELETE RESTRICT,
    CONSTRAINT fk_paper_items_paper
        FOREIGN KEY (paper_id) REFERENCES papers(id) ON DELETE CASCADE,
    CONSTRAINT fk_paper_items_source_question
        FOREIGN KEY (source_question_id) REFERENCES questions(id) ON DELETE SET NULL,
    CONSTRAINT uq_paper_items_position UNIQUE (paper_id, position)
) STRICT;

INSERT INTO paper_items_dynamic SELECT * FROM paper_items;
DROP TABLE paper_items;
ALTER TABLE paper_items_dynamic RENAME TO paper_items;

CREATE UNIQUE INDEX uq_paper_items_source_question
    ON paper_items(paper_id, source_question_id) WHERE source_question_id IS NOT NULL;

-- Question content. Rich JSON is canonical; *_plain columns are rebuildable search caches.
-- Intentionally absent from this schema: grade, difficulty, score and usage counters.

CREATE TABLE questions (
    local_id INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE CHECK (length(id) = 36),
    question_type TEXT NOT NULL CHECK (
        question_type IN (
            'single_choice',
            'multiple_choice',
            'fill_blank',
            'short_answer',
            'application',
            'case_analysis'
        )
    ),
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
    CONSTRAINT fk_questions_subject
        FOREIGN KEY (subject_id) REFERENCES subjects(id) ON DELETE RESTRICT,
    CONSTRAINT fk_questions_chapter_subject
        FOREIGN KEY (chapter_id, subject_id)
        REFERENCES chapters(id, subject_id)
        ON DELETE RESTRICT
) STRICT;

CREATE TABLE question_options (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    question_id TEXT NOT NULL CHECK (length(question_id) = 36),
    position INTEGER NOT NULL CHECK (position >= 0),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    plain_text TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT fk_question_options_question
        FOREIGN KEY (question_id) REFERENCES questions(id) ON DELETE CASCADE,
    CONSTRAINT uq_question_options_position UNIQUE (question_id, position),
    CONSTRAINT uq_question_options_id_question UNIQUE (id, question_id)
) STRICT;

CREATE TABLE question_tags (
    question_id TEXT NOT NULL CHECK (length(question_id) = 36),
    tag_id TEXT NOT NULL CHECK (length(tag_id) = 36),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    PRIMARY KEY (question_id, tag_id),
    CONSTRAINT fk_question_tags_question
        FOREIGN KEY (question_id) REFERENCES questions(id) ON DELETE CASCADE,
    CONSTRAINT fk_question_tags_tag
        FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
) STRICT;

CREATE INDEX idx_questions_active_updated
    ON questions(updated_at_ms DESC, id)
    WHERE deleted_at_ms IS NULL;

CREATE INDEX idx_questions_active_filter
    ON questions(subject_id, chapter_id, question_type, updated_at_ms DESC, id)
    WHERE deleted_at_ms IS NULL;

CREATE INDEX idx_questions_active_created
    ON questions(created_at_ms DESC, id)
    WHERE deleted_at_ms IS NULL;

CREATE INDEX idx_questions_active_last_used
    ON questions(last_used_at_ms, id)
    WHERE deleted_at_ms IS NULL;

CREATE INDEX idx_questions_recycle_bin
    ON questions(deleted_at_ms DESC, id)
    WHERE deleted_at_ms IS NOT NULL;

-- This is deliberately not UNIQUE: "keep both" is a supported duplicate action.
CREATE INDEX idx_questions_exact_fingerprint
    ON questions(fingerprint_version, exact_fingerprint)
    WHERE deleted_at_ms IS NULL;

CREATE INDEX idx_question_tags_reverse
    ON question_tags(tag_id, question_id);


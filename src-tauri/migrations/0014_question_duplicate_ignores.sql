-- Pairs explicitly confirmed by the teacher as not duplicated. The content
-- versions make an old decision expire automatically after either question is edited.

CREATE TABLE question_duplicate_ignores (
    question_id_low TEXT NOT NULL,
    question_id_high TEXT NOT NULL,
    content_version_low INTEGER NOT NULL CHECK (content_version_low >= 1),
    content_version_high INTEGER NOT NULL CHECK (content_version_high >= 1),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    PRIMARY KEY (question_id_low, question_id_high),
    CHECK (question_id_low < question_id_high),
    FOREIGN KEY (question_id_low) REFERENCES questions(id) ON DELETE CASCADE,
    FOREIGN KEY (question_id_high) REFERENCES questions(id) ON DELETE CASCADE
) STRICT;


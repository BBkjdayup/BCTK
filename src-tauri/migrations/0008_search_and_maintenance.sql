-- Chinese-friendly substring search. local_id is a stable local FTS rowid only.

CREATE VIRTUAL TABLE question_fts USING fts5(
    stem_plain,
    options_plain,
    answer_plain,
    explanation_plain,
    tags_plain,
    content = 'questions',
    content_rowid = 'local_id',
    tokenize = 'trigram'
);

CREATE TRIGGER question_fts_after_insert
AFTER INSERT ON questions
BEGIN
    INSERT INTO question_fts(
        rowid,
        stem_plain,
        options_plain,
        answer_plain,
        explanation_plain,
        tags_plain
    ) VALUES (
        new.local_id,
        new.stem_plain,
        new.options_plain,
        new.answer_plain,
        new.explanation_plain,
        new.tags_plain
    );
END;

CREATE TRIGGER question_fts_after_delete
AFTER DELETE ON questions
BEGIN
    INSERT INTO question_fts(
        question_fts,
        rowid,
        stem_plain,
        options_plain,
        answer_plain,
        explanation_plain,
        tags_plain
    ) VALUES (
        'delete',
        old.local_id,
        old.stem_plain,
        old.options_plain,
        old.answer_plain,
        old.explanation_plain,
        old.tags_plain
    );
END;

CREATE TRIGGER question_fts_after_search_update
AFTER UPDATE OF
    stem_plain,
    options_plain,
    answer_plain,
    explanation_plain,
    tags_plain
ON questions
BEGIN
    INSERT INTO question_fts(
        question_fts,
        rowid,
        stem_plain,
        options_plain,
        answer_plain,
        explanation_plain,
        tags_plain
    ) VALUES (
        'delete',
        old.local_id,
        old.stem_plain,
        old.options_plain,
        old.answer_plain,
        old.explanation_plain,
        old.tags_plain
    );

    INSERT INTO question_fts(
        rowid,
        stem_plain,
        options_plain,
        answer_plain,
        explanation_plain,
        tags_plain
    ) VALUES (
        new.local_id,
        new.stem_plain,
        new.options_plain,
        new.answer_plain,
        new.explanation_plain,
        new.tags_plain
    );
END;

-- Every resource-owning table participates in garbage-collection reachability.
CREATE VIEW resource_all_refs AS
    SELECT
        'question' AS owner_kind,
        id AS reference_id,
        question_id AS owner_id,
        resource_id
    FROM question_resource_refs
    UNION ALL
    SELECT
        'paper_item' AS owner_kind,
        id AS reference_id,
        paper_item_id AS owner_id,
        resource_id
    FROM paper_item_resource_refs
    UNION ALL
    SELECT
        'draft' AS owner_kind,
        id AS reference_id,
        draft_id AS owner_id,
        resource_id
    FROM draft_resource_refs
    UNION ALL
    SELECT
        'word_import_item' AS owner_kind,
        id AS reference_id,
        import_item_id AS owner_id,
        resource_id
    FROM word_import_item_resource_refs;

CREATE VIEW resource_reference_counts AS
    SELECT resource_id, COUNT(*) AS reference_count
    FROM resource_all_refs
    GROUP BY resource_id;

-- The table is empty on a fresh installation, and this also makes the migration
-- correct when it is added to an already populated pre-FTS database.
INSERT INTO question_fts(question_fts) VALUES ('rebuild');


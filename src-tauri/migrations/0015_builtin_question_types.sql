-- Keep the built-in catalogue focused on the five classroom question types.
-- Historical application/case questions are open responses, so they can be
-- merged into short answers without changing their stem, answer or resources.

PRAGMA defer_foreign_keys = ON;

-- Make room for the stable built-in judgment name if an older installation
-- happened to create a custom type with the same display name.
UPDATE question_types
SET name = name || '（原自定义）',
    name_key = name_key || '原自定义',
    updated_at_ms = updated_at_ms
WHERE is_builtin = 0 AND name_key IN ('判断题', '判断', '正误题', '是非题');

DELETE FROM question_type_aliases
WHERE alias_key IN ('判断题', '判断', '正误题', '是非题');

INSERT INTO question_types (
    code, name, name_key, behavior, is_builtin, is_enabled, sort_order,
    default_options_json, created_at_ms, updated_at_ms
) VALUES (
    'true_false', '判断题', '判断题', 'single_choice', 1, 1, 40,
    '["正确","错误"]', 0, 0
);

INSERT INTO question_type_aliases (
    id, question_type_code, alias, alias_key, created_at_ms
) VALUES
    ('00000000-0000-7000-8000-000000000009', 'true_false', '判断',   '判断',   0),
    ('00000000-0000-7000-8000-000000000010', 'true_false', '正误题', '正误题', 0),
    ('00000000-0000-7000-8000-000000000011', 'true_false', '是非题', '是非题', 0);

-- Keep old Word headings compatible: these names now resolve to short answer.
DELETE FROM question_type_aliases
WHERE question_type_code IN ('application', 'case_analysis');

INSERT INTO question_type_aliases (
    id, question_type_code, alias, alias_key, created_at_ms
) VALUES
    ('00000000-0000-7000-8000-000000000012', 'short_answer', '综合应用题', '综合应用题', 0),
    ('00000000-0000-7000-8000-000000000013', 'short_answer', '综合题',     '综合题',     0),
    ('00000000-0000-7000-8000-000000000014', 'short_answer', '应用题',     '应用题',     0),
    ('00000000-0000-7000-8000-000000000015', 'short_answer', '案例分析题', '案例分析题', 0),
    ('00000000-0000-7000-8000-000000000016', 'short_answer', '案例题',     '案例题',     0),
    ('00000000-0000-7000-8000-000000000017', 'short_answer', '案例分析',   '案例分析',   0);

UPDATE questions
SET question_type = 'short_answer'
WHERE question_type IN ('application', 'case_analysis');

UPDATE paper_items
SET question_type = 'short_answer',
    snapshot_json = json_set(snapshot_json, '$.type', 'short_answer')
WHERE question_type IN ('application', 'case_analysis');

-- Recoverable drafts should reopen with a valid type after the migration.
UPDATE drafts
SET payload_json = replace(
        replace(payload_json, '"type":"application"', '"type":"short_answer"'),
        '"type":"case_analysis"', '"type":"short_answer"'
    )
WHERE payload_json LIKE '%"type":"application"%'
   OR payload_json LIKE '%"type":"case_analysis"%';

UPDATE word_import_items
SET payload_json = replace(
        replace(payload_json, '"type":"application"', '"type":"short_answer"'),
        '"type":"case_analysis"', '"type":"short_answer"'
    )
WHERE payload_json LIKE '%"type":"application"%'
   OR payload_json LIKE '%"type":"case_analysis"%';

UPDATE document_question_drafts
SET payload_json = replace(
        replace(payload_json, '"type":"application"', '"type":"short_answer"'),
        '"type":"case_analysis"', '"type":"short_answer"'
    )
WHERE payload_json LIKE '%"type":"application"%'
   OR payload_json LIKE '%"type":"case_analysis"%';

-- Automatic paper settings use type codes as JSON object keys. Preserve the
-- requested total by merging retired counts into short_answer.
UPDATE papers
SET generation_config_json = json_remove(
        json_set(
            generation_config_json,
            '$.questionTypeCounts.short_answer',
            COALESCE(json_extract(generation_config_json, '$.questionTypeCounts.short_answer'), 0)
              + COALESCE(json_extract(generation_config_json, '$.questionTypeCounts.application'), 0)
              + COALESCE(json_extract(generation_config_json, '$.questionTypeCounts.case_analysis'), 0)
        ),
        '$.questionTypeCounts.application',
        '$.questionTypeCounts.case_analysis'
    )
WHERE generation_config_json IS NOT NULL
  AND (
      json_type(generation_config_json, '$.questionTypeCounts.application') IS NOT NULL
      OR json_type(generation_config_json, '$.questionTypeCounts.case_analysis') IS NOT NULL
  );

DELETE FROM question_types
WHERE code IN ('application', 'case_analysis');

UPDATE question_types SET sort_order = 10 WHERE code = 'single_choice';
UPDATE question_types SET sort_order = 20 WHERE code = 'multiple_choice';
UPDATE question_types SET sort_order = 30 WHERE code = 'fill_blank';
UPDATE question_types SET sort_order = 40 WHERE code = 'true_false';
UPDATE question_types SET sort_order = 50 WHERE code = 'short_answer';

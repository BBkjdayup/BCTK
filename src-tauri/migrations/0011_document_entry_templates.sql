-- Named, user-configurable recognition profiles for free-document question entry.

CREATE TABLE document_entry_templates (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL COLLATE NOCASE CHECK (length(trim(name)) BETWEEN 1 AND 40),
    config_json TEXT NOT NULL CHECK (json_valid(config_json)),
    is_builtin INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    UNIQUE (name)
) STRICT;

CREATE UNIQUE INDEX idx_document_entry_templates_single_default
    ON document_entry_templates(is_default)
    WHERE is_default = 1;

INSERT INTO document_entry_templates (
    id,
    name,
    config_json,
    is_builtin,
    is_default,
    created_at_ms,
    updated_at_ms
) VALUES (
    'labeled_fields_v1',
    '默认字段模板',
    json_object(
        'schemaVersion', 1,
        'typeMarker', '题型：',
        'stemMarker', '题目：',
        'answerMarker', '答案：',
        'explanationMarker', '解析：',
        'optionStyle', 'letter_dot',
        'questionSeparator', '---',
        'compatibleDefaultMarkers', json('true'),
        'stemPrompt', '这里输入题目的题干',
        'optionPrompt', '这里输入选项',
        'answerPrompt', '这里输入参考答案',
        'explanationPrompt', '这里输入解析，可以不填'
    ),
    1,
    1,
    0,
    0
);

-- Enable numbered question starts for existing document-entry templates.

UPDATE document_entry_templates
SET config_json = json_set(
    config_json,
    '$.recognizeQuestionNumbers', json('true'),
    '$.stripRecognizedQuestionNumbers', json('true')
)
WHERE json_extract(config_json, '$.recognizeQuestionNumbers') IS NULL
   OR json_extract(config_json, '$.stripRecognizedQuestionNumbers') IS NULL;

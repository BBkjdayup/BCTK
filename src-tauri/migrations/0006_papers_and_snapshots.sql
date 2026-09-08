-- Draft/saved papers. Every paper item owns a complete content snapshot.

CREATE TABLE papers (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    title TEXT NOT NULL DEFAULT '未命名试卷' CHECK (trim(title) <> ''),
    composition_mode TEXT NOT NULL CHECK (composition_mode IN ('manual', 'automatic')),
    paper_status TEXT NOT NULL CHECK (paper_status IN ('draft', 'saved')),
    generation_config_json TEXT CHECK (
        generation_config_json IS NULL OR json_valid(generation_config_json)
    ),
    subject_summary_text TEXT NOT NULL DEFAULT '',
    preferred_template_id TEXT CHECK (
        preferred_template_id IS NULL OR length(preferred_template_id) = 36
    ),
    export_content_mode TEXT NOT NULL DEFAULT 'paper_only' CHECK (
        export_content_mode IN (
            'paper_only',
            'answers_only',
            'paper_and_answers',
            'paper_answers_explanations'
        )
    ),
    proposed_export_filename TEXT,
    proposed_export_directory TEXT,
    row_version INTEGER NOT NULL DEFAULT 1 CHECK (row_version >= 1),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    saved_at_ms INTEGER CHECK (saved_at_ms IS NULL OR saved_at_ms >= 0),
    last_saved_at_ms INTEGER CHECK (last_saved_at_ms IS NULL OR last_saved_at_ms >= 0),
    CONSTRAINT ck_papers_saved_timestamp CHECK (
        paper_status = 'draft' OR saved_at_ms IS NOT NULL
    ),
    CONSTRAINT fk_papers_preferred_template
        FOREIGN KEY (preferred_template_id) REFERENCES word_templates(id) ON DELETE SET NULL
) STRICT;

CREATE TABLE paper_items (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    paper_id TEXT NOT NULL CHECK (length(paper_id) = 36),
    source_question_id TEXT CHECK (source_question_id IS NULL OR length(source_question_id) = 36),
    position INTEGER NOT NULL CHECK (position >= 0),
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
    subject_id_snapshot TEXT NOT NULL CHECK (length(subject_id_snapshot) = 36),
    chapter_id_snapshot TEXT NOT NULL CHECK (length(chapter_id_snapshot) = 36),
    tag_ids_snapshot_json TEXT NOT NULL CHECK (json_valid(tag_ids_snapshot_json)),
    snapshot_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (snapshot_schema_version >= 1),
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    source_content_version INTEGER CHECK (source_content_version IS NULL OR source_content_version >= 1),
    source_exact_fingerprint BLOB CHECK (
        source_exact_fingerprint IS NULL OR length(source_exact_fingerprint) = 32
    ),
    usage_recorded_at_ms INTEGER CHECK (usage_recorded_at_ms IS NULL OR usage_recorded_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CONSTRAINT fk_paper_items_paper
        FOREIGN KEY (paper_id) REFERENCES papers(id) ON DELETE CASCADE,
    CONSTRAINT fk_paper_items_source_question
        FOREIGN KEY (source_question_id) REFERENCES questions(id) ON DELETE SET NULL,
    CONSTRAINT uq_paper_items_position UNIQUE (paper_id, position)
) STRICT;

CREATE UNIQUE INDEX uq_paper_items_source_question
    ON paper_items(paper_id, source_question_id)
    WHERE source_question_id IS NOT NULL;

CREATE TABLE paper_item_resource_refs (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    paper_item_id TEXT NOT NULL CHECK (length(paper_item_id) = 36),
    resource_id TEXT NOT NULL CHECK (length(resource_id) = 36),
    node_id TEXT NOT NULL CHECK (trim(node_id) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CONSTRAINT fk_paper_item_resource_item
        FOREIGN KEY (paper_item_id) REFERENCES paper_items(id) ON DELETE CASCADE,
    CONSTRAINT fk_paper_item_resource_resource
        FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE RESTRICT,
    CONSTRAINT uq_paper_item_resource_node UNIQUE (paper_item_id, node_id)
) STRICT;

CREATE TABLE paper_export_runs (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    paper_id TEXT NOT NULL CHECK (length(paper_id) = 36),
    template_id TEXT CHECK (template_id IS NULL OR length(template_id) = 36),
    template_name_snapshot TEXT,
    template_sha256_snapshot BLOB CHECK (
        template_sha256_snapshot IS NULL OR length(template_sha256_snapshot) = 32
    ),
    content_mode TEXT NOT NULL CHECK (
        content_mode IN (
            'paper_only',
            'answers_only',
            'paper_and_answers',
            'paper_answers_explanations'
        )
    ),
    output_path TEXT,
    output_filename TEXT,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'cancelled')),
    output_sha256 BLOB CHECK (output_sha256 IS NULL OR length(output_sha256) = 32),
    output_byte_size INTEGER CHECK (output_byte_size IS NULL OR output_byte_size >= 0),
    error_code TEXT,
    error_message TEXT,
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    finished_at_ms INTEGER CHECK (finished_at_ms IS NULL OR finished_at_ms >= 0),
    CONSTRAINT fk_paper_export_runs_paper
        FOREIGN KEY (paper_id) REFERENCES papers(id) ON DELETE CASCADE,
    CONSTRAINT fk_paper_export_runs_template
        FOREIGN KEY (template_id) REFERENCES word_templates(id) ON DELETE SET NULL
) STRICT;

CREATE INDEX idx_papers_history
    ON papers(paper_status, saved_at_ms DESC, id);

CREATE INDEX idx_paper_items_order
    ON paper_items(paper_id, position, id);

CREATE INDEX idx_paper_item_resources_resource
    ON paper_item_resource_refs(resource_id, paper_item_id);

CREATE INDEX idx_paper_export_runs_paper_time
    ON paper_export_runs(paper_id, started_at_ms DESC);


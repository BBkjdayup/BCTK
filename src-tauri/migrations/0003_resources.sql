-- Immutable, content-addressed resources and strong question ownership references.

CREATE TABLE resources (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    sha256 BLOB NOT NULL UNIQUE CHECK (length(sha256) = 32),
    resource_kind TEXT NOT NULL CHECK (resource_kind IN ('image', 'formula', 'attachment')),
    mime_type TEXT NOT NULL CHECK (trim(mime_type) <> ''),
    storage_rel_path TEXT NOT NULL UNIQUE CHECK (trim(storage_rel_path) <> ''),
    original_filename TEXT,
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    intrinsic_width_px INTEGER CHECK (intrinsic_width_px IS NULL OR intrinsic_width_px > 0),
    intrinsic_height_px INTEGER CHECK (intrinsic_height_px IS NULL OR intrinsic_height_px > 0),
    availability_status TEXT NOT NULL DEFAULT 'ready'
        CHECK (availability_status IN ('ready', 'missing', 'quarantined')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    last_verified_at_ms INTEGER CHECK (last_verified_at_ms IS NULL OR last_verified_at_ms >= 0)
) STRICT;

CREATE TABLE question_resource_refs (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    question_id TEXT NOT NULL CHECK (length(question_id) = 36),
    option_id TEXT CHECK (option_id IS NULL OR length(option_id) = 36),
    resource_id TEXT NOT NULL CHECK (length(resource_id) = 36),
    content_slot TEXT NOT NULL CHECK (content_slot IN ('stem', 'option', 'answer', 'explanation')),
    node_id TEXT NOT NULL CHECK (trim(node_id) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CONSTRAINT ck_question_resource_option_slot CHECK (
        (content_slot = 'option' AND option_id IS NOT NULL)
        OR (content_slot <> 'option' AND option_id IS NULL)
    ),
    CONSTRAINT fk_question_resource_question
        FOREIGN KEY (question_id) REFERENCES questions(id) ON DELETE CASCADE,
    CONSTRAINT fk_question_resource_option
        FOREIGN KEY (option_id, question_id)
        REFERENCES question_options(id, question_id)
        ON DELETE CASCADE,
    CONSTRAINT fk_question_resource_resource
        FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE RESTRICT,
    CONSTRAINT uq_question_resource_node UNIQUE (question_id, node_id)
) STRICT;

CREATE INDEX idx_resources_availability
    ON resources(availability_status, created_at_ms);

CREATE INDEX idx_question_resource_refs_resource
    ON question_resource_refs(resource_id, question_id);


-- Optional native canvas-editor layout. Existing papers remain valid with NULL layout.

ALTER TABLE papers ADD COLUMN layout_json TEXT CHECK (
    layout_json IS NULL OR json_valid(layout_json)
);

ALTER TABLE papers ADD COLUMN layout_updated_at_ms INTEGER CHECK (
    layout_updated_at_ms IS NULL OR layout_updated_at_ms >= 0
);

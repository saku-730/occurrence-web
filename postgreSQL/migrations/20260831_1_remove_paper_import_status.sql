-- +goose Up
-- +goose StatementBegin

-- paper_imports is now a reusable source record for PDFs that have not yet been
-- promoted to papers. Import/extraction workflow state is no longer persisted.
DROP INDEX IF EXISTS idx_paper_imports_status;

ALTER TABLE paper_imports
    DROP CONSTRAINT IF EXISTS chk_paper_imports_status;

ALTER TABLE paper_imports
    DROP COLUMN status;

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

-- Restore the legacy workflow column for rollback. Existing source rows are
-- assigned staged because that was the neutral reusable state in the old flow.
ALTER TABLE paper_imports
    ADD COLUMN status TEXT NOT NULL DEFAULT 'staged';

ALTER TABLE paper_imports
    ALTER COLUMN status DROP DEFAULT;

ALTER TABLE paper_imports
    ADD CONSTRAINT chk_paper_imports_status CHECK (
        status IN (
            'metadata_required',
            'staged',
            'extracting_occurrences',
            'reviewing',
            'cancelling',
            'failed'
        )
    );

CREATE INDEX idx_paper_imports_status ON paper_imports(status);

-- +goose StatementEnd

-- +goose Up
-- +goose StatementBegin

-- papers becomes the only PostgreSQL table for paper PDFs.
-- Existing papers rows were formal paper records in the previous design, so
-- preserve that meaning by marking them registered.
ALTER TABLE papers
    ADD COLUMN status TEXT;

UPDATE papers
SET status = 'registered';

ALTER TABLE papers
    ALTER COLUMN status SET NOT NULL;

ALTER TABLE papers
    ADD CONSTRAINT chk_papers_status CHECK (
        status IN ('unregistered', 'registered')
    );

-- Preserve PDFs that were still in the old paper_imports table. They have not
-- completed occurrence registration yet, so migrate them as unregistered.
INSERT INTO papers (
    id,
    bucket,
    object_key,
    content_type,
    size_bytes,
    original_filename,
    sha256,
    doi,
    title,
    authors,
    publication_year,
    journal,
    volume,
    issue,
    pages,
    article_number,
    uploaded_by,
    created_at,
    status
)
SELECT
    reserved_paper_id,
    bucket,
    object_key,
    content_type,
    size_bytes,
    original_filename,
    sha256,
    doi,
    title,
    authors,
    publication_year,
    journal,
    volume,
    issue,
    pages,
    article_number,
    uploaded_by,
    created_at,
    'unregistered'
FROM paper_imports
ON CONFLICT DO NOTHING;

DROP TABLE paper_imports;

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

-- Recreate the schema that existed after 20260831_1. Rollback cannot restore
-- the original paper_imports UUIDs, so the paper UUID is reused for both id and
-- reserved_paper_id while preserving the PDF/object metadata.
CREATE TABLE paper_imports (
    id UUID PRIMARY KEY,
    reserved_paper_id UUID NOT NULL UNIQUE,
    bucket TEXT NOT NULL,
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    original_filename TEXT,
    sha256 TEXT NOT NULL,
    doi TEXT,
    title TEXT,
    authors TEXT,
    publication_year INTEGER,
    journal TEXT,
    volume TEXT,
    issue TEXT,
    pages TEXT,
    article_number TEXT,
    uploaded_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_paper_imports_content_type CHECK (content_type = 'application/pdf'),
    CONSTRAINT chk_paper_imports_size_bytes CHECK (size_bytes > 0),
    CONSTRAINT chk_paper_imports_sha256_format CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_paper_imports_publication_year CHECK (
        publication_year IS NULL OR publication_year BETWEEN 1000 AND 3000
    )
);

CREATE INDEX idx_paper_imports_uploaded_by ON paper_imports(uploaded_by);
CREATE INDEX idx_paper_imports_sha256 ON paper_imports(sha256);

INSERT INTO paper_imports (
    id,
    reserved_paper_id,
    bucket,
    object_key,
    content_type,
    size_bytes,
    original_filename,
    sha256,
    doi,
    title,
    authors,
    publication_year,
    journal,
    volume,
    issue,
    pages,
    article_number,
    uploaded_by,
    created_at,
    updated_at
)
SELECT
    id,
    id,
    bucket,
    object_key,
    content_type,
    size_bytes,
    original_filename,
    sha256,
    doi,
    title,
    authors,
    publication_year,
    journal,
    volume,
    issue,
    pages,
    article_number,
    uploaded_by,
    created_at,
    created_at
FROM papers
WHERE status = 'unregistered';

DELETE FROM papers
WHERE status = 'unregistered';

ALTER TABLE papers
    DROP CONSTRAINT chk_papers_status;

ALTER TABLE papers
    DROP COLUMN status;

-- +goose StatementEnd

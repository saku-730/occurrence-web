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
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_paper_imports_content_type CHECK (content_type = 'application/pdf'),
    CONSTRAINT chk_paper_imports_size_bytes CHECK (size_bytes > 0),
    CONSTRAINT chk_paper_imports_sha256_format CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_paper_imports_publication_year CHECK (
        publication_year IS NULL OR publication_year BETWEEN 1000 AND 3000
    ),
    CONSTRAINT chk_paper_imports_status CHECK (
        status IN (
            'metadata_required',
            'staged',
            'extracting_occurrences',
            'reviewing',
            'cancelling',
            'failed'
        )
    )
);

CREATE INDEX idx_paper_imports_uploaded_by ON paper_imports(uploaded_by);
CREATE INDEX idx_paper_imports_sha256 ON paper_imports(sha256);
CREATE INDEX idx_paper_imports_status ON paper_imports(status);

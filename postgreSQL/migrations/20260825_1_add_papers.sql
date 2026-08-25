-- +goose Up
-- +goose StatementBegin

CREATE TABLE papers (
    id UUID PRIMARY KEY,
    bucket TEXT NOT NULL,
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    original_filename TEXT,
    -- 同一PDFはユーザーをまたいで1件だけ保持する。
    -- backendが受信時に計算したlowercase SHA-256を重複判定に使用する。
    sha256 TEXT NOT NULL UNIQUE,
    uploaded_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_papers_content_type CHECK (content_type = 'application/pdf'),
    CONSTRAINT chk_papers_size_bytes CHECK (size_bytes > 0),
    CONSTRAINT chk_papers_sha256_format CHECK (sha256 ~ '^[0-9a-f]{64}$')
);

CREATE INDEX idx_papers_uploaded_by
ON papers(uploaded_by);

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

DROP TABLE IF EXISTS papers;

-- +goose StatementEnd

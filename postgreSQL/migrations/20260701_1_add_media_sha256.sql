-- +goose Up
-- +goose StatementBegin

ALTER TABLE media_objects
ADD COLUMN sha256 TEXT;

-- 既存objectはGarageからhashを再計算するまでNULLを許可する。
-- 新規uploadはbackendが必ずSHA-256を保存し、同一ユーザー内だけ物理objectを再利用する。
CREATE UNIQUE INDEX idx_media_objects_uploaded_by_sha256
ON media_objects(uploaded_by, sha256)
WHERE sha256 IS NOT NULL;

-- SHA-256はlowercase hexadecimal 64文字で保存する。
ALTER TABLE media_objects
ADD CONSTRAINT chk_media_objects_sha256_format
CHECK (sha256 IS NULL OR sha256 ~ '^[0-9a-f]{64}$');

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

ALTER TABLE media_objects
DROP CONSTRAINT IF EXISTS chk_media_objects_sha256_format;

DROP INDEX IF EXISTS idx_media_objects_uploaded_by_sha256;

ALTER TABLE media_objects
DROP COLUMN IF EXISTS sha256;

-- +goose StatementEnd

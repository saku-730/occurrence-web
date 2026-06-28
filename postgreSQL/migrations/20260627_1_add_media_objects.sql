-- +goose Up
-- +goose StatementBegin

CREATE TABLE media_objects (
    -- RDFで参照するmedia URIとGarage object keyの両方に同じUUIDを使用する。
    id UUID PRIMARY KEY,
    bucket TEXT NOT NULL,
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    original_filename TEXT,
    -- 所有者による取得・削除認可に使用するため、登録ユーザーを必須とする。
    -- ON DELETEを指定せず、参照中のユーザーが意図せず削除されることを防ぐ。
    uploaded_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_media_objects_uploaded_by
ON media_objects(uploaded_by);

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

DROP TABLE IF EXISTS media_objects;

-- +goose StatementEnd

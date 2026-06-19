-- +goose Up
-- +goose StatementBegin

CREATE TABLE password_reset_tokens (
    -- MVPではユーザーごとに最新のリセットtokenだけを保持する。
    -- 再発行時はuser_id単位でUPSERTし、古いURLを自然に無効化する設計にする。
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_password_reset_tokens_expires_at
ON password_reset_tokens(expires_at);

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

DROP TABLE IF EXISTS password_reset_tokens;

-- +goose StatementEnd

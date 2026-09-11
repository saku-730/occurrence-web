-- +goose Up
-- +goose StatementBegin

CREATE TABLE label_templates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    template JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_label_templates_template_object
        CHECK (jsonb_typeof(template) = 'object')
);

CREATE INDEX idx_label_templates_user_id
ON label_templates(user_id);

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

DROP TABLE IF EXISTS label_templates;

-- +goose StatementEnd

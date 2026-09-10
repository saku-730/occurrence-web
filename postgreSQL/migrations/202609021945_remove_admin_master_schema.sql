-- +goose Up
-- +goose StatementBegin

-- Administrative reference data is no longer copied into the application
-- database. The backend reads the official Digital Agency ABR PostgreSQL
-- database directly through ABR_DATABASE_URL.
DROP SCHEMA IF EXISTS admin_master CASCADE;

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

-- Intentionally no-op. The copied admin_master schema was an abandoned design;
-- restoring it would also require restoring the removed importer and resolver.
SELECT 1;

-- +goose StatementEnd

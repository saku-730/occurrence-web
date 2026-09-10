-- +goose Up
-- +goose StatementBegin

-- ABR may contain both residential-address and non-residential-address rows
-- for the same local-government code + machiaza ID. Keep the flag as part of
-- the identity so the reference master can represent the official source.
ALTER TABLE admin_master.jp_machiaza
    DROP CONSTRAINT jp_machiaza_pkey;

ALTER TABLE admin_master.jp_machiaza
    ALTER COLUMN rsdt_addr_flg SET NOT NULL;

ALTER TABLE admin_master.jp_machiaza
    ADD PRIMARY KEY (lg_code, machiaza_id, rsdt_addr_flg);

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

-- A rollback can only restore the narrower key when there are no duplicate
-- lg_code + machiaza_id pairs. This is acceptable for migration rollback on a
-- freshly rebuilt reference master.
ALTER TABLE admin_master.jp_machiaza
    DROP CONSTRAINT jp_machiaza_pkey;

ALTER TABLE admin_master.jp_machiaza
    ADD PRIMARY KEY (lg_code, machiaza_id);

ALTER TABLE admin_master.jp_machiaza
    ALTER COLUMN rsdt_addr_flg DROP NOT NULL;

-- +goose StatementEnd

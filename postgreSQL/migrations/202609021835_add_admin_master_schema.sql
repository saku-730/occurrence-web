-- +goose Up
-- +goose StatementBegin

-- External administrative reference data is kept outside public so that
-- Bio-Database's own application tables and replaceable master datasets are
-- clearly separated.
CREATE SCHEMA IF NOT EXISTS admin_master;

-- One row describes the currently loaded dataset for a country. The master
-- tables themselves intentionally do not depend on a particular ABR release
-- layout; import code can replace a country's rows when a newer release is
-- loaded.
CREATE TABLE IF NOT EXISTS admin_master.datasets (
    country_code TEXT PRIMARY KEY,
    source_name TEXT NOT NULL,
    source_url TEXT,
    dataset_version TEXT,
    imported_at TIMESTAMPTZ,
    CONSTRAINT chk_admin_master_country_code
        CHECK (country_code ~ '^[A-Z]{2}$')
);

-- Japan: prefecture layer. pref_code is the two-digit prefecture code.
CREATE TABLE IF NOT EXISTS admin_master.jp_prefectures (
    pref_code TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    CONSTRAINT chk_jp_prefecture_code
        CHECK (pref_code ~ '^[0-9]{2}$'),
    CONSTRAINT chk_jp_prefecture_name
        CHECK (btrim(name) <> '')
);

-- Japan: municipality layer. lg_code stores the six-digit nationwide local
-- government code used by ABR. match_name is the canonical prefix used by the
-- resolver; examples are "大津市", "新宿区", and "横浜市中区".
CREATE TABLE IF NOT EXISTS admin_master.jp_municipalities (
    lg_code TEXT PRIMARY KEY,
    pref_code TEXT NOT NULL
        REFERENCES admin_master.jp_prefectures(pref_code)
        ON UPDATE CASCADE
        ON DELETE RESTRICT,
    match_name TEXT NOT NULL,
    county TEXT,
    city TEXT,
    ward TEXT,
    CONSTRAINT chk_jp_municipality_lg_code
        CHECK (lg_code ~ '^[0-9]{6}$'),
    CONSTRAINT chk_jp_municipality_match_name
        CHECK (btrim(match_name) <> ''),
    UNIQUE (pref_code, match_name)
);

CREATE INDEX IF NOT EXISTS idx_jp_municipalities_pref_code
    ON admin_master.jp_municipalities(pref_code);

-- Japan: machi-aza layer. machiaza_id is the ABR town/aza identifier within
-- the local government. match_name is the concatenated canonical prefix that
-- the resolver consumes from the remaining locality text. Source components
-- are retained so a later ABR importer does not have to discard structure.
CREATE TABLE IF NOT EXISTS admin_master.jp_machiaza (
    lg_code TEXT NOT NULL
        REFERENCES admin_master.jp_municipalities(lg_code)
        ON UPDATE CASCADE
        ON DELETE CASCADE,
    machiaza_id TEXT NOT NULL,
    match_name TEXT NOT NULL,
    oaza_cho TEXT,
    chome TEXT,
    koaza TEXT,
    rsdt_addr_flg SMALLINT,
    PRIMARY KEY (lg_code, machiaza_id),
    CONSTRAINT chk_jp_machiaza_id
        CHECK (machiaza_id ~ '^[0-9]{7}$'),
    CONSTRAINT chk_jp_machiaza_match_name
        CHECK (btrim(match_name) <> ''),
    CONSTRAINT chk_jp_machiaza_rsdt_addr_flg
        CHECK (rsdt_addr_flg IS NULL OR rsdt_addr_flg IN (0, 1))
);

CREATE INDEX IF NOT EXISTS idx_jp_machiaza_lg_code
    ON admin_master.jp_machiaza(lg_code);

-- A municipality can contain multiple source records that normalize to the
-- same textual prefix, so this is deliberately a non-unique lookup index.
CREATE INDEX IF NOT EXISTS idx_jp_machiaza_match_name
    ON admin_master.jp_machiaza(lg_code, match_name);

-- +goose StatementEnd

-- +goose Down
-- +goose StatementBegin

DROP SCHEMA IF EXISTS admin_master CASCADE;

-- +goose StatementEnd

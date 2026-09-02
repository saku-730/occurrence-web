use std::{collections::HashSet, env, error::Error, fmt};

use futures_util::TryStreamExt;
use sqlx::{
    PgPool, Postgres, QueryBuilder, Row, Transaction,
    postgres::PgPoolOptions,
};

const COUNTRY_CODE_JP: &str = "JP";
const SOURCE_NAME: &str = "Digital Agency Address Base Registry (ABR)";
const SOURCE_URL: &str = "https://www.digital.go.jp/policies/base_registry_address";
const BATCH_SIZE: usize = 1_000;

const PREF_COLUMNS: &[&str] = &["lg_code", "pref"];
const CITY_COLUMNS: &[&str] = &["lg_code", "county", "city", "ward"];
const TOWN_COLUMNS: &[&str] = &[
    "lg_code",
    "machiaza_id",
    "oaza_cho",
    "chome",
    "koaza",
    "rsdt_addr_flg",
    "koaza_aka_code",
];

type DynError = Box<dyn Error + Send + Sync>;

#[derive(Debug)]
struct ImportError(String);

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ImportError {}

#[derive(Debug)]
struct CliOptions {
    country: String,
    dataset_version: Option<String>,
}

#[derive(Debug)]
struct ImportCounts {
    prefectures: u64,
    municipalities: u64,
    machiaza: u64,
}

#[derive(Debug)]
struct PrefectureRow {
    pref_code: String,
    name: String,
}

#[derive(Debug)]
struct MunicipalityRow {
    lg_code: String,
    pref_code: String,
    match_name: String,
    county: Option<String>,
    city: String,
    ward: Option<String>,
}

#[derive(Debug)]
struct MachiazaRow {
    lg_code: String,
    machiaza_id: String,
    match_name: String,
    oaza_cho: Option<String>,
    chome: Option<String>,
    koaza: Option<String>,
    rsdt_addr_flg: i16,
}

#[tokio::main]
async fn main() -> Result<(), DynError> {
    dotenvy::dotenv().ok();
    let options = parse_args()?;
    if options.country != COUNTRY_CODE_JP {
        return Err(import_error(format!(
            "unsupported country '{}'; only JP is implemented",
            options.country
        )));
    }

    let abr_database_url = env::var("ABR_DATABASE_URL").map_err(|_| {
        import_error(
            "ABR_DATABASE_URL is required and must point to the PostgreSQL database populated by abrdb",
        )
    })?;
    let database_url = env::var("DATABASE_URL").map_err(|_| {
        import_error("DATABASE_URL is required and must point to the Bio-Database PostgreSQL database")
    })?;
    let dataset_version = options
        .dataset_version
        .or_else(|| env::var("ABR_DATASET_VERSION").ok())
        .and_then(nonempty_string);

    let abr = PgPoolOptions::new()
        .max_connections(2)
        .connect(&abr_database_url)
        .await?;
    let target = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;

    validate_source_schema(&abr).await?;
    validate_target_schema(&target).await?;

    println!("Importing JP administrative master from abrdb...");
    let counts = import_japan(&abr, &target, dataset_version.as_deref()).await?;
    println!(
        "Administrative master import completed: prefectures={}, municipalities={}, machiaza={}",
        counts.prefectures, counts.municipalities, counts.machiaza
    );

    Ok(())
}

fn parse_args() -> Result<CliOptions, DynError> {
    let mut country = COUNTRY_CODE_JP.to_string();
    let mut dataset_version = None;
    let mut args = env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--country" => {
                country = args
                    .next()
                    .ok_or_else(|| import_error("--country requires a value"))?
                    .trim()
                    .to_ascii_uppercase();
            }
            "--dataset-version" => {
                dataset_version = Some(
                    args.next()
                        .ok_or_else(|| import_error("--dataset-version requires a value"))?,
                );
            }
            "-h" | "--help" => {
                println!(
                    "Usage: import_admin_master [--country JP] [--dataset-version VERSION]\n\n\
                     Environment:\n\
                       ABR_DATABASE_URL   PostgreSQL URL populated by official abrdb\n\
                       DATABASE_URL       Bio-Database PostgreSQL URL\n\
                       ABR_DATASET_VERSION optional version label (CLI flag takes precedence)"
                );
                std::process::exit(0);
            }
            other => return Err(import_error(format!("unknown argument: {other}"))),
        }
    }

    Ok(CliOptions {
        country,
        dataset_version,
    })
}

async fn validate_source_schema(pool: &PgPool) -> Result<(), DynError> {
    validate_columns(pool, "mt_pref_unified", PREF_COLUMNS).await?;
    validate_columns(pool, "mt_city_unified", CITY_COLUMNS).await?;
    validate_columns(pool, "mt_town_unified", TOWN_COLUMNS).await?;
    Ok(())
}

async fn validate_columns(pool: &PgPool, table: &str, required: &[&str]) -> Result<(), DynError> {
    let columns: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT column_name
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = $1
        "#,
    )
    .bind(table)
    .fetch_all(pool)
    .await?;

    if columns.is_empty() {
        return Err(import_error(format!(
            "abrdb source table public.{table} was not found; initialize/import abrdb with --category basic"
        )));
    }

    let available: HashSet<&str> = columns.iter().map(String::as_str).collect();
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|column| !available.contains(column))
        .collect();
    if !missing.is_empty() {
        return Err(import_error(format!(
            "abrdb source table public.{table} is missing required column(s): {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

async fn validate_target_schema(pool: &PgPool) -> Result<(), DynError> {
    for table in ["datasets", "jp_prefectures", "jp_municipalities", "jp_machiaza"] {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.tables
                WHERE table_schema = 'admin_master' AND table_name = $1
            )
            "#,
        )
        .bind(table)
        .fetch_one(pool)
        .await?;
        if !exists {
            return Err(import_error(format!(
                "target table admin_master.{table} was not found; apply PostgreSQL migrations first"
            )));
        }
    }
    Ok(())
}

async fn import_japan(
    source: &PgPool,
    target: &PgPool,
    dataset_version: Option<&str>,
) -> Result<ImportCounts, DynError> {
    let mut transaction = target.begin().await?;
    create_staging_tables(&mut transaction).await?;

    let prefectures = stage_prefectures(source, &mut transaction).await?;
    let municipalities = stage_municipalities(source, &mut transaction).await?;
    let machiaza = stage_machiaza(source, &mut transaction).await?;

    validate_staged_data(&mut transaction, prefectures, municipalities, machiaza).await?;
    replace_master(&mut transaction, dataset_version).await?;
    transaction.commit().await?;

    Ok(ImportCounts {
        prefectures,
        municipalities,
        machiaza,
    })
}

async fn create_staging_tables(transaction: &mut Transaction<'_, Postgres>) -> Result<(), DynError> {
    sqlx::query(
        r#"
        CREATE TEMP TABLE jp_prefectures_stage (
            pref_code TEXT NOT NULL,
            name TEXT NOT NULL
        ) ON COMMIT DROP;

        CREATE TEMP TABLE jp_municipalities_stage (
            lg_code TEXT NOT NULL,
            pref_code TEXT NOT NULL,
            match_name TEXT NOT NULL,
            county TEXT,
            city TEXT NOT NULL,
            ward TEXT
        ) ON COMMIT DROP;

        CREATE TEMP TABLE jp_machiaza_stage (
            lg_code TEXT NOT NULL,
            machiaza_id TEXT NOT NULL,
            match_name TEXT NOT NULL,
            oaza_cho TEXT,
            chome TEXT,
            koaza TEXT,
            rsdt_addr_flg SMALLINT
        ) ON COMMIT DROP;
        "#,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn stage_prefectures(
    source: &PgPool,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<u64, DynError> {
    let mut rows = sqlx::query(
        "SELECT lg_code, pref FROM public.mt_pref_unified ORDER BY lg_code",
    )
    .fetch(source);
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut count = 0_u64;

    while let Some(row) = rows.try_next().await? {
        let lg_code: String = row.try_get("lg_code")?;
        let name: String = row.try_get("pref")?;
        let lg_code = validate_lg_code(&lg_code)?;
        let name = required_component(&name, "pref", &lg_code)?;
        batch.push(PrefectureRow {
            pref_code: lg_code[..2].to_string(),
            name,
        });
        count += 1;

        if batch.len() >= BATCH_SIZE {
            insert_prefecture_batch(transaction, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        insert_prefecture_batch(transaction, &batch).await?;
    }
    Ok(count)
}

async fn stage_municipalities(
    source: &PgPool,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<u64, DynError> {
    let mut rows = sqlx::query(
        "SELECT lg_code, county, city, ward FROM public.mt_city_unified ORDER BY lg_code",
    )
    .fetch(source);
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut count = 0_u64;

    while let Some(row) = rows.try_next().await? {
        let lg_code: String = row.try_get("lg_code")?;
        let county: Option<String> = row.try_get("county")?;
        let city: String = row.try_get("city")?;
        let ward: Option<String> = row.try_get("ward")?;
        let lg_code = validate_lg_code(&lg_code)?;
        let county = normalize_optional(county);
        let city = required_component(&city, "city", &lg_code)?;
        let ward = normalize_optional(ward);
        let match_name = concat_components([
            county.as_deref(),
            Some(city.as_str()),
            ward.as_deref(),
        ]);
        if match_name.is_empty() {
            return Err(import_error(format!(
                "municipality {lg_code} produced an empty match_name"
            )));
        }

        batch.push(MunicipalityRow {
            pref_code: lg_code[..2].to_string(),
            lg_code,
            match_name,
            county,
            city,
            ward,
        });
        count += 1;

        if batch.len() >= BATCH_SIZE {
            insert_municipality_batch(transaction, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        insert_municipality_batch(transaction, &batch).await?;
    }
    Ok(count)
}

async fn stage_machiaza(
    source: &PgPool,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<u64, DynError> {
    let mut rows = sqlx::query(
        r#"
        SELECT lg_code, machiaza_id, oaza_cho, chome, koaza,
               rsdt_addr_flg, koaza_aka_code
        FROM public.mt_town_unified
        ORDER BY lg_code, machiaza_id
        "#,
    )
    .fetch(source);
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut count = 0_u64;

    while let Some(row) = rows.try_next().await? {
        let lg_code: String = row.try_get("lg_code")?;
        let machiaza_id: String = row.try_get("machiaza_id")?;
        let oaza_cho: Option<String> = row.try_get("oaza_cho")?;
        let chome: Option<String> = row.try_get("chome")?;
        let koaza: Option<String> = row.try_get("koaza")?;
        let rsdt_addr_flg: i16 = row.try_get("rsdt_addr_flg")?;
        let koaza_aka_code: Option<i16> = row.try_get("koaza_aka_code")?;

        let lg_code = validate_lg_code(&lg_code)?;
        let machiaza_id = validate_machiaza_id(&machiaza_id, &lg_code)?;
        let oaza_cho = normalize_optional(oaza_cho);
        let chome = normalize_optional(chome);
        let koaza = normalize_optional(koaza);

        // Match the official ABR geocoder's component order. Kyoto street
        // names (koaza_aka_code=2) appear before oaza/chome and must not be
        // appended again as an ordinary koaza.
        let match_name = if koaza_aka_code == Some(2) {
            concat_components([koaza.as_deref(), oaza_cho.as_deref(), chome.as_deref()])
        } else {
            concat_components([oaza_cho.as_deref(), chome.as_deref(), koaza.as_deref()])
        };
        if match_name.is_empty() {
            return Err(import_error(format!(
                "machiaza {lg_code}/{machiaza_id} produced an empty match_name"
            )));
        }

        batch.push(MachiazaRow {
            lg_code,
            machiaza_id,
            match_name,
            oaza_cho,
            chome,
            koaza,
            rsdt_addr_flg,
        });
        count += 1;

        if batch.len() >= BATCH_SIZE {
            insert_machiaza_batch(transaction, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        insert_machiaza_batch(transaction, &batch).await?;
    }
    Ok(count)
}

async fn insert_prefecture_batch(
    transaction: &mut Transaction<'_, Postgres>,
    rows: &[PrefectureRow],
) -> Result<(), DynError> {
    let mut builder: QueryBuilder<Postgres> =
        QueryBuilder::new("INSERT INTO jp_prefectures_stage (pref_code, name) ");
    builder.push_values(rows, |mut values, row| {
        values.push_bind(&row.pref_code).push_bind(&row.name);
    });
    builder.build().execute(&mut **transaction).await?;
    Ok(())
}

async fn insert_municipality_batch(
    transaction: &mut Transaction<'_, Postgres>,
    rows: &[MunicipalityRow],
) -> Result<(), DynError> {
    let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO jp_municipalities_stage (lg_code, pref_code, match_name, county, city, ward) ",
    );
    builder.push_values(rows, |mut values, row| {
        values
            .push_bind(&row.lg_code)
            .push_bind(&row.pref_code)
            .push_bind(&row.match_name)
            .push_bind(&row.county)
            .push_bind(&row.city)
            .push_bind(&row.ward);
    });
    builder.build().execute(&mut **transaction).await?;
    Ok(())
}

async fn insert_machiaza_batch(
    transaction: &mut Transaction<'_, Postgres>,
    rows: &[MachiazaRow],
) -> Result<(), DynError> {
    let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO jp_machiaza_stage (lg_code, machiaza_id, match_name, oaza_cho, chome, koaza, rsdt_addr_flg) ",
    );
    builder.push_values(rows, |mut values, row| {
        values
            .push_bind(&row.lg_code)
            .push_bind(&row.machiaza_id)
            .push_bind(&row.match_name)
            .push_bind(&row.oaza_cho)
            .push_bind(&row.chome)
            .push_bind(&row.koaza)
            .push_bind(row.rsdt_addr_flg);
    });
    builder.build().execute(&mut **transaction).await?;
    Ok(())
}

async fn validate_staged_data(
    transaction: &mut Transaction<'_, Postgres>,
    prefectures: u64,
    municipalities: u64,
    machiaza: u64,
) -> Result<(), DynError> {
    if prefectures != 47 {
        return Err(import_error(format!(
            "expected all 47 Japanese prefectures but abrdb provided {prefectures}; run abrdb init --pref all --category basic and abrdb import"
        )));
    }
    if municipalities == 0 || machiaza == 0 {
        return Err(import_error(
            "abrdb returned no municipality or machiaza rows; import category basic before running this CLI",
        ));
    }

    let duplicate_prefectures: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT pref_code FROM jp_prefectures_stage GROUP BY pref_code HAVING COUNT(*) > 1) duplicated",
    )
    .fetch_one(&mut **transaction)
    .await?;
    ensure_zero(duplicate_prefectures, "duplicate prefecture codes")?;

    let duplicate_municipality_codes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT lg_code FROM jp_municipalities_stage GROUP BY lg_code HAVING COUNT(*) > 1) duplicated",
    )
    .fetch_one(&mut **transaction)
    .await?;
    ensure_zero(duplicate_municipality_codes, "duplicate municipality lg_code values")?;

    let duplicate_municipality_names: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT pref_code, match_name FROM jp_municipalities_stage GROUP BY pref_code, match_name HAVING COUNT(*) > 1) duplicated",
    )
    .fetch_one(&mut **transaction)
    .await?;
    ensure_zero(
        duplicate_municipality_names,
        "duplicate municipality match_name values within a prefecture",
    )?;

    let duplicate_machiaza: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT lg_code, machiaza_id FROM jp_machiaza_stage GROUP BY lg_code, machiaza_id HAVING COUNT(*) > 1) duplicated",
    )
    .fetch_one(&mut **transaction)
    .await?;
    ensure_zero(duplicate_machiaza, "duplicate machiaza identifiers")?;

    let orphan_municipalities: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM jp_municipalities_stage municipality
        LEFT JOIN jp_prefectures_stage prefecture
          ON prefecture.pref_code = municipality.pref_code
        WHERE prefecture.pref_code IS NULL
        "#,
    )
    .fetch_one(&mut **transaction)
    .await?;
    ensure_zero(orphan_municipalities, "municipalities without a prefecture")?;

    let orphan_machiaza: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM jp_machiaza_stage machiaza
        LEFT JOIN jp_municipalities_stage municipality
          ON municipality.lg_code = machiaza.lg_code
        WHERE municipality.lg_code IS NULL
        "#,
    )
    .fetch_one(&mut **transaction)
    .await?;
    ensure_zero(orphan_machiaza, "machiaza rows without a municipality")?;

    Ok(())
}

async fn replace_master(
    transaction: &mut Transaction<'_, Postgres>,
    dataset_version: Option<&str>,
) -> Result<(), DynError> {
    // The staging tables are fully loaded and validated before this point.
    // Permanent tables are locked/replaced only for this short final section.
    sqlx::query(
        "TRUNCATE admin_master.jp_machiaza, admin_master.jp_municipalities, admin_master.jp_prefectures",
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO admin_master.jp_prefectures (pref_code, name)
        SELECT pref_code, name
        FROM jp_prefectures_stage
        ORDER BY pref_code
        "#,
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO admin_master.jp_municipalities
            (lg_code, pref_code, match_name, county, city, ward)
        SELECT lg_code, pref_code, match_name, county, city, ward
        FROM jp_municipalities_stage
        ORDER BY lg_code
        "#,
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO admin_master.jp_machiaza
            (lg_code, machiaza_id, match_name, oaza_cho, chome, koaza, rsdt_addr_flg)
        SELECT lg_code, machiaza_id, match_name, oaza_cho, chome, koaza, rsdt_addr_flg
        FROM jp_machiaza_stage
        ORDER BY lg_code, machiaza_id
        "#,
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO admin_master.datasets
            (country_code, source_name, source_url, dataset_version, imported_at)
        VALUES ('JP', $1, $2, $3, NOW())
        ON CONFLICT (country_code) DO UPDATE SET
            source_name = EXCLUDED.source_name,
            source_url = EXCLUDED.source_url,
            dataset_version = EXCLUDED.dataset_version,
            imported_at = EXCLUDED.imported_at
        "#,
    )
    .bind(SOURCE_NAME)
    .bind(SOURCE_URL)
    .bind(dataset_version)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

fn validate_lg_code(value: &str) -> Result<String, DynError> {
    let normalized = value.trim();
    if normalized.len() != 6 || !normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(import_error(format!("invalid ABR lg_code: {value:?}")));
    }
    Ok(normalized.to_string())
}

fn validate_machiaza_id(value: &str, lg_code: &str) -> Result<String, DynError> {
    let normalized = value.trim();
    if normalized.len() != 7 || !normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(import_error(format!(
            "invalid ABR machiaza_id for {lg_code}: {value:?}"
        )));
    }
    Ok(normalized.to_string())
}

fn required_component(value: &str, field: &str, identifier: &str) -> Result<String, DynError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(import_error(format!(
            "ABR {field} is empty for {identifier}"
        )));
    }
    Ok(normalized.to_string())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(nonempty_string)
}

fn nonempty_string(value: String) -> Option<String> {
    let normalized = value.trim();
    (!normalized.is_empty()).then(|| normalized.to_string())
}

fn concat_components<'a>(components: impl IntoIterator<Item = Option<&'a str>>) -> String {
    components
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|component| !component.is_empty())
        .collect::<String>()
}

fn ensure_zero(value: i64, description: &str) -> Result<(), DynError> {
    if value == 0 {
        Ok(())
    } else {
        Err(import_error(format!(
            "staged administrative master contains {value} {description}"
        )))
    }
}

fn import_error(message: impl Into<String>) -> DynError {
    Box::new(ImportError(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn municipality_match_name_preserves_administrative_hierarchy() {
        assert_eq!(
            concat_components([Some("蒲生郡"), Some("日野町"), None]),
            "蒲生郡日野町"
        );
        assert_eq!(
            concat_components([None, Some("横浜市"), Some("中区")]),
            "横浜市中区"
        );
        assert_eq!(
            concat_components([None, Some("大津市"), None]),
            "大津市"
        );
    }

    #[test]
    fn machiaza_match_name_uses_abr_component_order() {
        assert_eq!(
            concat_components([Some("勝谷町"), None, None]),
            "勝谷町"
        );
        assert_eq!(
            concat_components([Some("紀尾井町"), Some("一丁目"), Some("小字")]),
            "紀尾井町一丁目小字"
        );
    }

    #[test]
    fn identifiers_are_strictly_validated() {
        assert_eq!(validate_lg_code("252018").unwrap(), "252018");
        assert!(validate_lg_code("25").is_err());
        assert_eq!(
            validate_machiaza_id("0000001", "252018").unwrap(),
            "0000001"
        );
        assert!(validate_machiaza_id("abc", "252018").is_err());
    }
}
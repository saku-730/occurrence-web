use std::process::Stdio;

use sqlx::{PgPool, Postgres, QueryBuilder, postgres::PgPoolOptions};
use tokio::process::Command;

fn database_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set for administrative master importer tests")
}

async fn test_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url())
        .await
        .expect("test PostgreSQL should be available")
}

async fn create_fake_abrdb(pool: &PgPool) {
    for table in ["mt_town_unified", "mt_city_unified", "mt_pref_unified"] {
        sqlx::query(&format!("DROP TABLE IF EXISTS public.{table}"))
            .execute(pool)
            .await
            .expect("old fake ABR table should drop");
    }

    sqlx::query(
        r#"
        CREATE TABLE public.mt_pref_unified (
            lg_code TEXT NOT NULL,
            pref TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("fake prefecture table should be created");

    sqlx::query(
        r#"
        CREATE TABLE public.mt_city_unified (
            lg_code TEXT NOT NULL,
            county TEXT,
            city TEXT NOT NULL,
            ward TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("fake municipality table should be created");

    sqlx::query(
        r#"
        CREATE TABLE public.mt_town_unified (
            lg_code TEXT NOT NULL,
            machiaza_id TEXT NOT NULL,
            oaza_cho TEXT,
            chome TEXT,
            koaza TEXT,
            rsdt_addr_flg SMALLINT NOT NULL,
            koaza_aka_code SMALLINT
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("fake machiaza table should be created");

    let prefectures: Vec<(String, String)> = (1..=47)
        .map(|code| {
            let pref_code = format!("{code:02}");
            let name = if code == 25 {
                "滋賀県".to_string()
            } else {
                format!("試験県{pref_code}")
            };
            (format!("{pref_code}0000"), name)
        })
        .collect();
    let mut builder: QueryBuilder<Postgres> =
        QueryBuilder::new("INSERT INTO public.mt_pref_unified (lg_code, pref) ");
    builder.push_values(&prefectures, |mut values, (lg_code, pref)| {
        values.push_bind(lg_code).push_bind(pref);
    });
    builder
        .build()
        .execute(pool)
        .await
        .expect("47 fake prefectures should be inserted");

    sqlx::query(
        r#"
        INSERT INTO public.mt_city_unified (lg_code, county, city, ward)
        VALUES ('252018', NULL, '大津市', NULL)
        "#,
    )
    .execute(pool)
    .await
    .expect("fake Otsu municipality should be inserted");

    sqlx::query(
        r#"
        INSERT INTO public.mt_town_unified
            (lg_code, machiaza_id, oaza_cho, chome, koaza, rsdt_addr_flg, koaza_aka_code)
        VALUES ('252018', '0000001', '勝谷町', NULL, NULL, 0, 0)
        "#,
    )
    .execute(pool)
    .await
    .expect("fake Katsutani machiaza should be inserted");
}

async fn drop_fake_abrdb(pool: &PgPool) {
    for table in ["mt_town_unified", "mt_city_unified", "mt_pref_unified"] {
        sqlx::query(&format!("DROP TABLE IF EXISTS public.{table}"))
            .execute(pool)
            .await
            .expect("fake ABR table should be removed");
    }
}

#[tokio::test]
async fn cli_imports_abrdb_basic_tables_into_admin_master() {
    let pool = test_pool().await;
    create_fake_abrdb(&pool).await;

    let url = database_url();
    let output = Command::new(env!("CARGO_BIN_EXE_import_admin_master"))
        .arg("--country")
        .arg("JP")
        .arg("--dataset-version")
        .arg("ci-fixture")
        .env("ABR_DATABASE_URL", &url)
        .env("DATABASE_URL", &url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("import_admin_master binary should start");

    if !output.status.success() {
        panic!(
            "import_admin_master failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let prefecture_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM admin_master.jp_prefectures")
            .fetch_one(&pool)
            .await
            .expect("prefecture count should be readable");
    assert_eq!(prefecture_count, 47);

    let municipality: String = sqlx::query_scalar(
        "SELECT match_name FROM admin_master.jp_municipalities WHERE lg_code = '252018'",
    )
    .fetch_one(&pool)
    .await
    .expect("Otsu should be imported");
    assert_eq!(municipality, "大津市");

    let machiaza: String = sqlx::query_scalar(
        r#"
        SELECT match_name
        FROM admin_master.jp_machiaza
        WHERE lg_code = '252018' AND machiaza_id = '0000001'
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("Katsutani should be imported");
    assert_eq!(machiaza, "勝谷町");

    let dataset_version: Option<String> = sqlx::query_scalar(
        "SELECT dataset_version FROM admin_master.datasets WHERE country_code = 'JP'",
    )
    .fetch_one(&pool)
    .await
    .expect("dataset metadata should be recorded");
    assert_eq!(dataset_version.as_deref(), Some("ci-fixture"));

    drop_fake_abrdb(&pool).await;
}

use backend::infrastructure::abr::{AbrClient, AdministrativeMatchLevel};
use sqlx::{PgPool, postgres::PgPoolOptions};

fn database_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for direct ABR tests")
}

async fn pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url())
        .await
        .expect("test PostgreSQL should be available")
}

async fn create_fake_abr(pool: &PgPool) {
    for table in ["mt_town_unified", "mt_city_unified", "mt_pref_unified"] {
        sqlx::query(&format!("DROP TABLE IF EXISTS public.{table}"))
            .execute(pool)
            .await
            .unwrap();
    }

    sqlx::query(
        r#"
        CREATE TABLE public.mt_pref_unified (
            lg_code TEXT NOT NULL,
            pref TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE public.mt_city_unified (
            lg_code TEXT NOT NULL,
            county TEXT,
            city TEXT,
            ward TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE public.mt_town_unified (
            lg_code TEXT NOT NULL,
            machiaza_id TEXT NOT NULL,
            oaza_cho TEXT,
            chome TEXT,
            koaza TEXT,
            rsdt_addr_flg TEXT,
            koaza_aka_code TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO public.mt_pref_unified (lg_code, pref) VALUES ('250007', '滋賀県')",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO public.mt_city_unified (lg_code, county, city, ward) VALUES ('252018', NULL, '大津市', NULL)",
    )
    .execute(pool)
    .await
    .unwrap();

    // 0000000 is ABR's unknown-machiaza sentinel and must not become a match.
    sqlx::query(
        r#"
        INSERT INTO public.mt_town_unified
            (lg_code, machiaza_id, oaza_cho, chome, koaza, rsdt_addr_flg, koaza_aka_code)
        VALUES
            ('252018', '0000000', NULL, NULL, NULL, '0', NULL),
            ('252018', '0000001', '勝谷町', NULL, NULL, '0', '0'),
            ('252018', '0000001', '勝谷町', NULL, NULL, '1', '0')
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn drop_fake_abr(pool: &PgPool) {
    for table in ["mt_town_unified", "mt_city_unified", "mt_pref_unified"] {
        sqlx::query(&format!("DROP TABLE IF EXISTS public.{table}"))
            .execute(pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn resolves_official_abr_tables_and_reuses_cached_search_result() {
    let pool = pool().await;
    create_fake_abr(&pool).await;

    let client = AbrClient::new(&database_url(), 16).expect("ABR client should build");
    let resolved = client
        .resolve("Japan", " 滋賀県　大津市 勝谷町 採集地点 ")
        .await
        .expect("ABR lookup should succeed")
        .expect("Japanese locality should resolve");

    assert_eq!(resolved.country_code, "JP");
    assert_eq!(resolved.prefecture_code, "25");
    assert_eq!(resolved.prefecture, "滋賀県");
    assert_eq!(resolved.municipality_code.as_deref(), Some("252018"));
    assert_eq!(resolved.municipality.as_deref(), Some("大津市"));
    assert_eq!(resolved.machiaza_id.as_deref(), Some("0000001"));
    assert_eq!(resolved.machiaza.as_deref(), Some("勝谷町"));
    assert_eq!(resolved.remainder.as_deref(), Some("採集地点"));
    assert_eq!(resolved.match_level, AdministrativeMatchLevel::Machiaza);

    // Remove the source row. The same search still succeeds because the process-local
    // cache stores the resolution and does not duplicate the ABR master persistently.
    sqlx::query("DELETE FROM public.mt_pref_unified")
        .execute(&pool)
        .await
        .unwrap();
    let cached = client
        .resolve("JP", "滋賀県大津市勝谷町採集地点")
        .await
        .expect("cached lookup should succeed")
        .expect("cached result should remain available");
    assert_eq!(cached, resolved);

    client.clear_cache().await;
    assert!(
        client
            .resolve("JP", "滋賀県大津市勝谷町採集地点")
            .await
            .expect("lookup after cache clear should complete")
            .is_none()
    );

    drop_fake_abr(&pool).await;
}

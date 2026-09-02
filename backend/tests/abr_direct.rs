use std::sync::{Arc, Mutex};

use backend::{
    features::occurrence_map::geocoding::{
        GeocodedLocation, LocationGeocoder, LocationGeocoderError,
        enrich_nquads_with_geocoding_and_abr,
    },
    infrastructure::abr::AbrClient,
};
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

#[derive(Clone)]
struct FallbackGeocoder {
    queries: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl LocationGeocoder for FallbackGeocoder {
    async fn geocode(
        &self,
        query: &str,
    ) -> Result<Option<GeocodedLocation>, LocationGeocoderError> {
        self.queries.lock().unwrap().push(query.to_string());
        if query == "勝谷町, 大津市, 滋賀県, Japan" {
            Ok(Some(GeocodedLocation {
                latitude: "35.0001".into(),
                longitude: "135.9001".into(),
            }))
        } else {
            Ok(None)
        }
    }
}

#[tokio::test]
async fn abr_split_drives_nominatim_and_only_nominatim_is_recorded_as_source() {
    let pool = pool().await;
    create_fake_abr(&pool).await;

    let abr = AbrClient::new(&database_url(), 16).expect("ABR client should build");
    let queries = Arc::new(Mutex::new(Vec::new()));
    let geocoder = FallbackGeocoder {
        queries: queries.clone(),
    };
    let input = r#"_:o <http://rs.tdwg.org/dwc/terms/locality> "滋賀県大津市勝谷町採集地点" <https://bio-database.net/graphs/occurrences> .
_:o <http://rs.tdwg.org/dwc/terms/country> "Japan" <https://bio-database.net/graphs/occurrences> ."#;

    let output = enrich_nquads_with_geocoding_and_abr(input.as_bytes(), &geocoder, Some(&abr))
        .await
        .expect("ABR split followed by Nominatim geocoding should complete");
    let text = String::from_utf8(output).unwrap();

    // The first query keeps ABR's remainder. The fake Nominatim returns zero results,
    // so the second query is the ABR-derived machiaza fallback and succeeds.
    assert_eq!(
        queries.lock().unwrap().as_slice(),
        &[
            "採集地点, 勝谷町, 大津市, 滋賀県, Japan",
            "勝谷町, 大津市, 滋賀県, Japan",
        ]
    );
    assert!(text.contains("http://rs.tdwg.org/dwc/terms/decimalLatitude"));
    assert!(text.contains("35.0001"));
    assert!(text.contains("http://rs.tdwg.org/dwc/terms/decimalLongitude"));
    assert!(text.contains("135.9001"));
    assert!(text.contains("http://rs.tdwg.org/dwc/iri/georeferenceSources"));
    assert!(text.contains("https://nominatim.openstreetmap.org/"));
    assert!(!text.contains("digital.go.jp"));
    assert_eq!(
        text.matches("http://rs.tdwg.org/dwc/iri/georeferenceSources")
            .count(),
        1
    );
    assert!(text.contains("machiaza fallback"));

    drop_fake_abr(&pool).await;
}

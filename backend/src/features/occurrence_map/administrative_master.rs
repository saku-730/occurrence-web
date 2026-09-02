use sqlx::{PgPool, Row};

#[derive(Debug)]
pub enum AdministrativeMasterError {
    Database(sqlx::Error),
}

impl From<sqlx::Error> for AdministrativeMasterError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdministrativeMatchLevel {
    Prefecture,
    Municipality,
    Machiaza,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdministrativeResolution {
    pub country_code: String,
    pub prefecture_code: String,
    pub prefecture: String,
    pub municipality_code: Option<String>,
    pub municipality: Option<String>,
    pub machiaza_id: Option<String>,
    pub machiaza: Option<String>,
    pub remainder: Option<String>,
    pub match_level: AdministrativeMatchLevel,
}

/// Resolve administrative prefixes only for countries whose master data is
/// implemented. Unsupported countries deliberately return None so normal
/// free-form geocoding can continue unchanged.
pub async fn resolve_administrative_location(
    pool: &PgPool,
    country: &str,
    locality: &str,
) -> Result<Option<AdministrativeResolution>, AdministrativeMasterError> {
    match normalize_country_code(country) {
        Some("JP") => resolve_japan(pool, locality).await,
        _ => Ok(None),
    }
}

/// Resolve a Japanese locality against the imported administrative master.
///
/// The resolver consumes the longest known prefix at each level:
/// prefecture -> municipality -> machi-aza. It does not rewrite the original
/// Darwin Core values. The returned structure is intended for geocoding query
/// construction and validation only.
pub async fn resolve_japan(
    pool: &PgPool,
    locality: &str,
) -> Result<Option<AdministrativeResolution>, AdministrativeMasterError> {
    let normalized = normalize_locality(locality);
    if normalized.is_empty() {
        return Ok(None);
    }

    let prefecture_row = sqlx::query(
        r#"
        SELECT pref_code, name
        FROM admin_master.jp_prefectures
        WHERE $1 LIKE name || '%'
        ORDER BY char_length(name) DESC, pref_code
        LIMIT 1
        "#,
    )
    .bind(&normalized)
    .fetch_optional(pool)
    .await?;

    let Some(prefecture_row) = prefecture_row else {
        return Ok(None);
    };

    let prefecture_code: String = prefecture_row.try_get("pref_code")?;
    let prefecture: String = prefecture_row.try_get("name")?;
    let mut remainder = consume_prefix(&normalized, &prefecture);

    let mut resolution = AdministrativeResolution {
        country_code: "JP".to_string(),
        prefecture_code: prefecture_code.clone(),
        prefecture,
        municipality_code: None,
        municipality: None,
        machiaza_id: None,
        machiaza: None,
        remainder: nonempty(remainder.clone()),
        match_level: AdministrativeMatchLevel::Prefecture,
    };

    let municipality_row = sqlx::query(
        r#"
        SELECT lg_code, match_name
        FROM admin_master.jp_municipalities
        WHERE pref_code = $1
          AND $2 LIKE match_name || '%'
        ORDER BY char_length(match_name) DESC, lg_code
        LIMIT 1
        "#,
    )
    .bind(&prefecture_code)
    .bind(&remainder)
    .fetch_optional(pool)
    .await?;

    let Some(municipality_row) = municipality_row else {
        return Ok(Some(resolution));
    };

    let municipality_code: String = municipality_row.try_get("lg_code")?;
    let municipality: String = municipality_row.try_get("match_name")?;
    remainder = consume_prefix(&remainder, &municipality);
    resolution.municipality_code = Some(municipality_code.clone());
    resolution.municipality = Some(municipality);
    resolution.remainder = nonempty(remainder.clone());
    resolution.match_level = AdministrativeMatchLevel::Municipality;

    if remainder.is_empty() {
        return Ok(Some(resolution));
    }

    let machiaza_row = sqlx::query(
        r#"
        SELECT machiaza_id, match_name
        FROM admin_master.jp_machiaza
        WHERE lg_code = $1
          AND $2 LIKE match_name || '%'
        ORDER BY char_length(match_name) DESC, machiaza_id
        LIMIT 1
        "#,
    )
    .bind(&municipality_code)
    .bind(&remainder)
    .fetch_optional(pool)
    .await?;

    let Some(machiaza_row) = machiaza_row else {
        return Ok(Some(resolution));
    };

    let machiaza_id: String = machiaza_row.try_get("machiaza_id")?;
    let machiaza: String = machiaza_row.try_get("match_name")?;
    remainder = consume_prefix(&remainder, &machiaza);
    resolution.machiaza_id = Some(machiaza_id);
    resolution.machiaza = Some(machiaza);
    resolution.remainder = nonempty(remainder);
    resolution.match_level = AdministrativeMatchLevel::Machiaza;

    Ok(Some(resolution))
}

pub fn normalize_country_code(country: &str) -> Option<&'static str> {
    let normalized = country.trim().to_lowercase();
    match normalized.as_str() {
        "jp" | "jpn" | "japan" | "日本" | "日本国" => Some("JP"),
        _ => None,
    }
}

fn normalize_locality(locality: &str) -> String {
    locality
        .trim()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn consume_prefix(value: &str, prefix: &str) -> String {
    value.strip_prefix(prefix).unwrap_or(value).to_string()
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    #[test]
    fn normalizes_supported_japan_country_names() {
        for country in ["Japan", " japan ", "JP", "JPN", "日本", "日本国"] {
            assert_eq!(normalize_country_code(country), Some("JP"));
        }
        assert_eq!(normalize_country_code("United States"), None);
    }

    #[test]
    fn locality_normalization_removes_ascii_and_full_width_whitespace() {
        assert_eq!(
            normalize_locality("  滋賀県　大津市 勝谷町  "),
            "滋賀県大津市勝谷町"
        );
    }

    #[test]
    fn prefix_consumption_keeps_only_unresolved_detail() {
        let locality = normalize_locality("滋賀県大津市勝谷町");
        let after_prefecture = consume_prefix(&locality, "滋賀県");
        let after_municipality = consume_prefix(&after_prefecture, "大津市");
        let after_machiaza = consume_prefix(&after_municipality, "勝谷町");

        assert_eq!(after_prefecture, "大津市勝谷町");
        assert_eq!(after_municipality, "勝谷町");
        assert_eq!(after_machiaza, "");
    }

    #[test]
    fn nonmatching_prefix_does_not_destroy_locality() {
        assert_eq!(consume_prefix("大津市勝谷町", "京都市"), "大津市勝谷町");
    }

    #[tokio::test]
    async fn resolves_hierarchy_from_postgres_master_when_database_is_available() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("administrative master test database should connect");

        sqlx::query(
            "INSERT INTO admin_master.jp_prefectures (pref_code, name) VALUES ('98', '試験県')",
        )
        .execute(&pool)
        .await
        .expect("test prefecture should insert");
        sqlx::query(
            r#"
            INSERT INTO admin_master.jp_municipalities
                (lg_code, pref_code, match_name, city)
            VALUES ('980001', '98', '試験市', '試験市')
            "#,
        )
        .execute(&pool)
        .await
        .expect("test municipality should insert");
        sqlx::query(
            r#"
            INSERT INTO admin_master.jp_machiaza
                (lg_code, machiaza_id, match_name, oaza_cho, rsdt_addr_flg)
            VALUES ('980001', '0000001', '試験町', '試験町', 0)
            "#,
        )
        .execute(&pool)
        .await
        .expect("test machiaza should insert");

        let resolved = resolve_administrative_location(
            &pool,
            "Japan",
            " 試験県　試験市 試験町 採集地点 ",
        )
        .await
        .expect("master lookup should succeed")
        .expect("Japanese master should resolve the test locality");

        assert_eq!(resolved.country_code, "JP");
        assert_eq!(resolved.prefecture_code, "98");
        assert_eq!(resolved.prefecture, "試験県");
        assert_eq!(resolved.municipality_code.as_deref(), Some("980001"));
        assert_eq!(resolved.municipality.as_deref(), Some("試験市"));
        assert_eq!(resolved.machiaza_id.as_deref(), Some("0000001"));
        assert_eq!(resolved.machiaza.as_deref(), Some("試験町"));
        assert_eq!(resolved.remainder.as_deref(), Some("採集地点"));
        assert_eq!(resolved.match_level, AdministrativeMatchLevel::Machiaza);

        sqlx::query("DELETE FROM admin_master.jp_municipalities WHERE lg_code = '980001'")
            .execute(&pool)
            .await
            .expect("test municipality should clean up");
        sqlx::query("DELETE FROM admin_master.jp_prefectures WHERE pref_code = '98'")
            .execute(&pool)
            .await
            .expect("test prefecture should clean up");
    }
}

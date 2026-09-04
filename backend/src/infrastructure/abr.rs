use std::{
    collections::{HashMap, VecDeque},
    env,
    sync::OnceLock,
};

use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tokio::sync::Mutex;

const COUNTRY_CODE_JP: &str = "JP";
const UNKNOWN_MACHIAZA_ID: &str = "0000000";
const DEFAULT_CACHE_CAPACITY: usize = 4_096;

#[derive(Debug)]
pub enum AbrError {
    Database(sqlx::Error),
}

impl From<sqlx::Error> for AbrError {
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

/// Read the official Digital Agency ABR PostgreSQL database directly.
///
/// ABR remains the only persistent administrative master. Bio-Database keeps
/// only a bounded process-local cache keyed by normalized country + locality.
pub struct AbrClient {
    pool: PgPool,
    cache: Mutex<ResolutionCache>,
}

impl AbrClient {
    pub fn new(database_url: &str, cache_capacity: usize) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_lazy(database_url)?;
        Ok(Self::from_pool(pool, cache_capacity))
    }

    pub fn from_pool(pool: PgPool, cache_capacity: usize) -> Self {
        Self {
            pool,
            cache: Mutex::new(ResolutionCache::new(cache_capacity)),
        }
    }

    /// Global optional ABR connection used by occurrence geocoding.
    /// If ABR_DATABASE_URL is absent or malformed, callers can fail open and
    /// continue with ordinary free-form Nominatim geocoding.
    pub fn global() -> Option<&'static Self> {
        static GLOBAL: OnceLock<Option<AbrClient>> = OnceLock::new();

        GLOBAL
            .get_or_init(|| {
                dotenvy::dotenv().ok();
                let database_url = env::var("ABR_DATABASE_URL").ok()?;
                let capacity = env::var("ABR_SEARCH_CACHE_CAPACITY")
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(DEFAULT_CACHE_CAPACITY);
                AbrClient::new(&database_url, capacity).ok()
            })
            .as_ref()
    }

    pub async fn resolve(
        &self,
        country: &str,
        locality: &str,
    ) -> Result<Option<AdministrativeResolution>, AbrError> {
        let Some(country_code) = normalize_country_code(country) else {
            return Ok(None);
        };
        if country_code != COUNTRY_CODE_JP {
            return Ok(None);
        }

        let locality = normalize_locality(locality);
        if locality.is_empty() {
            return Ok(None);
        }

        let cache_key = format!("{country_code}\0{locality}");
        if let Some(cached) = self.cache.lock().await.get(&cache_key) {
            return Ok(cached);
        }

        let resolved = self.resolve_japan(&locality).await?;
        self.cache.lock().await.insert(cache_key, resolved.clone());
        Ok(resolved)
    }

    pub async fn clear_cache(&self) {
        self.cache.lock().await.clear();
    }

    async fn resolve_japan(
        &self,
        locality: &str,
    ) -> Result<Option<AdministrativeResolution>, AbrError> {
        let prefecture_row = sqlx::query(
            r#"
            SELECT SUBSTRING(lg_code::text, 1, 2) AS pref_code, pref
            FROM public.mt_pref_unified
            WHERE pref IS NOT NULL
              AND BTRIM(pref) <> ''
              AND $1 LIKE pref || '%'
            ORDER BY CHAR_LENGTH(pref) DESC, lg_code::text
            LIMIT 1
            "#,
        )
        .bind(locality)
        .fetch_optional(&self.pool)
        .await?;

        let Some(prefecture_row) = prefecture_row else {
            return Ok(None);
        };

        let prefecture_code: String = prefecture_row.try_get("pref_code")?;
        let prefecture: String = prefecture_row.try_get("pref")?;
        let mut remainder = consume_prefix(locality, &prefecture);

        let mut resolution = AdministrativeResolution {
            country_code: COUNTRY_CODE_JP.to_string(),
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
            WITH candidates AS (
                SELECT
                    lg_code::text AS lg_code,
                    CONCAT(COALESCE(county, ''), COALESCE(city, ''), COALESCE(ward, '')) AS match_name
                FROM public.mt_city_unified
                WHERE SUBSTRING(lg_code::text, 1, 2) = $1
            )
            SELECT lg_code, match_name
            FROM candidates
            WHERE match_name <> ''
              AND $2 LIKE match_name || '%'
            ORDER BY CHAR_LENGTH(match_name) DESC, lg_code
            LIMIT 1
            "#,
        )
        .bind(&prefecture_code)
        .bind(&remainder)
        .fetch_optional(&self.pool)
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
            WITH candidates AS (
                SELECT
                    machiaza_id::text AS machiaza_id,
                    CASE
                        WHEN COALESCE(koaza_aka_code::text, '') = '2'
                        THEN CONCAT(COALESCE(koaza, ''), COALESCE(oaza_cho, ''), COALESCE(chome, ''))
                        ELSE CONCAT(COALESCE(oaza_cho, ''), COALESCE(chome, ''), COALESCE(koaza, ''))
                    END AS match_name
                FROM public.mt_town_unified
                WHERE lg_code::text = $1
                  AND machiaza_id::text <> $3
            )
            SELECT machiaza_id, match_name
            FROM candidates
            WHERE match_name <> ''
              AND $2 LIKE match_name || '%'
            ORDER BY CHAR_LENGTH(match_name) DESC, machiaza_id
            LIMIT 1
            "#,
        )
        .bind(&municipality_code)
        .bind(&remainder)
        .bind(UNKNOWN_MACHIAZA_ID)
        .fetch_optional(&self.pool)
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
}

pub fn normalize_country_code(country: &str) -> Option<&'static str> {
    let normalized = country.trim().to_lowercase();
    match normalized.as_str() {
        "jp" | "jpn" | "japan" | "日本" | "日本国" => Some(COUNTRY_CODE_JP),
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

struct ResolutionCache {
    capacity: usize,
    entries: HashMap<String, Option<AdministrativeResolution>>,
    insertion_order: VecDeque<String>,
}

impl ResolutionCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            insertion_order: VecDeque::new(),
        }
    }

    fn get(&self, key: &str) -> Option<Option<AdministrativeResolution>> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: String, value: Option<AdministrativeResolution>) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.contains_key(&key) {
            self.entries.insert(key, value);
            return;
        }
        while self.entries.len() >= self.capacity {
            let Some(oldest) = self.insertion_order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.insertion_order.push_back(key.clone());
        self.entries.insert(key, value);
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.insertion_order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_supported_japan_country_names() {
        for country in ["Japan", " japan ", "JP", "JPN", "日本", "日本国"] {
            assert_eq!(normalize_country_code(country), Some("JP"));
        }
        assert_eq!(normalize_country_code("United States"), None);
    }

    #[test]
    fn bounded_cache_keeps_positive_and_negative_results() {
        let mut cache = ResolutionCache::new(2);
        cache.insert("a".into(), None);
        cache.insert("b".into(), None);
        assert_eq!(cache.get("a"), Some(None));

        cache.insert("c".into(), None);
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b"), Some(None));
        assert_eq!(cache.get("c"), Some(None));
    }

    #[test]
    fn locality_normalization_removes_whitespace() {
        assert_eq!(
            normalize_locality("  滋賀県　大津市 勝谷町  "),
            "滋賀県大津市勝谷町"
        );
    }
}

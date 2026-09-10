use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::features::occurrence_map::geocoding::{
    GeocodedLocation, LocationGeocoder, LocationGeocoderError, NOMINATIM_SOURCE_URI,
};

const MIN_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
const USER_AGENT: &str = "bio-database/0.1 occurrence geocoder";

#[derive(Debug, Default)]
struct NominatimState {
    cache: HashMap<String, Option<GeocodedLocation>>,
    last_request_started: Option<Instant>,
}

pub struct NominatimClient {
    http: reqwest::Client,
    base_url: String,
    min_request_interval: Duration,
    state: Mutex<NominatimState>,
}

#[derive(Debug, Deserialize)]
struct NominatimPlace {
    lat: String,
    lon: String,
}

impl NominatimClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("Nominatim HTTP client configuration should be valid");

        Self {
            http,
            base_url: NOMINATIM_SOURCE_URI.trim_end_matches('/').to_string(),
            min_request_interval: MIN_REQUEST_INTERVAL,
            state: Mutex::new(NominatimState::default()),
        }
    }

    pub fn global() -> &'static Self {
        static CLIENT: OnceLock<NominatimClient> = OnceLock::new();
        CLIENT.get_or_init(Self::new)
    }

    #[cfg(test)]
    fn with_base_url_and_interval(base_url: String, min_request_interval: Duration) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .unwrap(),
            base_url: base_url.trim_end_matches('/').to_string(),
            min_request_interval,
            state: Mutex::new(NominatimState::default()),
        }
    }
}

impl Default for NominatimClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl LocationGeocoder for NominatimClient {
    async fn geocode(
        &self,
        query: &str,
    ) -> Result<Option<GeocodedLocation>, LocationGeocoderError> {
        // Keep this lock through the HTTP request. This intentionally serializes every
        // external Nominatim access from this backend process, including paper batch import.
        let mut state = self.state.lock().await;

        if let Some(cached) = state.cache.get(query) {
            return Ok(cached.clone());
        }

        if let Some(last_started) = state.last_request_started {
            let elapsed = last_started.elapsed();
            if elapsed < self.min_request_interval {
                tokio::time::sleep(self.min_request_interval - elapsed).await;
            }
        }
        state.last_request_started = Some(Instant::now());

        let response = self
            .http
            .get(format!("{}/search", self.base_url))
            .query(&[("q", query), ("format", "jsonv2"), ("limit", "1")])
            .send()
            .await
            .map_err(|_| LocationGeocoderError::RequestFailed)?;

        if !response.status().is_success() {
            // Transport/service failures are not cached. A later registration can retry.
            return Err(LocationGeocoderError::RequestFailed);
        }

        let places = response
            .json::<Vec<NominatimPlace>>()
            .await
            .map_err(|_| LocationGeocoderError::RequestFailed)?;

        let result = match places.into_iter().next() {
            None => None,
            Some(place) => {
                let latitude = place
                    .lat
                    .parse::<f64>()
                    .map_err(|_| LocationGeocoderError::RequestFailed)?;
                let longitude = place
                    .lon
                    .parse::<f64>()
                    .map_err(|_| LocationGeocoderError::RequestFailed)?;
                if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
                    return Err(LocationGeocoderError::RequestFailed);
                }
                Some(GeocodedLocation {
                    latitude: place.lat,
                    longitude: place.lon,
                })
            }
        };

        // Successful lookups, including a valid zero-result response, are cached.
        state.cache.insert(query.to_string(), result.clone());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Json, Router, routing::get};

    use super::*;

    async fn spawn_fake_nominatim(
        body: serde_json::Value,
        request_count: Arc<AtomicUsize>,
    ) -> String {
        let app = Router::new().route(
            "/search",
            get(move || {
                let body = body.clone();
                let request_count = request_count.clone();
                async move {
                    request_count.fetch_add(1, Ordering::SeqCst);
                    Json(body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn same_successful_query_uses_process_cache() {
        let count = Arc::new(AtomicUsize::new(0));
        let base_url = spawn_fake_nominatim(
            serde_json::json!([{ "lat": "35.0116", "lon": "135.7681" }]),
            count.clone(),
        )
        .await;
        let client = NominatimClient::with_base_url_and_interval(base_url, Duration::ZERO);

        let first = client.geocode("Kyoto City").await.unwrap();
        let second = client.geocode("Kyoto City").await.unwrap();

        assert_eq!(first, second);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn zero_result_is_cached() {
        let count = Arc::new(AtomicUsize::new(0));
        let base_url = spawn_fake_nominatim(serde_json::json!([]), count.clone()).await;
        let client = NominatimClient::with_base_url_and_interval(base_url, Duration::ZERO);

        assert!(client.geocode("No such place").await.unwrap().is_none());
        assert!(client.geocode("No such place").await.unwrap().is_none());
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}

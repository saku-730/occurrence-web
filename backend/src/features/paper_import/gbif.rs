use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::Deserialize;

const GBIF_SPECIES_MATCH_URL: &str = "https://api.gbif.org/v1/species/match";
const GBIF_TAXON_URI_BASE: &str = "https://www.gbif.org/species/";
const GBIF_MATCH_TIMEOUT_SECONDS: u64 = 10;
const MIN_FUZZY_CONFIDENCE: u8 = 90;

#[derive(Debug)]
pub enum GbifMatchError {
    InvalidConfiguration,
    RequestFailed,
    Upstream(StatusCode),
    InvalidResponse,
}

#[derive(Debug, Clone)]
pub struct GbifClient {
    http: Client,
    endpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GbifTaxonMatch {
    pub taxon_uri: String,
    pub scientific_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GbifSpeciesMatchResponse {
    usage_key: Option<u64>,
    accepted_usage_key: Option<u64>,
    scientific_name: Option<String>,
    accepted_scientific_name: Option<String>,
    match_type: Option<String>,
    confidence: Option<u8>,
}

impl GbifClient {
    pub fn new() -> Result<Self, GbifMatchError> {
        Self::with_endpoint(
            GBIF_SPECIES_MATCH_URL,
            Duration::from_secs(GBIF_MATCH_TIMEOUT_SECONDS),
        )
    }

    pub fn with_endpoint(endpoint: &str, timeout: Duration) -> Result<Self, GbifMatchError> {
        let endpoint = endpoint.trim().to_string();
        let parsed = reqwest::Url::parse(&endpoint)
            .map_err(|_| GbifMatchError::InvalidConfiguration)?;
        if endpoint.is_empty()
            || timeout.is_zero()
            || !matches!(parsed.scheme(), "http" | "https")
        {
            return Err(GbifMatchError::InvalidConfiguration);
        }

        let http = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| GbifMatchError::InvalidConfiguration)?;

        Ok(Self { http, endpoint })
    }

    /// Resolve a scientific name to one trusted GBIF taxon.
    ///
    /// Exact matches are accepted. Fuzzy matches are accepted only when GBIF
    /// reports high confidence. HIGHERRANK is deliberately rejected. When GBIF
    /// resolves a synonym to an accepted usage, both the accepted key and the
    /// accepted full scientific name are preferred so the UI displays the same
    /// taxon that dwciri:toTaxon points to.
    pub async fn match_taxon(
        &self,
        scientific_name: &str,
    ) -> Result<Option<GbifTaxonMatch>, GbifMatchError> {
        let scientific_name = scientific_name.trim();
        if scientific_name.is_empty() {
            return Ok(None);
        }

        let mut url = reqwest::Url::parse(&self.endpoint)
            .map_err(|_| GbifMatchError::InvalidConfiguration)?;
        url.query_pairs_mut().append_pair("name", scientific_name);

        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| GbifMatchError::RequestFailed)?;

        if !response.status().is_success() {
            return Err(GbifMatchError::Upstream(response.status()));
        }

        let matched: GbifSpeciesMatchResponse = response
            .json()
            .await
            .map_err(|_| GbifMatchError::InvalidResponse)?;

        Ok(taxon_match_from_response(&matched))
    }

    pub async fn match_to_taxon(
        &self,
        scientific_name: &str,
    ) -> Result<Option<String>, GbifMatchError> {
        Ok(self
            .match_taxon(scientific_name)
            .await?
            .map(|matched| matched.taxon_uri))
    }
}

fn taxon_match_from_response(matched: &GbifSpeciesMatchResponse) -> Option<GbifTaxonMatch> {
    let match_type = matched
        .match_type
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let accepted = match_type == "EXACT"
        || (match_type == "FUZZY"
            && matched.confidence.unwrap_or(0) >= MIN_FUZZY_CONFIDENCE);
    if !accepted {
        return None;
    }

    let key = matched.accepted_usage_key.or(matched.usage_key)?;
    let scientific_name = matched
        .accepted_scientific_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            matched
                .scientific_name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        })?
        .trim()
        .to_string();

    Some(GbifTaxonMatch {
        taxon_uri: format!("{GBIF_TAXON_URI_BASE}{key}"),
        scientific_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_returns_uri_and_full_scientific_name() {
        let matched = GbifSpeciesMatchResponse {
            usage_key: Some(5231190),
            accepted_usage_key: None,
            scientific_name: Some("Passer domesticus (Linnaeus, 1758)".to_string()),
            accepted_scientific_name: None,
            match_type: Some("EXACT".to_string()),
            confidence: Some(98),
        };

        assert_eq!(
            taxon_match_from_response(&matched),
            Some(GbifTaxonMatch {
                taxon_uri: "https://www.gbif.org/species/5231190".to_string(),
                scientific_name: "Passer domesticus (Linnaeus, 1758)".to_string(),
            })
        );
    }

    #[test]
    fn synonym_prefers_accepted_usage_and_name() {
        let matched = GbifSpeciesMatchResponse {
            usage_key: Some(2468551),
            accepted_usage_key: Some(9702100),
            scientific_name: Some("Old name Author, 1900".to_string()),
            accepted_scientific_name: Some("Accepted name Author, 1900".to_string()),
            match_type: Some("EXACT".to_string()),
            confidence: Some(97),
        };

        assert_eq!(
            taxon_match_from_response(&matched),
            Some(GbifTaxonMatch {
                taxon_uri: "https://www.gbif.org/species/9702100".to_string(),
                scientific_name: "Accepted name Author, 1900".to_string(),
            })
        );
    }

    #[test]
    fn weak_fuzzy_and_higher_rank_matches_are_rejected() {
        let weak_fuzzy = GbifSpeciesMatchResponse {
            usage_key: Some(1),
            accepted_usage_key: None,
            scientific_name: Some("Name Author".to_string()),
            accepted_scientific_name: None,
            match_type: Some("FUZZY".to_string()),
            confidence: Some(73),
        };
        let higher_rank = GbifSpeciesMatchResponse {
            usage_key: Some(2),
            accepted_usage_key: None,
            scientific_name: Some("Genus Author".to_string()),
            accepted_scientific_name: None,
            match_type: Some("HIGHERRANK".to_string()),
            confidence: Some(100),
        };

        assert_eq!(taxon_match_from_response(&weak_fuzzy), None);
        assert_eq!(taxon_match_from_response(&higher_rank), None);
    }
}

use crate::config::FusekiConfig;
use crate::features::occurrences::service::{
    DarwinCoreTerm, OccurrenceRdfStore, OccurrenceServiceError, PredicateObjectMapping,
    SearchOccurrencesStoreInput, SearchOccurrencesStorePage,
};

// 既存Fuseki実装を低レベルstoreとして利用し、Darwin Core候補取得だけ
// Bio-Database固有のoccurrence-profile graphを適用する。
#[path = "fuseki.rs"]
mod base;

pub use base::FusekiClientError;

const DARWIN_CORE_VOCABULARY_GRAPH_URI: &str =
    "https://bio-database.net/graphs/vocabularies/darwin-core";
const OCCURRENCE_PROFILE_GRAPH_URI: &str =
    "https://bio-database.net/graphs/app/occurrence-profile";
const LOCAL_NAME_PREDICATE_URI: &str = "https://bio-database.net/terms/localName";
const USE_AT_BIO_DATABASE_PREDICATE_URI: &str =
    "https://bio-database.net/terms/useAtBioDatabase";

#[derive(Clone)]
pub struct FusekiClient {
    inner: base::FusekiClient,
    http: reqwest::Client,
    config: FusekiConfig,
}

impl FusekiClient {
    pub fn new(config: FusekiConfig) -> Self {
        Self {
            inner: base::FusekiClient::new(config.clone()),
            http: reqwest::Client::new(),
            config,
        }
    }

    pub async fn post_nquads(&self, nquads: Vec<u8>) -> Result<(), FusekiClientError> {
        self.inner.post_nquads(nquads).await
    }
}

fn build_list_darwin_core_terms_query() -> String {
    format!(
        r#"
        SELECT DISTINCT ?term ?localName
        WHERE {{
            GRAPH <{DARWIN_CORE_VOCABULARY_GRAPH_URI}> {{
                ?term <{LOCAL_NAME_PREDICATE_URI}> ?localName .
                FILTER(isIRI(?term))
            }}
            GRAPH <{OCCURRENCE_PROFILE_GRAPH_URI}> {{
                ?term <{USE_AT_BIO_DATABASE_PREDICATE_URI}> true .
            }}
        }}
        ORDER BY LCASE(STR(?localName)) STR(?term)
        "#
    )
}

#[async_trait::async_trait]
impl OccurrenceRdfStore for FusekiClient {
    async fn save_nquads(&self, nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
        OccurrenceRdfStore::save_nquads(&self.inner, nquads).await
    }

    async fn get_occurrence_nquads(
        &self,
        occurrence_uri: &str,
    ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
        OccurrenceRdfStore::get_occurrence_nquads(&self.inner, occurrence_uri).await
    }

    async fn is_media_referenced_by_public_occurrence(
        &self,
        media_uri: &str,
    ) -> Result<bool, OccurrenceServiceError> {
        OccurrenceRdfStore::is_media_referenced_by_public_occurrence(&self.inner, media_uri).await
    }

    async fn is_media_referenced_by_occurrence(
        &self,
        media_uri: &str,
    ) -> Result<bool, OccurrenceServiceError> {
        OccurrenceRdfStore::is_media_referenced_by_occurrence(&self.inner, media_uri).await
    }

    async fn replace_occurrence_nquads(
        &self,
        occurrence_uri: &str,
        nquads: Vec<u8>,
    ) -> Result<(), OccurrenceServiceError> {
        OccurrenceRdfStore::replace_occurrence_nquads(&self.inner, occurrence_uri, nquads).await
    }

    async fn delete_occurrence_nquads(
        &self,
        occurrence_uri: &str,
    ) -> Result<(), OccurrenceServiceError> {
        OccurrenceRdfStore::delete_occurrence_nquads(&self.inner, occurrence_uri).await
    }

    async fn list_darwin_core_terms(&self) -> Result<Vec<DarwinCoreTerm>, OccurrenceServiceError> {
        let query = build_list_darwin_core_terms_query();
        let response = self
            .http
            .post(self.config.sparql_url())
            .basic_auth(&self.config.user, Some(&self.config.password))
            .header(reqwest::header::CONTENT_TYPE, "application/sparql-query")
            .header(reqwest::header::ACCEPT, "application/sparql-results+json")
            .body(query)
            .send()
            .await
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;

        if !response.status().is_success() {
            return Err(OccurrenceServiceError::StoreFailed);
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;
        let bindings = body["results"]["bindings"]
            .as_array()
            .ok_or(OccurrenceServiceError::StoreFailed)?;

        bindings
            .iter()
            .map(|binding| {
                Ok(DarwinCoreTerm {
                    uri: binding_value(binding, "term")
                        .ok_or(OccurrenceServiceError::StoreFailed)?,
                    local_name: binding_value(binding, "localName")
                        .ok_or(OccurrenceServiceError::StoreFailed)?,
                })
            })
            .collect()
    }

    async fn predicate_object_mapping(
        &self,
        predicate_uri: &str,
    ) -> Result<Option<PredicateObjectMapping>, OccurrenceServiceError> {
        OccurrenceRdfStore::predicate_object_mapping(&self.inner, predicate_uri).await
    }

    async fn search_occurrences(
        &self,
        input: SearchOccurrencesStoreInput,
    ) -> Result<SearchOccurrencesStorePage, OccurrenceServiceError> {
        OccurrenceRdfStore::search_occurrences(&self.inner, input).await
    }
}

fn binding_value(binding: &serde_json::Value, name: &str) -> Option<String> {
    binding
        .get(name)?
        .get("value")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darwin_core_term_query_joins_occurrence_profile_and_requires_enabled_terms() {
        let query = build_list_darwin_core_terms_query();

        assert!(query.contains(&format!(
            "GRAPH <{DARWIN_CORE_VOCABULARY_GRAPH_URI}>"
        )));
        assert!(query.contains(&format!("GRAPH <{OCCURRENCE_PROFILE_GRAPH_URI}>")));
        assert!(query.contains(&format!(
            "?term <{USE_AT_BIO_DATABASE_PREDICATE_URI}> true ."
        )));
    }
}

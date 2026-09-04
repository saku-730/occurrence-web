use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

use crate::{
    config::FusekiConfig,
    features::{
        occurrences::service::{
            DarwinCoreTerm, OccurrenceRdfStore, OccurrenceServiceError, PredicateObjectMapping,
            SearchOccurrenceFilterInput, SearchOccurrencesStoreInput, SearchOccurrencesStorePage,
        },
        paper_import::gbif::GbifClient,
    },
};

#[path = "fuseki_profiled.rs"]
mod profiled;

pub use profiled::FusekiClientError;

const DWCIRI_TO_TAXON_PREDICATE_URI: &str = "http://rs.tdwg.org/dwc/iri/toTaxon";

#[derive(Clone)]
pub struct FusekiClient {
    inner: profiled::FusekiClient,
    gbif: Option<GbifClient>,
    gbif_search_cache: Arc<RwLock<HashMap<String, Option<String>>>>,
}

impl FusekiClient {
    pub fn new(config: FusekiConfig) -> Self {
        Self {
            inner: profiled::FusekiClient::new(config),
            gbif: GbifClient::new().ok(),
            gbif_search_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn post_nquads(&self, nquads: Vec<u8>) -> Result<(), FusekiClientError> {
        self.inner.post_nquads(nquads).await
    }

    async fn resolve_to_taxon_name(
        &self,
        scientific_name: &str,
    ) -> Result<Option<String>, OccurrenceServiceError> {
        let scientific_name = scientific_name.trim();
        if scientific_name.is_empty() {
            return Ok(None);
        }

        let cache_key = scientific_name.to_lowercase();
        if let Some(cached) = self.gbif_search_cache.read().await.get(&cache_key).cloned() {
            return Ok(cached);
        }

        let gbif = self
            .gbif
            .as_ref()
            .ok_or(OccurrenceServiceError::StoreFailed)?;
        let resolved = gbif
            .match_to_taxon(scientific_name)
            .await
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;

        self.gbif_search_cache
            .write()
            .await
            .insert(cache_key, resolved.clone());

        Ok(resolved)
    }

    async fn resolve_search_filters(
        &self,
        filters: Vec<SearchOccurrenceFilterInput>,
    ) -> Result<Option<Vec<SearchOccurrenceFilterInput>>, OccurrenceServiceError> {
        let mut resolved_filters = Vec::with_capacity(filters.len());

        for mut filter in filters {
            if filter.predicate == DWCIRI_TO_TAXON_PREDICATE_URI
                && filter.value_type == "literal"
            {
                let Some(taxon_uri) = self.resolve_to_taxon_name(&filter.value).await? else {
                    return Ok(None);
                };
                filter.value = taxon_uri;
                filter.value_type = "uri".to_string();
            }

            resolved_filters.push(filter);
        }

        Ok(Some(resolved_filters))
    }
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
        OccurrenceRdfStore::list_darwin_core_terms(&self.inner).await
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
        let SearchOccurrencesStoreInput {
            filters,
            limit,
            cursor,
            visibility,
        } = input;
        let limit = limit.max(1);

        let Some(filters) = self.resolve_search_filters(filters).await? else {
            return Ok(SearchOccurrencesStorePage {
                rows: Vec::new(),
                limit,
                next_cursor: None,
                has_next: false,
            });
        };

        OccurrenceRdfStore::search_occurrences(
            &self.inner,
            SearchOccurrencesStoreInput {
                filters,
                limit,
                cursor,
                visibility,
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_literal_to_taxon_requires_gbif_resolution() {
        let to_taxon_literal = SearchOccurrenceFilterInput {
            predicate: DWCIRI_TO_TAXON_PREDICATE_URI.to_string(),
            value: "Annelida".to_string(),
            value_type: "literal".to_string(),
            match_type: "exact".to_string(),
        };
        assert!(
            to_taxon_literal.predicate == DWCIRI_TO_TAXON_PREDICATE_URI
                && to_taxon_literal.value_type == "literal"
        );

        let to_taxon_uri = SearchOccurrenceFilterInput {
            value: "https://www.gbif.org/species/42".to_string(),
            value_type: "uri".to_string(),
            ..to_taxon_literal.clone()
        };
        assert!(!(to_taxon_uri.predicate == DWCIRI_TO_TAXON_PREDICATE_URI
            && to_taxon_uri.value_type == "literal"));

        let scientific_name = SearchOccurrenceFilterInput {
            predicate: "http://rs.tdwg.org/dwc/terms/scientificName".to_string(),
            value: "Annelida".to_string(),
            value_type: "literal".to_string(),
            match_type: "exact".to_string(),
        };
        assert!(!(scientific_name.predicate == DWCIRI_TO_TAXON_PREDICATE_URI
            && scientific_name.value_type == "literal"));
    }
}

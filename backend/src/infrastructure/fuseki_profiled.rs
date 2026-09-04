use crate::config::FusekiConfig;
use crate::features::occurrences::service::{
    DarwinCoreTerm, HAS_EVENT_PREDICATE_URI, HAS_IDENTIFICATION_PREDICATE_URI,
    HAS_LOCATION_PREDICATE_URI, OccurrenceRdfStore, OccurrenceServiceError, OccurrenceTarget,
    PredicateObjectMapping, SearchOccurrenceFilterInput, SearchOccurrenceStoreRow,
    SearchOccurrencesStoreInput, SearchOccurrencesStorePage, SearchVisibility,
};

// 既存Fuseki実装を低レベルstoreとして利用し、Darwin Core候補取得と
// Bio-Database固有のGBIF階層検索だけこのwrapperで上書きする。
#[path = "fuseki.rs"]
mod base;

pub use base::FusekiClientError;

const DARWIN_CORE_VOCABULARY_GRAPH_URI: &str =
    "https://bio-database.net/graphs/vocabularies/darwin-core";
const OCCURRENCE_PROFILE_GRAPH_URI: &str = "https://bio-database.net/graphs/app/occurrence-profile";
const LOCAL_NAME_PREDICATE_URI: &str = "https://bio-database.net/terms/localName";
const USE_AT_BIO_DATABASE_PREDICATE_URI: &str = "https://bio-database.net/terms/useAtBioDatabase";
const SKOS_PREF_LABEL_PREDICATE_URI: &str = "http://www.w3.org/2004/02/skos/core#prefLabel";

const OCCURRENCE_GRAPH_URI: &str = "https://bio-database.net/graphs/occurrences";
const OCCURRENCE_URI_BASE: &str = "https://bio-database.net/occurrences/";
const DWCIRI_TO_TAXON_PREDICATE_URI: &str = "http://rs.tdwg.org/dwc/iri/toTaxon";
const GBIF_BACKBONE_TAXONOMY_GRAPH_URI: &str =
    "https://bio-database.net/graphs/taxonomy/gbif-backbone";
const GBIF_PUBLIC_TAXON_URI_BASE: &str = "https://www.gbif.org/species/";
const GBIF_INTERNAL_TAXON_URI_BASE: &str = "https://bio-database.net/taxa/gbif/";
const GBIF_PARENT_NAME_USAGE_PREDICATE_URI: &str =
    "https://bio-database.net/terms/parentNameUsage";

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
        SELECT DISTINCT ?term ?localName ?labelJa
        WHERE {{
            GRAPH <{DARWIN_CORE_VOCABULARY_GRAPH_URI}> {{
                ?term <{LOCAL_NAME_PREDICATE_URI}> ?localName .
                FILTER(isIRI(?term))
            }}
            GRAPH <{OCCURRENCE_PROFILE_GRAPH_URI}> {{
                ?term <{USE_AT_BIO_DATABASE_PREDICATE_URI}> true .
                OPTIONAL {{
                    ?term <{SKOS_PREF_LABEL_PREDICATE_URI}> ?labelJa .
                    FILTER(LANG(?labelJa) = "ja")
                }}
            }}
        }}
        ORDER BY LCASE(COALESCE(STR(?labelJa), STR(?localName))) STR(?term)
        "#
    )
}

fn intermediate_link_values() -> String {
    format!(
        "<{HAS_IDENTIFICATION_PREDICATE_URI}> <{HAS_EVENT_PREDICATE_URI}> <{HAS_LOCATION_PREDICATE_URI}>"
    )
}

fn build_search_occurrences_query(
    filters: &[SearchOccurrenceFilterInput],
    visibility: &SearchVisibility,
    cursor: Option<&str>,
    limit: u32,
) -> Result<String, OccurrenceServiceError> {
    let scientific_name_predicate = "http://rs.tdwg.org/dwc/terms/scientificName";
    let basis_of_record_predicate = "http://rs.tdwg.org/dwc/terms/basisOfRecord";
    let recorded_by_predicate = "http://rs.tdwg.org/dwc/terms/recordedBy";
    let created_predicate = "http://purl.org/dc/terms/created";
    let modified_predicate = "http://purl.org/dc/terms/modified";
    let filter_patterns = build_search_filter_patterns(filters)?;
    let visibility_patterns = build_search_visibility_patterns(visibility)?;
    let cursor_filter = build_search_cursor_filter(cursor)?;
    let query_limit = limit.max(1) + 1;

    Ok(format!(
        r#"
        SELECT DISTINCT ?occurrence ?scientificName ?basisOfRecord ?recordedBy ?created ?modified ?accessRights ?creator
        WHERE {{
          GRAPH <{OCCURRENCE_GRAPH_URI}> {{
            ?occurrence <{created_predicate}> ?created .
            FILTER(STRSTARTS(STR(?occurrence), "{OCCURRENCE_URI_BASE}"))
            {filter_patterns}
            {visibility_patterns}
            {cursor_filter}
            OPTIONAL {{
              {{ ?occurrence <{scientific_name_predicate}> ?scientificName . }}
              UNION
              {{
                ?occurrence <{HAS_IDENTIFICATION_PREDICATE_URI}> ?identification .
                ?identification <{scientific_name_predicate}> ?scientificName .
              }}
            }}
            OPTIONAL {{ ?occurrence <{basis_of_record_predicate}> ?basisOfRecord . }}
            OPTIONAL {{
              {{ ?occurrence <{recorded_by_predicate}> ?recordedBy . }}
              UNION
              {{
                ?occurrence <{HAS_EVENT_PREDICATE_URI}> ?event .
                ?event <{recorded_by_predicate}> ?recordedBy .
              }}
            }}
            OPTIONAL {{ ?occurrence <{modified_predicate}> ?modified . }}
          }}
        }}
        ORDER BY DESC(?created) DESC(?occurrence)
        LIMIT {query_limit}
        "#
    ))
}

fn build_search_visibility_patterns(
    visibility: &SearchVisibility,
) -> Result<String, OccurrenceServiceError> {
    let access_rights_predicate = "http://purl.org/dc/terms/accessRights";
    let creator_predicate = "http://purl.org/dc/terms/creator";
    let private_access_rights_uri = "https://bio-database.net/terms/access-rights/private";

    let patterns = match visibility {
        SearchVisibility::All => format!(
            r#"OPTIONAL {{ ?occurrence <{access_rights_predicate}> ?accessRights . }}
                OPTIONAL {{ ?occurrence <{creator_predicate}> ?creator . }}"#
        ),
        SearchVisibility::PublicOnly => format!(
            r#"OPTIONAL {{ ?occurrence <{access_rights_predicate}> ?accessRights . }}
                OPTIONAL {{ ?occurrence <{creator_predicate}> ?creator . }}
                FILTER(!BOUND(?accessRights) || ?accessRights != <{private_access_rights_uri}>)"#
        ),
        SearchVisibility::PublicOrOwnPrivate { user_id } => {
            let user_uri = format!("https://bio-database.net/users/{user_id}");
            let user_uri = escape_sparql_iri(&user_uri)?;

            format!(
                r#"OPTIONAL {{ ?occurrence <{access_rights_predicate}> ?accessRights . }}
                OPTIONAL {{ ?occurrence <{creator_predicate}> ?creator . }}
                FILTER(!BOUND(?accessRights) || ?accessRights != <{private_access_rights_uri}> || ?creator = <{user_uri}>)"#
            )
        }
    };

    Ok(patterns)
}

fn build_search_filter_patterns(
    filters: &[SearchOccurrenceFilterInput],
) -> Result<String, OccurrenceServiceError> {
    let mut patterns = Vec::new();

    for (index, filter) in filters.iter().enumerate() {
        if filter.match_type != "exact" {
            return Err(OccurrenceServiceError::StoreFailed);
        }

        let predicate = escape_sparql_iri(&filter.predicate)?;
        let object_var = format!("?filterObject{index}");
        let predicate_pattern = match OccurrenceTarget::for_predicate(&filter.predicate)
            .intermediate_definition()
        {
            None if filter
                .predicate
                .starts_with("http://rs.tdwg.org/dwc/terms/")
                || filter.predicate.starts_with("http://rs.tdwg.org/dwc/iri/") =>
            {
                let target_var = format!("?filterTarget{index}");
                let link_var = format!("?filterLink{index}");
                let links = intermediate_link_values();
                format!(
                    "{{ ?occurrence <{predicate}> {object_var} . }} UNION {{ VALUES {link_var} {{ {links} }} ?occurrence {link_var} {target_var} . {target_var} <{predicate}> {object_var} . }}"
                )
            }
            None => format!("?occurrence <{predicate}> {object_var} ."),
            Some((_, link_predicate_uri, _)) => {
                let target_var = format!("?filterTarget{index}");
                format!(
                    "{{ ?occurrence <{predicate}> {object_var} . }} UNION {{ ?occurrence <{link_predicate_uri}> {target_var} . {target_var} <{predicate}> {object_var} . }}"
                )
            }
        };

        match filter.value_type.as_str() {
            "literal" => {
                let value = escape_sparql_literal(&filter.value.trim().to_lowercase());
                patterns.push(format!(
                    "{predicate_pattern} FILTER(isLiteral({object_var}) && LCASE(STR({object_var})) = \"{value}\")"
                ));
            }
            "uri" => {
                let escaped_value = escape_sparql_iri(&filter.value)?;
                let object = format!("<{escaped_value}>");

                if filter.predicate == DWCIRI_TO_TAXON_PREDICATE_URI {
                    if let Some(gbif_key) = gbif_key_from_public_taxon_uri(&filter.value) {
                        let internal_target = format!("{GBIF_INTERNAL_TAXON_URI_BASE}{gbif_key}");
                        let internal_taxon_var = format!("?filterInternalTaxon{index}");

                        patterns.push(format!(
                            r#"{{
                        {{ {predicate_pattern} FILTER({object_var} = {object}) }}
                        UNION
                        {{
                            {predicate_pattern}
                            FILTER(STRSTARTS(STR({object_var}), "{GBIF_PUBLIC_TAXON_URI_BASE}"))
                            BIND(
                                IRI(CONCAT(
                                    "{GBIF_INTERNAL_TAXON_URI_BASE}",
                                    STRAFTER(STR({object_var}), "{GBIF_PUBLIC_TAXON_URI_BASE}")
                                ))
                                AS {internal_taxon_var}
                            )
                            GRAPH <{GBIF_BACKBONE_TAXONOMY_GRAPH_URI}> {{
                                {internal_taxon_var} <{GBIF_PARENT_NAME_USAGE_PREDICATE_URI}>+ <{internal_target}> .
                            }}
                        }}
                    }}"#
                        ));
                    } else {
                        patterns.push(format!("{predicate_pattern} FILTER({object_var} = {object})"));
                    }
                } else {
                    patterns.push(format!("{predicate_pattern} FILTER({object_var} = {object})"));
                }
            }
            _ => return Err(OccurrenceServiceError::StoreFailed),
        }
    }

    Ok(patterns.join("\n"))
}

fn gbif_key_from_public_taxon_uri(value: &str) -> Option<&str> {
    let key = value.strip_prefix(GBIF_PUBLIC_TAXON_URI_BASE)?;
    if key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(key)
}

fn escape_sparql_iri(value: &str) -> Result<String, OccurrenceServiceError> {
    if value.contains(['<', '>', '"', '{', '}', '|', '^', '`', '\\']) {
        return Err(OccurrenceServiceError::StoreFailed);
    }
    Ok(value.to_string())
}

fn escape_sparql_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn build_search_cursor_filter(cursor: Option<&str>) -> Result<String, OccurrenceServiceError> {
    let Some(cursor) = cursor else {
        return Ok(String::new());
    };

    let bytes = hex::decode(cursor).map_err(|_| OccurrenceServiceError::StoreFailed)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| OccurrenceServiceError::StoreFailed)?;
    let created = value["created"]
        .as_str()
        .ok_or(OccurrenceServiceError::StoreFailed)?;
    let occurrence_uri = value["occurrence_uri"]
        .as_str()
        .ok_or(OccurrenceServiceError::StoreFailed)?;
    let created_literal = escape_sparql_literal(created);
    let occurrence_uri = escape_sparql_literal(occurrence_uri);

    Ok(format!(
        r#"FILTER(
                  ?created < "{created_literal}"^^<http://www.w3.org/2001/XMLSchema#dateTime>
                  || (
                    ?created = "{created_literal}"^^<http://www.w3.org/2001/XMLSchema#dateTime>
                    && STR(?occurrence) < "{occurrence_uri}"
                  )
                )"#
    ))
}

fn search_next_cursor(row: &SearchOccurrenceStoreRow) -> String {
    let cursor = serde_json::json!({
        "created": row.created.as_deref().unwrap_or(""),
        "occurrence_uri": row.occurrence_uri,
    });
    hex::encode(cursor.to_string())
}

fn access_rights_label(value: &str) -> String {
    value
        .strip_prefix("https://bio-database.net/terms/access-rights/")
        .unwrap_or(value)
        .to_string()
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
                    local_name: display_name_from_binding(binding)
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
        let limit = input.limit.max(1);
        let query = build_search_occurrences_query(
            &input.filters,
            &input.visibility,
            input.cursor.as_deref(),
            limit,
        )?;

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

        let has_next = bindings.len() > limit as usize;
        let mut rows = Vec::new();

        for binding in bindings.iter().take(limit as usize) {
            let occurrence_uri =
                binding_value(binding, "occurrence").ok_or(OccurrenceServiceError::StoreFailed)?;
            let occurrence_id = occurrence_uri
                .strip_prefix(OCCURRENCE_URI_BASE)
                .and_then(|id| uuid::Uuid::parse_str(id).ok())
                .ok_or(OccurrenceServiceError::StoreFailed)?;
            let creator_user_id = binding_value(binding, "creator")
                .and_then(|creator_uri| {
                    creator_uri
                        .strip_prefix("https://bio-database.net/users/")
                        .map(str::to_string)
                })
                .and_then(|user_id| uuid::Uuid::parse_str(&user_id).ok());

            rows.push(SearchOccurrenceStoreRow {
                occurrence_id,
                occurrence_uri,
                creator_user_id,
                scientific_name: binding_value(binding, "scientificName"),
                basis_of_record: binding_value(binding, "basisOfRecord"),
                recorded_by: binding_value(binding, "recordedBy"),
                created: binding_value(binding, "created"),
                modified: binding_value(binding, "modified"),
                access_rights: binding_value(binding, "accessRights")
                    .map(|value| access_rights_label(&value)),
            });
        }

        let next_cursor = if has_next {
            rows.last().map(search_next_cursor)
        } else {
            None
        };

        Ok(SearchOccurrencesStorePage {
            rows,
            limit,
            next_cursor,
            has_next,
        })
    }
}

fn display_name_from_binding(binding: &serde_json::Value) -> Option<String> {
    binding_value(binding, "labelJa").or_else(|| binding_value(binding, "localName"))
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
    fn darwin_core_term_query_joins_profile_requires_enabled_and_reads_japanese_label() {
        let query = build_list_darwin_core_terms_query();

        assert!(query.contains(&format!("GRAPH <{DARWIN_CORE_VOCABULARY_GRAPH_URI}>")));
        assert!(query.contains(&format!("GRAPH <{OCCURRENCE_PROFILE_GRAPH_URI}>")));
        assert!(query.contains(&format!(
            "?term <{USE_AT_BIO_DATABASE_PREDICATE_URI}> true ."
        )));
        assert!(query.contains(&format!(
            "?term <{SKOS_PREF_LABEL_PREDICATE_URI}> ?labelJa ."
        )));
        assert!(query.contains("FILTER(LANG(?labelJa) = \"ja\")"));
    }

    #[test]
    fn japanese_label_is_preferred_and_local_name_is_fallback() {
        let with_japanese = serde_json::json!({
            "localName": { "value": "scientificName" },
            "labelJa": { "value": "学名" }
        });
        assert_eq!(
            display_name_from_binding(&with_japanese).as_deref(),
            Some("学名")
        );

        let without_japanese = serde_json::json!({
            "localName": { "value": "scientificName" }
        });
        assert_eq!(
            display_name_from_binding(&without_japanese).as_deref(),
            Some("scientificName")
        );
    }

    #[test]
    fn to_taxon_uri_filter_uses_gbif_parent_hierarchy() {
        let patterns = build_search_filter_patterns(&[SearchOccurrenceFilterInput {
            predicate: DWCIRI_TO_TAXON_PREDICATE_URI.to_string(),
            value: "https://www.gbif.org/species/42".to_string(),
            value_type: "uri".to_string(),
            match_type: "exact".to_string(),
        }])
        .expect("toTaxon hierarchy filter should build");

        assert!(patterns.contains(HAS_IDENTIFICATION_PREDICATE_URI));
        assert!(patterns.contains("<https://www.gbif.org/species/42>"));
        assert!(patterns.contains("<https://bio-database.net/taxa/gbif/42>"));
        assert!(patterns.contains(GBIF_BACKBONE_TAXONOMY_GRAPH_URI));
        assert!(patterns.contains(GBIF_PARENT_NAME_USAGE_PREDICATE_URI));
        assert!(patterns.contains("STRAFTER"));
        assert!(patterns.contains("IRI(CONCAT"));
        assert!(!patterns.contains("http://www.w3.org/2000/01/rdf-schema#subClassOf"));
    }

    #[test]
    fn non_to_taxon_uri_filter_is_exact_only() {
        let creator_predicate = "http://purl.org/dc/terms/creator";
        let creator_uri = "https://bio-database.net/users/550e8400-e29b-41d4-a716-446655440000";
        let patterns = build_search_filter_patterns(&[SearchOccurrenceFilterInput {
            predicate: creator_predicate.to_string(),
            value: creator_uri.to_string(),
            value_type: "uri".to_string(),
            match_type: "exact".to_string(),
        }])
        .expect("ordinary URI filter should build");

        assert!(patterns.contains(&format!("FILTER(?filterObject0 = <{creator_uri}>)")));
        assert!(!patterns.contains(GBIF_BACKBONE_TAXONOMY_GRAPH_URI));
        assert!(!patterns.contains(GBIF_PARENT_NAME_USAGE_PREDICATE_URI));
        assert!(!patterns.contains("STRAFTER"));
    }

    #[test]
    fn non_gbif_to_taxon_uri_filter_is_exact_only() {
        let taxon_uri = "https://example.org/taxa/annelida";
        let patterns = build_search_filter_patterns(&[SearchOccurrenceFilterInput {
            predicate: DWCIRI_TO_TAXON_PREDICATE_URI.to_string(),
            value: taxon_uri.to_string(),
            value_type: "uri".to_string(),
            match_type: "exact".to_string(),
        }])
        .expect("non-GBIF toTaxon URI filter should build");

        assert!(patterns.contains(&format!("FILTER(?filterObject0 = <{taxon_uri}>)")));
        assert!(!patterns.contains(GBIF_BACKBONE_TAXONOMY_GRAPH_URI));
        assert!(!patterns.contains(GBIF_PARENT_NAME_USAGE_PREDICATE_URI));
    }

    #[test]
    fn gbif_key_parser_accepts_only_numeric_species_keys() {
        assert_eq!(
            gbif_key_from_public_taxon_uri("https://www.gbif.org/species/42"),
            Some("42")
        );
        assert_eq!(
            gbif_key_from_public_taxon_uri("https://www.gbif.org/species/9782253"),
            Some("9782253")
        );
        assert_eq!(
            gbif_key_from_public_taxon_uri("https://www.gbif.org/species/"),
            None
        );
        assert_eq!(
            gbif_key_from_public_taxon_uri("https://www.gbif.org/species/42?x=1"),
            None
        );
        assert_eq!(
            gbif_key_from_public_taxon_uri("https://example.org/species/42"),
            None
        );
    }
}

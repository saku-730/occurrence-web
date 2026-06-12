use crate::config::FusekiConfig;
use crate::features::occurrences::service::{
    OccurrenceRdfStore, OccurrenceServiceError, SearchOccurrenceFilterInput,
    SearchOccurrenceStoreRow, SearchOccurrencesStoreInput, SearchOccurrencesStorePage,
};

#[derive(Clone)]
pub struct FusekiClient {
    http: reqwest::Client,
    config: FusekiConfig,
}

#[derive(Debug)]
pub enum FusekiClientError {
    RequestFailed(reqwest::Error),
    UnexpectedStatus(reqwest::StatusCode),
}

impl FusekiClient {
    pub fn new(config: FusekiConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    pub async fn post_nquads(&self, nquads: Vec<u8>) -> Result<(), FusekiClientError> {
        let response = self
            .http
            .post(self.config.data_url())
            .basic_auth(
                self.config.user.as_str(),
                Some(self.config.password.as_str()),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/n-quads")
            .body(nquads)
            .send()
            .await
            .map_err(FusekiClientError::RequestFailed)?;

        if !response.status().is_success() {
            return Err(FusekiClientError::UnexpectedStatus(response.status()));
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl OccurrenceRdfStore for FusekiClient {
    async fn save_nquads(&self, nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
        self.post_nquads(nquads)
            .await
            .map_err(|_| OccurrenceServiceError::StoreFailed)
    }

    async fn search_occurrences(
        &self,
        input: SearchOccurrencesStoreInput,
    ) -> Result<SearchOccurrencesStorePage, OccurrenceServiceError> {
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let occurrence_uri_base = "https://bio-database.net/occurrences/";
        let scientific_name_predicate = "http://rs.tdwg.org/dwc/terms/scientificName";
        let basis_of_record_predicate = "http://rs.tdwg.org/dwc/terms/basisOfRecord";
        let recorded_by_predicate = "http://rs.tdwg.org/dwc/terms/recordedBy";
        let created_predicate = "http://purl.org/dc/terms/created";
        let modified_predicate = "http://purl.org/dc/terms/modified";
        let access_rights_predicate = "http://purl.org/dc/terms/accessRights";
        let creator_predicate = "http://purl.org/dc/terms/creator";

        let filter_patterns = build_search_filter_patterns(&input.filters)?;
        let cursor_filter = build_search_cursor_filter(input.cursor.as_deref())?;
        let limit = input.limit.max(1);
        let query_limit = limit + 1;

        let query = format!(
            r#"
            SELECT DISTINCT ?occurrence ?scientificName ?basisOfRecord ?recordedBy ?created ?modified ?accessRights ?creator
            WHERE {{
              GRAPH <{graph_uri}> {{
                ?occurrence ?p ?o .
                FILTER(STRSTARTS(STR(?occurrence), "{occurrence_uri_base}"))
                {filter_patterns}
                {cursor_filter}
                OPTIONAL {{ ?occurrence <{scientific_name_predicate}> ?scientificName . }}
                OPTIONAL {{ ?occurrence <{basis_of_record_predicate}> ?basisOfRecord . }}
                OPTIONAL {{ ?occurrence <{recorded_by_predicate}> ?recordedBy . }}
                OPTIONAL {{ ?occurrence <{created_predicate}> ?created . }}
                OPTIONAL {{ ?occurrence <{modified_predicate}> ?modified . }}
                OPTIONAL {{ ?occurrence <{access_rights_predicate}> ?accessRights . }}
                OPTIONAL {{ ?occurrence <{creator_predicate}> ?creator . }}
              }}
            }}
            ORDER BY DESC(?created) DESC(?occurrence)
            LIMIT {query_limit}
            "#
        );

        let sparql_url = format!("{}/sparql", self.config.base_url.trim_end_matches('/'));

        let response = self
            .http
            .post(sparql_url)
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
        let mut rows = Vec::new(); //検索結果入れる

        for binding in bindings.iter().take(limit as usize) {
            let occurrence_uri = binding_value(binding, "occurrence")
                .ok_or(OccurrenceServiceError::StoreFailed)?;
            let occurrence_id = occurrence_uri
                .strip_prefix(occurrence_uri_base)
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

    async fn get_occurrence_nquads(
        &self,
        occurrence_uri: &str,
    ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
        use oxrdf::{GraphName, NamedNode, Quad};
        use oxrdfio::{RdfFormat, RdfParser, RdfSerializer};

        let graph_uri = "https://bio-database.net/graphs/occurrences";

        let query = format!(
            r#"
            CONSTRUCT {{
              <{occurrence_uri}> ?p ?o .
            }}
            WHERE {{
              GRAPH <{graph_uri}> {{
                <{occurrence_uri}> ?p ?o .
              }}
            }}
            "#
        );

        let sparql_url = format!("{}/sparql", self.config.base_url.trim_end_matches('/'));

        let response = self
            .http
            .post(sparql_url)
            .basic_auth(&self.config.user, Some(&self.config.password))
            .header(reqwest::header::CONTENT_TYPE, "application/sparql-query")
            .header(reqwest::header::ACCEPT, "application/n-triples")
            .body(query)
            .send()
            .await
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;

        if !response.status().is_success() {
            return Err(OccurrenceServiceError::StoreFailed);
        }

        let ntriples = response
            .bytes()
            .await
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;

        if ntriples.is_empty() {
            return Ok(None);
        }

        let triples = RdfParser::from_format(RdfFormat::NTriples)
            .for_slice(&ntriples)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;

        if triples.is_empty() {
            return Ok(None);
        }

        let graph_name =
            NamedNode::new(graph_uri).map_err(|_| OccurrenceServiceError::StoreFailed)?;

        let quads = triples
            .into_iter()
            .map(|triple| {
                Quad::new(
                    triple.subject,
                    triple.predicate,
                    triple.object,
                    GraphName::NamedNode(graph_name.clone()),
                )
            })
            .collect::<Vec<_>>();

        let mut nquads = Vec::new();

        let mut serializer = RdfSerializer::from_format(RdfFormat::NQuads).for_writer(&mut nquads);

        for quad in quads {
            serializer
                .serialize_quad(&quad)
                .map_err(|_| OccurrenceServiceError::StoreFailed)?;
        }

        serializer
            .finish()
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;

        Ok(Some(nquads))
    }
}

fn build_search_filter_patterns(
    filters: &[SearchOccurrenceFilterInput],
) -> Result<String, OccurrenceServiceError> {
    let mut patterns = Vec::new();

    for filter in filters {
        if filter.match_type != "exact" {
            return Err(OccurrenceServiceError::StoreFailed);
        }

        let predicate = escape_sparql_iri(&filter.predicate)?;

        let object = match filter.value_type.as_str() {
            "literal" => format!("\"{}\"", escape_sparql_literal(&filter.value)),
            "uri" => format!("<{}>", escape_sparql_iri(&filter.value)?),
            _ => return Err(OccurrenceServiceError::StoreFailed),
        };

        patterns.push(format!("?occurrence <{}> {} .", predicate, object));
    }

    Ok(patterns.join("\n"))
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

fn search_next_cursor(row: &SearchOccurrenceStoreRow) -> String {
    let cursor = serde_json::json!({
        "created": row.created.as_deref().unwrap_or(""),
        "occurrence_uri": row.occurrence_uri,
    });

    hex::encode(cursor.to_string())
}

fn build_search_cursor_filter(cursor: Option<&str>) -> Result<String, OccurrenceServiceError> {//only created
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

fn binding_value(binding: &serde_json::Value, name: &str) -> Option<String> {
    binding.get(name)?.get("value")?.as_str().map(str::to_string)
}

fn access_rights_label(value: &str) -> String {
    value
        .strip_prefix("https://bio-database.net/terms/access-rights/")
        .unwrap_or(value)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use uuid::Uuid;

    #[tokio::test]
    #[ignore]
    async fn post_nquads_posts_to_running_fuseki() {
        dotenvy::dotenv().ok();

        let config = Config::from_env().expect("config should be loaded from .env");

        let client = FusekiClient::new(config.fuseki);

        let occurrence_id = uuid::Uuid::new_v4();

        let nquads = format!(
            r#"<https://bio-database.net/occurrences/test-{occurrence_id}> <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
"#
        );

        client
            .post_nquads(nquads.into_bytes())
            .await
            .expect("n-quads should be posted to Fuseki");
    }

    #[tokio::test]
    #[ignore]
    async fn fuseki_client_save_nquads_inserts_data_into_fuseki() {
        dotenvy::dotenv().ok();

        let config = FusekiConfig {
            base_url: std::env::var("FUSEKI_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3033/occurrence".to_string()),
            user: std::env::var("FUSEKI_USER").unwrap_or_else(|_| "occurrence_backend".to_string()),
            password: std::env::var("FUSEKI_PASSWORD")
                .unwrap_or_else(|_| "change_me_backend_password".to_string()),
        };

        let client = FusekiClient::new(config);

        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", Uuid::new_v4());

        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let predicate_uri = "https://example.org/vocab/scientificName";
        let scientific_name = "Lumbricus terrestris";

        let nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
"#,
            occurrence_uri, predicate_uri, scientific_name, graph_uri
        );

        client
            .save_nquads(nquads.into_bytes())
            .await
            .expect("N-Quads should be saved to Fuseki");

        let query = format!(
            r#"
            ASK WHERE {{
                GRAPH <{}> {{
                <{}> <{}> "{}" .
                }}
            }}
            "#,
            graph_uri, occurrence_uri, predicate_uri, scientific_name
        );

        let response = reqwest::Client::new()
            .post("http://127.0.0.1:3033/occurrence/sparql")
            .basic_auth("occurrence_backend", Some("change_me_backend_password"))
            .header(reqwest::header::CONTENT_TYPE, "application/sparql-query")
            .header(reqwest::header::ACCEPT, "application/sparql-results+json")
            .body(query)
            .send()
            .await
            .expect("SPARQL ASK request should be sent");

        assert!(
            response.status().is_success(),
            "SPARQL ASK should succeed, got {}",
            response.status()
        );

        let body: serde_json::Value = response
            .json()
            .await
            .expect("SPARQL ASK response should be JSON");

        assert_eq!(
            body["boolean"], true,
            "saved N-Quads should be queryable from Fuseki"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn fuseki_client_search_occurrences_returns_saved_occurrence_from_real_fuseki() {
        use crate::features::occurrences::service::{
            SearchOccurrenceFilterInput, SearchOccurrencesStoreInput,
        };

        dotenvy::dotenv().ok();

        let config = FusekiConfig {
            base_url: std::env::var("FUSEKI_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3033/occurrence".to_string()),
            user: std::env::var("FUSEKI_USER").unwrap_or_else(|_| "occurrence_backend".to_string()),
            password: std::env::var("FUSEKI_PASSWORD")
                .unwrap_or_else(|_| "change_me_backend_password".to_string()),
        };

        let client = FusekiClient::new(config);

        let occurrence_id = Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let other_occurrence_id = Uuid::new_v4();
        let other_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            other_occurrence_id
        );

        let creator_user_id = Uuid::new_v4();
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let scientific_name_predicate = "http://rs.tdwg.org/dwc/terms/scientificName";
        let basis_of_record_predicate = "http://rs.tdwg.org/dwc/terms/basisOfRecord";
        let recorded_by_predicate = "http://rs.tdwg.org/dwc/terms/recordedBy";
        let created_predicate = "http://purl.org/dc/terms/created";
        let modified_predicate = "http://purl.org/dc/terms/modified";
        let access_rights_predicate = "http://purl.org/dc/terms/accessRights";
        let creator_predicate = "http://purl.org/dc/terms/creator";
        let public_access_rights_uri = "https://bio-database.net/terms/access-rights/public";
        let scientific_name = format!("Quercus serrata {}", Uuid::new_v4());
        let other_scientific_name = format!("Acer palmatum {}", Uuid::new_v4());

        let nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
<{}> <{}> "PreservedSpecimen" <{}> .
<{}> <{}> "Yamada Taro" <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> <{}> <{}> .
<{}> <{}> <https://bio-database.net/users/{}> <{}> .
<{}> <{}> "{}" <{}> .
"#,
            occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            graph_uri,
            occurrence_uri,
            basis_of_record_predicate,
            graph_uri,
            occurrence_uri,
            recorded_by_predicate,
            graph_uri,
            occurrence_uri,
            created_predicate,
            graph_uri,
            occurrence_uri,
            modified_predicate,
            graph_uri,
            occurrence_uri,
            access_rights_predicate,
            public_access_rights_uri,
            graph_uri,
            occurrence_uri,
            creator_predicate,
            creator_user_id,
            graph_uri,
            other_occurrence_uri,
            scientific_name_predicate,
            other_scientific_name,
            graph_uri,
        );

        client
            .save_nquads(nquads.into_bytes())
            .await
            .expect("test N-Quads should be saved to Fuseki");

        let page = client
            .search_occurrences(SearchOccurrencesStoreInput {
                filters: vec![SearchOccurrenceFilterInput {
                    predicate: scientific_name_predicate.to_string(),
                    value: scientific_name.clone(),
                    value_type: "literal".to_string(),
                    match_type: "exact".to_string(),
                }],
                limit: 50,
                cursor: None,
            })
            .await
            .expect("occurrence search should fetch rows from real Fuseki");

        assert_eq!(page.limit, 50);
        assert_eq!(page.rows.len(), 1);
        assert!(!page.has_next);
        assert!(page.next_cursor.is_none());

        let row = &page.rows[0];
        assert_eq!(row.occurrence_id, occurrence_id);
        assert_eq!(row.occurrence_uri, occurrence_uri);
        assert_eq!(row.creator_user_id, Some(creator_user_id));
        assert_eq!(row.scientific_name.as_deref(), Some(scientific_name.as_str()));
        assert_eq!(row.basis_of_record.as_deref(), Some("PreservedSpecimen"));
        assert_eq!(row.recorded_by.as_deref(), Some("Yamada Taro"));
        assert_eq!(row.created.as_deref(), Some("2026-06-02T10:20:30Z"));
        assert_eq!(row.modified.as_deref(), Some("2026-06-02T10:20:30Z"));
        assert_eq!(row.access_rights.as_deref(), Some("public"));
    }

    #[tokio::test]
    #[ignore]
    async fn fuseki_client_search_occurrences_returns_next_cursor_when_results_exceed_limit() {
        use crate::features::occurrences::service::{
            SearchOccurrenceFilterInput, SearchOccurrencesStoreInput,
        };

        dotenvy::dotenv().ok();

        let config = FusekiConfig {
            base_url: std::env::var("FUSEKI_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3033/occurrence".to_string()),
            user: std::env::var("FUSEKI_USER").unwrap_or_else(|_| "occurrence_backend".to_string()),
            password: std::env::var("FUSEKI_PASSWORD")
                .unwrap_or_else(|_| "change_me_backend_password".to_string()),
        };

        let client = FusekiClient::new(config);

        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let scientific_name_predicate = "http://rs.tdwg.org/dwc/terms/scientificName";
        let created_predicate = "http://purl.org/dc/terms/created";
        let modified_predicate = "http://purl.org/dc/terms/modified";
        let access_rights_predicate = "http://purl.org/dc/terms/accessRights";
        let public_access_rights_uri = "https://bio-database.net/terms/access-rights/public";
        let scientific_name = format!("Pagination target {}", Uuid::new_v4());

        let newer_occurrence_id = Uuid::new_v4();
        let newer_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            newer_occurrence_id
        );
        let older_occurrence_id = Uuid::new_v4();
        let older_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            older_occurrence_id
        );

        let nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
<{}> <{}> "2026-06-02T10:20:31Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> "2026-06-02T10:20:31Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> <{}> <{}> .
<{}> <{}> "{}" <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> <{}> <{}> .
"#,
            newer_occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            graph_uri,
            newer_occurrence_uri,
            created_predicate,
            graph_uri,
            newer_occurrence_uri,
            modified_predicate,
            graph_uri,
            newer_occurrence_uri,
            access_rights_predicate,
            public_access_rights_uri,
            graph_uri,
            older_occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            graph_uri,
            older_occurrence_uri,
            created_predicate,
            graph_uri,
            older_occurrence_uri,
            modified_predicate,
            graph_uri,
            older_occurrence_uri,
            access_rights_predicate,
            public_access_rights_uri,
            graph_uri,
        );

        client
            .save_nquads(nquads.into_bytes())
            .await
            .expect("pagination test N-Quads should be saved to Fuseki");

        let page = client
            .search_occurrences(SearchOccurrencesStoreInput {
                filters: vec![SearchOccurrenceFilterInput {
                    predicate: scientific_name_predicate.to_string(),
                    value: scientific_name,
                    value_type: "literal".to_string(),
                    match_type: "exact".to_string(),
                }],
                limit: 1,
                cursor: None,
            })
            .await
            .expect("occurrence search should fetch the first limited page from real Fuseki");

        assert_eq!(page.limit, 1);
        assert_eq!(page.rows.len(), 1, "search should return only limit rows");
        assert_eq!(page.rows[0].occurrence_id, newer_occurrence_id);
        assert!(page.has_next, "search should detect another page when results exceed limit");
        assert!(
            page.next_cursor.is_some(),
            "search should return next_cursor when results exceed limit"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn fuseki_client_search_occurrences_uses_cursor_to_return_next_page() {
        use crate::features::occurrences::service::{
            SearchOccurrenceFilterInput, SearchOccurrencesStoreInput,
        };

        dotenvy::dotenv().ok();

        let config = FusekiConfig {
            base_url: std::env::var("FUSEKI_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3033/occurrence".to_string()),
            user: std::env::var("FUSEKI_USER").unwrap_or_else(|_| "occurrence_backend".to_string()),
            password: std::env::var("FUSEKI_PASSWORD")
                .unwrap_or_else(|_| "change_me_backend_password".to_string()),
        };

        let client = FusekiClient::new(config);

        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let scientific_name_predicate = "http://rs.tdwg.org/dwc/terms/scientificName";
        let created_predicate = "http://purl.org/dc/terms/created";
        let modified_predicate = "http://purl.org/dc/terms/modified";
        let access_rights_predicate = "http://purl.org/dc/terms/accessRights";
        let public_access_rights_uri = "https://bio-database.net/terms/access-rights/public";
        let scientific_name = format!("Cursor target {}", Uuid::new_v4());

        let newer_occurrence_id = Uuid::new_v4();
        let newer_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            newer_occurrence_id
        );
        let older_occurrence_id = Uuid::new_v4();
        let older_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            older_occurrence_id
        );

        let nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
<{}> <{}> "2026-06-02T10:20:31Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> "2026-06-02T10:20:31Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> <{}> <{}> .
<{}> <{}> "{}" <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> <{}> <{}> .
"#,
            newer_occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            graph_uri,
            newer_occurrence_uri,
            created_predicate,
            graph_uri,
            newer_occurrence_uri,
            modified_predicate,
            graph_uri,
            newer_occurrence_uri,
            access_rights_predicate,
            public_access_rights_uri,
            graph_uri,
            older_occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            graph_uri,
            older_occurrence_uri,
            created_predicate,
            graph_uri,
            older_occurrence_uri,
            modified_predicate,
            graph_uri,
            older_occurrence_uri,
            access_rights_predicate,
            public_access_rights_uri,
            graph_uri,
        );

        client
            .save_nquads(nquads.into_bytes())
            .await
            .expect("cursor test N-Quads should be saved to Fuseki");

        let filter = SearchOccurrenceFilterInput {
            predicate: scientific_name_predicate.to_string(),
            value: scientific_name,
            value_type: "literal".to_string(),
            match_type: "exact".to_string(),
        };

        let first_page = client
            .search_occurrences(SearchOccurrencesStoreInput {
                filters: vec![filter.clone()],
                limit: 1,
                cursor: None,
            })
            .await
            .expect("first page should be fetched from real Fuseki");

        assert_eq!(first_page.rows.len(), 1);
        assert_eq!(first_page.rows[0].occurrence_id, newer_occurrence_id);
        assert!(first_page.has_next);
        let cursor = first_page
            .next_cursor
            .expect("first page should return next_cursor");

        let second_page = client
            .search_occurrences(SearchOccurrencesStoreInput {
                filters: vec![filter],
                limit: 1,
                cursor: Some(cursor),
            })
            .await
            .expect("second page should be fetched from real Fuseki using cursor");

        assert_eq!(second_page.rows.len(), 1);
        assert_eq!(second_page.rows[0].occurrence_id, older_occurrence_id);
        assert!(!second_page.has_next);
        assert!(second_page.next_cursor.is_none());
    }

    #[tokio::test]
    #[ignore]
    async fn fuseki_client_get_occurrence_nquads_returns_only_requested_occurrence() {
        dotenvy::dotenv().ok();

        let config = FusekiConfig {
            base_url: std::env::var("FUSEKI_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3033/occurrence".to_string()),
            user: std::env::var("FUSEKI_USER").unwrap_or_else(|_| "occurrence_backend".to_string()),
            password: std::env::var("FUSEKI_PASSWORD")
                .unwrap_or_else(|_| "change_me_backend_password".to_string()),
        };

        let client = FusekiClient::new(config);

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        let other_occurrence_id = uuid::Uuid::new_v4();
        let other_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            other_occurrence_id
        );

        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let scientific_name_predicate = "https://example.org/vocab/scientificName";
        let creator_predicate = "http://purl.org/dc/terms/creator";

        let scientific_name = format!("Lumbricus terrestris {}", uuid::Uuid::new_v4());

        let nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
    <{}> <{}> <https://bio-database.net/users/test-user> <{}> .
    <{}> <{}> "Should not be returned" <{}> .
    "#,
            occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            graph_uri,
            occurrence_uri,
            creator_predicate,
            graph_uri,
            other_occurrence_uri,
            scientific_name_predicate,
            graph_uri,
        );

        client
            .save_nquads(nquads.into_bytes())
            .await
            .expect("test N-Quads should be saved to Fuseki");

        let result = client
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("occurrence N-Quads should be fetched from Fuseki");

        let fetched_nquads = result.expect("existing occurrence should return Some");

        let parsed_quads = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::NQuads)
            .for_slice(&fetched_nquads)
            .collect::<Result<Vec<_>, _>>()
            .expect("fetched N-Quads should be valid");

        assert_eq!(
            parsed_quads.len(),
            2,
            "only quads whose subject is the requested occurrence URI should be returned"
        );

        let expected_subject = format!("<{}>", occurrence_uri);
        let unexpected_subject = format!("<{}>", other_occurrence_uri);
        let expected_graph = format!("<{}>", graph_uri);

        assert!(
            parsed_quads
                .iter()
                .all(|quad| { quad.subject.to_string() == expected_subject }),
            "all returned quads should have the requested occurrence URI as subject"
        );

        assert!(
            parsed_quads
                .iter()
                .all(|quad| { quad.graph_name.to_string() == expected_graph }),
            "all returned quads should be in the occurrence graph"
        );

        assert!(
            parsed_quads
                .iter()
                .all(|quad| { quad.subject.to_string() != unexpected_subject }),
            "quads from other occurrences should not be returned"
        );

        let has_scientific_name = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == format!("<{}>", scientific_name_predicate)
                && quad.object.to_string() == format!("\"{}\"", scientific_name)
        });

        assert!(
            has_scientific_name,
            "fetched N-Quads should contain the saved scientificName"
        );

        let has_creator = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == format!("<{}>", creator_predicate)
                && quad.object.to_string() == "<https://bio-database.net/users/test-user>"
        });

        assert!(
            has_creator,
            "fetched N-Quads should contain the saved creator"
        );
    }
}

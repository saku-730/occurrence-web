use crate::config::FusekiConfig;
use crate::features::occurrences::service::{OccurrenceRdfStore, OccurrenceServiceError};

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

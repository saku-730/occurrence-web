use crate::config::FusekiConfig;
use crate::features::occurrences::service::{
    OccurrenceRdfStore,
    OccurrenceServiceError,
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

    pub async fn post_nquads(
        &self,
        nquads: Vec<u8>,
    ) -> Result<(), FusekiClientError> {
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
            return Err(FusekiClientError::UnexpectedStatus(
                response.status(),
            ));
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl OccurrenceRdfStore for FusekiClient {
    async fn save_nquads(
        &self,
        nquads: Vec<u8>,
    ) -> Result<(), OccurrenceServiceError> {
        self.post_nquads(nquads)
            .await
            .map_err(|_| OccurrenceServiceError::StoreFailed)
    }

    async fn get_occurrence_nquads(
        &self,
        occurrence_uri: &str,
    ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
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

        let sparql_url = format!(
            "{}/sparql",
            self.config.base_url.trim_end_matches('/')
        );

        let response = self
            .client
            .post(sparql_url)
            .basic_auth(&self.config.user, Some(&self.config.password))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/sparql-query",
            )
            .header(
                reqwest::header::ACCEPT,
                "application/n-triples",
            )
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

        let ntriples_text = String::from_utf8(ntriples.to_vec())
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;

        if ntriples_text.trim().is_empty() {
            return Ok(None);
        }

        let nquads = ntriples_text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let line = line.trim();

                let triple_without_dot = line
                    .strip_suffix('.')
                    .map(str::trim_end)
                    .unwrap_or(line);

                format!("{triple_without_dot} <{graph_uri}> .\n")
            })
            .collect::<String>();

        Ok(Some(nquads.into_bytes()))
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

        let config = Config::from_env()
            .expect("config should be loaded from .env");

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
            user: std::env::var("FUSEKI_USER")
                .unwrap_or_else(|_| "occurrence_backend".to_string()),
            password: std::env::var("FUSEKI_PASSWORD")
                .unwrap_or_else(|_| "change_me_backend_password".to_string()),
        };

        let client = FusekiClient::new(config);

        let occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            Uuid::new_v4()
        );

        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let predicate_uri = "https://example.org/vocab/scientificName";
        let scientific_name = "Lumbricus terrestris";

        let nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
"#,
            occurrence_uri,
            predicate_uri,
            scientific_name,
            graph_uri
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
            graph_uri,
            occurrence_uri,
            predicate_uri,
            scientific_name
        );

        let response = reqwest::Client::new()
            .post("http://127.0.0.1:3033/occurrence/sparql")
            .basic_auth("occurrence_backend", Some("change_me_backend_password"))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/sparql-query",
            )
            .header(
                reqwest::header::ACCEPT,
                "application/sparql-results+json",
            )
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
}
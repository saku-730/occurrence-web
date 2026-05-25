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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

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
}
use std::sync::Arc;

use sqlx::PgPool;

use crate::{
    config::Config,
    features::occurrences::service::OccurrenceRdfStore,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub posgre: PgPool,
    pub occurrence_rdf_store: Arc<dyn OccurrenceRdfStore>,
}

impl AppState {
    pub fn new(
        config: Config,
        posgre: PgPool,
        occurrence_rdf_store: Arc<dyn OccurrenceRdfStore>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            posgre,
            occurrence_rdf_store,
        }
    }
}
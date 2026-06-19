use std::sync::Arc;

use sqlx::PgPool;

use crate::{config::Config, features::occurrences::service::OccurrenceRdfStore};

// axum handlerへ共有する依存をまとめる。テストではOccurrenceRdfStoreを差し替えてFusekiなしで検証する。
#[derive(Clone)]
pub struct AppState {
    // Configは読み取り専用で全handlerから参照するためArcで共有する。
    pub config: Arc<Config>,
    pub posgre: PgPool,
    // RDF storeはtrait objectにして、Fake/Fusekiを同じapp経由テストで使えるようにする。
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

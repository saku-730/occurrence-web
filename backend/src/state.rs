use std::sync::Arc;

use sqlx::PgPool;

use crate::{
    config::Config,
    features::{
        media::service::{MediaObjectStore, MediaServiceError, PutMediaObjectInput},
        occurrences::service::OccurrenceRdfStore,
    },
};

// axum handlerへ共有する依存をまとめる。テストではOccurrenceRdfStoreを差し替えてFusekiなしで検証する。
#[derive(Clone)]
pub struct AppState {
    // Configは読み取り専用で全handlerから参照するためArcで共有する。
    pub config: Arc<Config>,
    pub posgre: PgPool,
    // RDF storeはtrait objectにして、Fake/Fusekiを同じapp経由テストで使えるようにする。
    pub occurrence_rdf_store: Arc<dyn OccurrenceRdfStore>,
    // media object storeもtrait objectにし、appテストではGarageなしでservice接続を検証する。
    pub media_object_store: Arc<dyn MediaObjectStore>,
}

impl AppState {
    pub fn new(
        config: Config,
        posgre: PgPool,
        occurrence_rdf_store: Arc<dyn OccurrenceRdfStore>,
    ) -> Self {
        Self::new_with_media_object_store(
            config,
            posgre,
            occurrence_rdf_store,
            Arc::new(UnconfiguredMediaObjectStore),
        )
    }

    pub fn new_with_media_object_store(
        config: Config,
        posgre: PgPool,
        occurrence_rdf_store: Arc<dyn OccurrenceRdfStore>,
        media_object_store: Arc<dyn MediaObjectStore>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            posgre,
            occurrence_rdf_store,
            media_object_store,
        }
    }
}

#[derive(Debug)]
struct UnconfiguredMediaObjectStore;

#[async_trait::async_trait]
impl MediaObjectStore for UnconfiguredMediaObjectStore {
    async fn put_object(&self, _input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
        // 本番用S3/Garage clientを差し込むまでは、誤って成功扱いにしない。
        Err(MediaServiceError::ObjectStoreFailed)
    }
}

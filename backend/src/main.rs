use sqlx::postgres::PgPoolOptions;

use backend::{app::build_app, config::Config, state::AppState};
use std::sync::Arc;

use backend::features::occurrence_map::{
    self, geocoding::geocoding_middleware, location_store::ExtendedLocationRdfStore,
};
use backend::features::occurrences::service::OccurrenceRdfStore;
use backend::infrastructure::{fuseki::FusekiClient, garage::GarageMediaObjectStore};

#[tokio::main]
async fn main() {
    let config = Config::from_env().unwrap();

    let posgre = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.posgre.url)
        .await
        .expect("failed to connect postgresql server");

    let bind_addr = config.app.bind_addr();

    let fuseki_store: Arc<dyn OccurrenceRdfStore> =
        Arc::new(FusekiClient::new(config.fuseki.clone()));
    let occurrence_rdf_store = Arc::new(ExtendedLocationRdfStore::new(fuseki_store));

    let media_object_store = Arc::new(
        GarageMediaObjectStore::from_env().expect("failed to configure Garage object storage"),
    );

    let state = AppState::new_with_media_object_store(
        config,
        posgre,
        occurrence_rdf_store,
        media_object_store,
    );
    let app = build_app(state.clone())
        .merge(backend::features::paper_import::router(state.clone()))
        .merge(occurrence_map::router(state))
        .layer(axum::middleware::from_fn(geocoding_middleware));

    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();

    println!("listening on http://{}", bind_addr);
    axum::serve(listener, app).await.unwrap();
}

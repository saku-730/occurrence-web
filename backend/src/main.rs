use sqlx::postgres::PgPoolOptions;

use backend::{
    app::build_app, config::Config, infrastructure::fuseki, state::AppState
};
use std::sync::Arc;

use backend::infrastructure::fuseki::FusekiClient;

#[tokio::main]
async fn main() {
    let config = Config::from_env().unwrap();

    let posgre = PgPoolOptions::new()
    .max_connections(5)
    .connect(&config.posgre.url)
    .await.expect("failed to connect postgresql server");

    let fuseki_client = FusekiClient::new(config.fuseki.clone());
    let bind_addr = config.app.bind_addr();
    let state = AppState::new(config, posgre,Arc::new(fuseki_client),);
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap();

    println!("listening on http://{}",bind_addr);
    axum::serve(listener, app).await.unwrap();
}
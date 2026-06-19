use sqlx::postgres::PgPoolOptions;

use backend::{app::build_app, config::Config, state::AppState};
use std::sync::Arc;

use backend::infrastructure::fuseki::FusekiClient;

#[tokio::main]
async fn main() {
    // mainでは本番起動に必要な外部依存だけを組み立てる。
    // route構成はbuild_appへ寄せ、テストではAppStateを差し替えやすくしている。
    let config = Config::from_env().unwrap();

    let posgre = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.posgre.url)
        .await
        .expect("failed to connect postgresql server");

    let bind_addr = config.app.bind_addr();

    // occurrence RDFの永続化先はtraitで抽象化しているが、本番起動ではFusekiを使う。
    let occurrence_rdf_store = Arc::new(
        //for fuseki
        FusekiClient::new(config.fuseki.clone()),
    );

    let state = AppState::new(config, posgre, occurrence_rdf_store);
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();

    println!("listening on http://{}", bind_addr);
    axum::serve(listener, app).await.unwrap();
}

use sqlx::postgres::PgPoolOptions;

use backend::{app::build_app, config::Config, state::AppState};
use std::sync::Arc;

use backend::infrastructure::{fuseki::FusekiClient, garage::GarageMediaObjectStore};

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

    // 添付ファイルの本番保存先にはGarageのS3互換APIを使う。
    // 設定不備のまま起動してupload時だけ失敗するのを避けるため、起動時に必須環境変数を検証する。
    let media_object_store = Arc::new(
        GarageMediaObjectStore::from_env().expect("failed to configure Garage object storage"),
    );

    let state = AppState::new_with_media_object_store(
        config,
        posgre,
        occurrence_rdf_store,
        media_object_store,
    );
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();

    println!("listening on http://{}", bind_addr);
    axum::serve(listener, app).await.unwrap();
}

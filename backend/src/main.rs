use backend::app::build_app;
use backend::config::Config;
use backend::state::AppState;

#[tokio::main]
async fn main() {
    let config = Config::from_env().unwrap();
    let bind_addr = config.app.bind_addr();
    let state = AppState::new(config);
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap();

    println!("listening on http://{}",bind_addr);
    axum::serve(listener, app).await.unwrap();
}
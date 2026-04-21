use backend::app::build_app;
use backend::config::Config;
use backend::state::AppState;

#[tokio::main]
async fn main() {
    let config = Config::from_env().unwrap();
    let state = AppState::new(config);
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
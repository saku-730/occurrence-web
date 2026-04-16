use backend::app::build_app;

#[tokio::main]
async fn main() {
    let app = build_app();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
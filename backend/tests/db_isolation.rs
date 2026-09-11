#[path = "../src/test_support.rs"]
mod test_support;

use sqlx::PgPool;
use uuid::Uuid;

async fn insert_user(db: &PgPool, id: Uuid) {
    sqlx::query("INSERT INTO users (id, email, user_name, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind("same-fixture@example.invalid")
        .bind("isolated fixture")
        .bind("not-a-login-password")
        .execute(db)
        .await
        .unwrap();
}

#[tokio::test]
async fn independent_fixtures_preserve_public_users_and_each_other() {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let first = test_support::isolated_pool(&url);
    let second = test_support::isolated_pool(&url);
    // Read public only to verify preservation; never insert a sentinel into live user data.
    let before: String = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(u) ORDER BY id), '[]'::jsonb)::text FROM public.users u"
    ).fetch_one(&first).await.unwrap();
    let id = Uuid::new_v4();
    tokio::join!(insert_user(&first, id), insert_user(&second, id));

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&first)
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&second)
        .await
        .unwrap();
    assert_eq!(
        remaining, 1,
        "one fixture must not delete another fixture's user"
    );
    let after: String = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(u) ORDER BY id), '[]'::jsonb)::text FROM public.users u"
    ).fetch_one(&first).await.unwrap();
    // Avoid printing user data on failure.
    assert!(before == after, "public users must remain unchanged");
    first.close().await;
    second.close().await;
}

#[tokio::test]
async fn isolated_fixtures_enforce_foreign_keys() {
    dotenvy::dotenv().ok();
    let db = test_support::isolated_pool(&std::env::var("DATABASE_URL").unwrap());
    let result = sqlx::query(
        "INSERT INTO sessions (user_id, session_token_hash, expires_at)
         VALUES ($1, 'isolated-session', now() + interval '1 day')",
    )
    .bind(Uuid::new_v4())
    .execute(&db)
    .await;
    let error = result.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );
    db.close().await;
}

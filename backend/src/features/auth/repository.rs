use sqlx::PgPool;

pub struct AuthRepository;

impl AuthRepository {
    pub async fn create_pending_registration(
        db: &PgPool,
        email: &str,
        token_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO pending_registrations (
                email,
                token_hash,
                expires_at
            )
            VALUES (
                $1,
                $2,
                now() + interval '30 minutes'
            )
            "#,
        )
        .bind(email)
        .bind(token_hash)
        .execute(db)
        .await?;

        Ok(())
    }
}
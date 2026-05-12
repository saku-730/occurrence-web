use sqlx::{PgPool, Postgres, Transaction};

pub struct AuthRepository;

#[derive(Debug)]
pub struct PendingRegistration {
    pub email: String,
}

#[derive(Debug)]
pub struct UserForAuth {
    pub email: String,
    pub user_name: String,
    pub password_hash: String,
}

impl AuthRepository {
    pub async fn create_pending_registration(
        db: &PgPool,
        email: &str,
        token_hash: &str,
    ) -> Result<(), sqlx::Error> { //有効期限は30分
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

    pub async fn find_pending_registration_by_token_hash(
        db: &PgPool,
        token_hash: &str,
    ) -> Result<Option<PendingRegistration>, sqlx::Error> {
        let row = sqlx::query_as!(
            PendingRegistration,
            r#"
            SELECT email
            FROM pending_registrations
            WHERE token_hash = $1
                AND completed_at IS NULL
                AND expires_at > now()
            "#,
            token_hash
        )
        .fetch_optional(db)
        .await?;

        Ok(row)
    }

    pub async fn create_user(
        db: &PgPool,
        email: &str,
        user_name: &str,
        password_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO users (
                email,
                user_name,
                password_hash
            )
            VALUES ($1, $2, $3)
            "#,
            email,
            user_name,
            password_hash
        )
        .execute(db)
        .await?;

        Ok(())
    }

    pub async fn mark_pending_registration_completed( //本登録完了後、pending_registrationのcompleted_atに時刻をいれる。これが登録完了の印。
        db: &PgPool,
        token_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE pending_registrations
            SET completed_at = now()
            WHERE token_hash = $1
                AND completed_at IS NULL
            "#,
            token_hash
        )
        .execute(db)
        .await?;

        Ok(())
    }

    pub async fn user_exists_by_email(
        db: &PgPool,
        email: &str,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM users
                WHERE email = $1
            ) AS "exists!"
            "#,
            email
        )
        .fetch_one(db)
        .await?;

        Ok(row.exists)
    }

    pub async fn find_pending_registration_by_token_hash_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        token_hash: &str,
    ) -> Result<Option<PendingRegistration>, sqlx::Error> {
        let row = sqlx::query_as!(
            PendingRegistration,
            r#"
            SELECT email
            FROM pending_registrations
            WHERE token_hash = $1
                AND completed_at IS NULL
                AND expires_at > now()
            "#,
            token_hash
        )
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn user_exists_by_email_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        email: &str,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM users
                WHERE email = $1
            ) AS "exists!"
            "#,
            email
        )
        .fetch_one(&mut **tx)
        .await?;

        Ok(row.exists)
    }

    pub async fn create_user_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        email: &str,
        user_name: &str,
        password_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO users (
                email,
                user_name,
                password_hash
            )
            VALUES ($1, $2, $3)
            "#,
            email,
            user_name,
            password_hash
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn mark_pending_registration_completed_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        token_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE pending_registrations
            SET completed_at = now()
            WHERE token_hash = $1
                AND completed_at IS NULL
            "#,
            token_hash
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn find_user_by_email( //既存ユーザーをメールで検索
        db: &PgPool,
        email: &str,
    ) -> Result<Option<UserForAuth>, sqlx::Error> {
        let row = sqlx::query_as!(
            UserForAuth,
            r#"
            SELECT email, user_name, password_hash
            FROM users
            WHERE email = $1
            "#,
            email
        )
        .fetch_optional(db)
        .await?;

        Ok(row)
    }
}



#[cfg(test)]
mod tests {
    use super::AuthRepository;
    use sqlx::{postgres::PgPoolOptions, PgPool};

    async fn test_db_pool() -> PgPool {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for repository tests");

        PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("failed to connect test database")
    }

    async fn delete_pending_registration_by_token_hash(db: &PgPool, token_hash: &str) {
        sqlx::query(
            r#"
            DELETE FROM pending_registrations
            WHERE token_hash = $1
            "#,
        )
        .bind(token_hash)
        .execute(db)
        .await
        .expect("failed to delete pending registration");
    }

    #[tokio::test]
    async fn create_pending_registration_inserts_row() {
        let db = test_db_pool().await;

        let email = "repository-insert@example.com";
        let token_hash = "repository-test-token-hash";

        delete_pending_registration_by_token_hash(&db, token_hash).await;

        let result =
            AuthRepository::create_pending_registration(&db, email, token_hash).await;

        assert!(result.is_ok());

        let row: (String, String, bool, bool) = sqlx::query_as(
            r#"
            SELECT
                email,
                token_hash,
                completed_at IS NULL,
                expires_at > now()  + interval '28 minutes'
            FROM pending_registrations
            WHERE token_hash = $1
            "#,
        )
        .bind(token_hash)
        .fetch_one(&db)
        .await
        .expect("failed to fetch pending registration");

        assert_eq!(row.0, email);
        assert_eq!(row.1, token_hash);
        assert!(row.2);
        assert!(row.3);

        delete_pending_registration_by_token_hash(&db, token_hash).await;
    }
    #[tokio::test]
    async fn create_pending_registration_rejects_duplicate_token_hash() {
        let db = test_db_pool().await;

        let token_hash = format!("duplicate-token-hash-{}", uuid::Uuid::new_v4());
        let email1 = format!("duplicate-1-{}@example.com", uuid::Uuid::new_v4());
        let email2 = format!("duplicate-2-{}@example.com", uuid::Uuid::new_v4());

        delete_pending_registration_by_token_hash(&db, &token_hash).await;

        let first_result =
            AuthRepository::create_pending_registration(&db, &email1, &token_hash).await;

        assert!(first_result.is_ok());

        let second_result =
            AuthRepository::create_pending_registration(&db, &email2, &token_hash).await;

        assert!(second_result.is_err());

        delete_pending_registration_by_token_hash(&db, &token_hash).await;
    }
}
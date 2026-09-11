use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct LabelTemplateRecord {
    pub id: Uuid,
    pub template_json: String,
}

pub struct LabelTemplateRepository;

impl LabelTemplateRepository {
    pub async fn create_if_under_limit(
        db: &PgPool,
        id: Uuid,
        user_id: Uuid,
        template_json: &str,
        max_templates: i64,
    ) -> Result<Option<LabelTemplateRecord>, sqlx::Error> {
        let mut transaction = db.begin().await?;

        // Serialize template creation per user so concurrent POST requests cannot
        // both observe the same count and exceed the per-user limit.
        sqlx::query(
            r#"
            SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))
            "#,
        )
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;

        let template_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM label_templates
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_one(&mut *transaction)
        .await?;

        if template_count >= max_templates {
            transaction.rollback().await?;
            return Ok(None);
        }

        let record = sqlx::query_as::<_, LabelTemplateRecord>(
            r#"
            INSERT INTO label_templates (id, user_id, template)
            VALUES ($1, $2, $3::jsonb)
            RETURNING id, template::text AS template_json
            "#,
        )
        .bind(id)
        .bind(user_id)
        .bind(template_json)
        .fetch_one(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(Some(record))
    }

    pub async fn list_by_user(
        db: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<LabelTemplateRecord>, sqlx::Error> {
        sqlx::query_as::<_, LabelTemplateRecord>(
            r#"
            SELECT id, template::text AS template_json
            FROM label_templates
            WHERE user_id = $1
            ORDER BY updated_at DESC, created_at DESC, id DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(db)
        .await
    }

    pub async fn find_by_id_and_user(
        db: &PgPool,
        id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<LabelTemplateRecord>, sqlx::Error> {
        sqlx::query_as::<_, LabelTemplateRecord>(
            r#"
            SELECT id, template::text AS template_json
            FROM label_templates
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await
    }

    pub async fn update(
        db: &PgPool,
        id: Uuid,
        user_id: Uuid,
        template_json: &str,
    ) -> Result<Option<LabelTemplateRecord>, sqlx::Error> {
        sqlx::query_as::<_, LabelTemplateRecord>(
            r#"
            UPDATE label_templates
            SET template = $3::jsonb,
                updated_at = now()
            WHERE id = $1 AND user_id = $2
            RETURNING id, template::text AS template_json
            "#,
        )
        .bind(id)
        .bind(user_id)
        .bind(template_json)
        .fetch_optional(db)
        .await
    }

    pub async fn delete(db: &PgPool, id: Uuid, user_id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            r#"
            DELETE FROM label_templates
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(id)
        .bind(user_id)
        .execute(db)
        .await?;

        Ok(result.rows_affected() == 1)
    }
}

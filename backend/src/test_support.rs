//! PostgreSQL fixtures confined to one connection. Never reset shared public tables.
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};

#[allow(dead_code)] // Some integration-test crates only need pool_options().
pub fn isolated_pool(database_url: &str) -> PgPool {
    pool_options()
        .connect_lazy(database_url)
        .expect("isolated test database URL should be valid")
}

pub fn pool_options() -> PgPoolOptions {
    PgPoolOptions::new()
        // All callers in this fixture share its connection; other tests get their own.
        // Disable recycling because reconnecting would silently erase the fixture.
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .after_connect(|connection, _| {
            Box::pin(async move {
                let tables: Vec<String> = sqlx::query_scalar(
                    "SELECT tablename::text FROM pg_tables WHERE schemaname = 'public'
                 AND tablename IN ('users', 'sessions', 'pending_registrations',
                 'password_reset_tokens', 'media_objects', 'papers', 'paper_imports',
                 'label_templates') ORDER BY tablename",
                )
                .fetch_all(&mut *connection)
                .await?;
                for table in &tables {
                    connection
                        .execute(
                            format!(
                                "CREATE TEMPORARY TABLE {table} (LIKE public.{table} INCLUDING ALL)"
                            )
                            .as_str(),
                        )
                        .await?;
                }
                // LIKE omits foreign keys. Recreate them against the temporary tables so
                // rollback/authorization tests exercise constraints without touching public.
                let constraints: Vec<(String, String, String)> = sqlx::query_as(
                    "SELECT r.relname::text, c.conname::text, pg_get_constraintdef(c.oid)
                 FROM pg_constraint c JOIN pg_class r ON r.oid = c.conrelid
                 JOIN pg_namespace n ON n.oid = r.relnamespace
                 WHERE n.nspname = 'public' AND c.contype = 'f' AND r.relname = ANY($1)",
                )
                .bind(&tables)
                .fetch_all(&mut *connection)
                .await?;
                // No public fallback: a forgotten fixture table must fail, never use live data.
                connection
                    .execute("SET search_path TO pg_temp, pg_catalog")
                    .await?;
                for (table, name, definition) in constraints {
                    let definition =
                        definition.replace("REFERENCES public.", "REFERENCES pg_temp.");
                    let quoted_name = name.replace('"', "\"\"");
                    connection.execute(format!(
                    "ALTER TABLE pg_temp.{table} ADD CONSTRAINT \"{quoted_name}\" {definition}"
                ).as_str()).await?;
                }
                Ok(())
            })
        })
}

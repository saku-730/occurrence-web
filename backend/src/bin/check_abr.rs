use std::{env, process, time::Duration};

use backend::infrastructure::abr::AbrClient;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url = match env::var("ABR_DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => fail("ABR_DATABASE_URL is not set"),
    };

    let mut args = env::args().skip(1);
    let country = args.next().unwrap_or_else(|| "Japan".to_string());
    let locality_parts = args.collect::<Vec<_>>();
    let locality = if locality_parts.is_empty() {
        "滋賀県大津市".to_string()
    } else {
        locality_parts.join(" ")
    };

    println!("[1/4] Connecting to ABR PostgreSQL...");
    let pool = match tokio::time::timeout(
        Duration::from_secs(5),
        PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url),
    )
    .await
    {
        Ok(Ok(pool)) => pool,
        Ok(Err(error)) => fail(&format!("ABR PostgreSQL connection failed: {error}")),
        Err(_) => fail("ABR PostgreSQL connection timed out after 5 seconds"),
    };

    print_connection_info(&pool).await;
    check_required_tables(&pool).await;
    print_table_counts(&pool).await;

    println!("[4/4] Testing Bio-Database ABR resolver...");
    let client = AbrClient::from_pool(pool.clone(), 16);
    match client.resolve(&country, &locality).await {
        Ok(Some(resolved)) => {
            println!("  OK resolver matched");
            println!("  country_code      = {}", resolved.country_code);
            println!("  prefecture        = {}", resolved.prefecture);
            println!(
                "  municipality      = {}",
                resolved.municipality.as_deref().unwrap_or("<none>")
            );
            println!(
                "  machiaza          = {}",
                resolved.machiaza.as_deref().unwrap_or("<none>")
            );
            println!(
                "  remainder         = {}",
                resolved.remainder.as_deref().unwrap_or("<none>")
            );
            println!("  match_level       = {:?}", resolved.match_level);
        }
        Ok(None) => fail(&format!(
            "ABR connection is healthy, but resolver found no match for country={country:?}, locality={locality:?}"
        )),
        Err(error) => fail(&format!("ABR resolver query failed: {error:?}")),
    }

    println!();
    println!("ABR connection/resolver diagnostic: SUCCESS");
}

async fn print_connection_info(pool: &PgPool) {
    let row = match sqlx::query(
        r#"
        SELECT
            current_database() AS database_name,
            current_user AS database_user,
            COALESCE(inet_server_addr()::text, 'local-socket') AS server_address,
            inet_server_port() AS server_port
        "#,
    )
    .fetch_one(pool)
    .await
    {
        Ok(row) => row,
        Err(error) => fail(&format!("Connected, but failed to read PostgreSQL connection info: {error}")),
    };

    let database_name: String = row.try_get("database_name").unwrap_or_else(|error| {
        fail(&format!("Failed to decode current_database(): {error}"))
    });
    let database_user: String = row.try_get("database_user").unwrap_or_else(|error| {
        fail(&format!("Failed to decode current_user: {error}"))
    });
    let server_address: String = row.try_get("server_address").unwrap_or_else(|error| {
        fail(&format!("Failed to decode inet_server_addr(): {error}"))
    });
    let server_port: Option<i32> = row.try_get("server_port").unwrap_or_else(|error| {
        fail(&format!("Failed to decode inet_server_port(): {error}"))
    });

    println!("  OK database={database_name}, user={database_user}, server={server_address}:{}", server_port.map(|value| value.to_string()).unwrap_or_else(|| "<socket>".to_string()));
}

async fn check_required_tables(pool: &PgPool) {
    println!("[2/4] Checking official abrdb tables...");
    for table in ["mt_pref_unified", "mt_city_unified", "mt_town_unified"] {
        let qualified = format!("public.{table}");
        let exists: Option<String> = match sqlx::query_scalar("SELECT to_regclass($1)::text")
            .bind(&qualified)
            .fetch_one(pool)
            .await
        {
            Ok(value) => value,
            Err(error) => fail(&format!("Failed checking {qualified}: {error}")),
        };

        if exists.is_none() {
            fail(&format!(
                "Required ABR table {qualified} does not exist. Did abrdb import complete?"
            ));
        }
        println!("  OK {qualified}");
    }
}

async fn print_table_counts(pool: &PgPool) {
    println!("[3/4] Checking imported ABR data...");
    for table in ["mt_pref_unified", "mt_city_unified", "mt_town_unified"] {
        let sql = format!("SELECT COUNT(*)::bigint FROM public.{table}");
        let count: i64 = match sqlx::query_scalar(&sql).fetch_one(pool).await {
            Ok(value) => value,
            Err(error) => fail(&format!("Failed counting public.{table}: {error}")),
        };
        if count == 0 {
            fail(&format!("public.{table} exists but contains 0 rows"));
        }
        println!("  OK public.{table}: {count} rows");
    }
}

fn fail(message: &str) -> ! {
    eprintln!();
    eprintln!("ABR diagnostic: FAILED");
    eprintln!("{message}");
    process::exit(1);
}

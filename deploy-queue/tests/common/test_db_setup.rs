use anyhow::Result;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};

/// Helper function to create local PostgreSQL connection URLs
fn create_local_urls(unique_db_name: &str) -> (String, String) {
    let base_url = format!("postgresql://localhost/{}?sslmode=disable", unique_db_name);
    let admin_url = "postgresql://localhost/postgres?sslmode=disable".to_string();
    (base_url, admin_url)
}

/// Parse TEST_DATABASE_URL and replace the database name with a unique name
/// Returns (base_url, admin_url) or falls back to local URLs if parsing fails
fn parse_and_replace_database_url(test_url: &str, unique_db_name: &str) -> (String, String) {
    let url_parts: Vec<&str> = test_url.rsplitn(2, '/').collect();
    if url_parts.len() == 2 {
        // url_parts[1] is everything before the last '/', url_parts[0] is the database name
        let base_with_unique = format!("{}/{}", url_parts[1], unique_db_name);
        let admin_with_postgres = format!("{}/postgres", url_parts[1]);
        (base_with_unique, admin_with_postgres)
    } else {
        // Fallback to default local setup if URL parsing fails
        create_local_urls(unique_db_name)
    }
}

/// Helper to create a test database connection with unique database name
/// Creates a unique database per test to allow parallel execution
pub async fn create_test_db_connection() -> Result<Pool<Postgres>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Create unique database name for each test to allow parallel execution
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let thread_id = std::thread::current().id();
    let unique_db_name = format!("test_deploy_queue_{}_{:?}", timestamp, thread_id)
        .replace("ThreadId(", "")
        .replace(")", "");

    // Parse base connection URL (either from TEST_DATABASE_URL or default)
    let (base_url, admin_url) = if let Ok(test_url) = std::env::var("TEST_DATABASE_URL") {
        parse_and_replace_database_url(&test_url, &unique_db_name)
    } else {
        create_local_urls(&unique_db_name)
    };

    // First, connect to postgres database to create our test database
    let admin_pool = PgPoolOptions::new().connect(&admin_url).await?;

    // Create the unique test database
    sqlx::query(&format!("CREATE DATABASE \"{}\"", unique_db_name))
        .execute(&admin_pool)
        .await?;

    // Now connect to our newly created database
    let pool = PgPoolOptions::new().connect(&base_url).await?;

    Ok(pool)
}

/// Helper to set up test database with migrations
pub async fn setup_test_db() -> Result<Pool<Postgres>> {
    let pool = create_test_db_connection().await?;

    // Run migrations - they're idempotent so safe to run multiple times
    // This will also insert the default 'dev' and 'prod' environments
    sqlx::migrate!().set_ignore_missing(true).run(&pool).await?;

    Ok(pool)
}

use anyhow::{Context, Result};
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use url::Url;

/// Replace the database name in a PostgreSQL URL with the specified name
/// Returns the URL string with the new database name
fn replace_database_name(database_url: &str, db_name: &str) -> Result<String> {
    let mut url = Url::parse(database_url)
        .with_context(|| format!("Failed to parse database URL: {}", database_url))?;

    url.set_path(db_name);
    Ok(url.to_string())
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

    // Get the database URL from environment or use localhost fallback
    let database_url = std::env::var("TEST_DATABASE_URL")
        .context("TEST_DATABASE_URL environment variable is not set")?;

    // Create URLs for both the unique test database and admin database
    let test_db_url = replace_database_name(&database_url, &unique_db_name)?;
    let admin_url = replace_database_name(&database_url, "postgres")?;

    // First, connect to postgres database to create our test database
    let admin_pool = PgPoolOptions::new().connect(&admin_url).await?;

    // Create the unique test database
    sqlx::query(&format!("CREATE DATABASE \"{}\"", unique_db_name))
        .execute(&admin_pool)
        .await?;

    // Now connect to our newly created database
    let pool = PgPoolOptions::new().connect(&test_db_url).await?;

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

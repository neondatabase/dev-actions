use anyhow::Result;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};

/// Helper to create a test database connection with unique database name
/// Creates a unique database per test to allow parallel execution
pub async fn create_test_db_connection() -> Result<Pool<Postgres>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // If TEST_DATABASE_URL is set (e.g., in CI), use it directly
    if let Ok(database_url) = std::env::var("TEST_DATABASE_URL") {
        let pool = PgPoolOptions::new().connect(&database_url).await?;
        return Ok(pool);
    }

    // Create unique database name for each test to allow parallel execution
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let thread_id = std::thread::current().id();
    let unique_db_name = format!("test_deploy_queue_{}_{:?}", timestamp, thread_id)
        .replace("ThreadId(", "")
        .replace(")", "");

    // First, connect to postgres database to create our test database
    let admin_pool = PgPoolOptions::new()
        .connect("postgresql://localhost/postgres?sslmode=disable")
        .await?;

    // Create the unique test database
    sqlx::query(&format!("CREATE DATABASE \"{}\"", unique_db_name))
        .execute(&admin_pool)
        .await?;

    // Now connect to our newly created database
    let database_url = format!("postgresql://localhost/{}?sslmode=disable", unique_db_name);
    let pool = PgPoolOptions::new().connect(&database_url).await?;

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

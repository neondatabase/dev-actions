use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use anyhow::Result;

/// Helper to create a test database connection
/// Uses TEST_DATABASE_URL env var if set, otherwise defaults to local PostgreSQL
/// with current system user (PostgreSQL will automatically use the current user when no username is specified)
pub async fn create_test_db_connection() -> Result<Pool<Postgres>> {
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/deploy_queue_test?sslmode=disable".to_string());

    let pool = PgPoolOptions::new()
        .connect(&database_url)
        .await?;

    Ok(pool)
}

/// Helper to set up test database with migrations
pub async fn setup_test_db() -> Result<Pool<Postgres>> {
    let pool = create_test_db_connection().await?;
    
    // Run migrations - they're idempotent so safe to run multiple times
    // This will also insert the default 'dev' and 'prod' environments
    sqlx::migrate!()
        .set_ignore_missing(true)
        .run(&pool)
        .await?;
        
    Ok(pool)
}

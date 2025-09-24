use anyhow::Result;
use sqlx::{Pool, Postgres};
use std::sync::atomic::{AtomicU32, Ordering};

/// Helper to create a test deployment record and return its ID  
/// Used by integration tests that need a simple deployment for testing application logic
pub async fn create_test_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);
    
    let record = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note) 
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id", 
        format!("test-region-{}", unique_id),
        format!("test-component-{}", unique_id),
        "dev",
        Some("v1.0.0".to_string()),
        Some("https://github.com/test".to_string()),
        Some("test deployment".to_string())
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}


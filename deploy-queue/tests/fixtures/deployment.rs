use anyhow::Result;
use sqlx::{Pool, Postgres};
use std::sync::atomic::{AtomicU32, Ordering};

/// Helper to create a test deployment record and return its ID  
/// Used by integration tests that need a simple deployment for testing application logic
#[allow(dead_code)]
pub async fn create_test_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let record = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key) 
         VALUES ($1, $2, 'dev', 'v1.0.0', 'https://github.com/test', 'test deployment', NULL) RETURNING id", 
        format!("test-region-{}", unique_id),
        format!("test-component-{}", unique_id)
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}

/// Helper to create a test deployment record in running state and return its ID
/// Used by tests that need deployments that are already started
#[allow(dead_code)]
pub async fn create_running_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(1000); // Use different range to avoid ID conflicts
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let record = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key, start_timestamp) 
         VALUES ($1, $2, 'dev', 'v1.0.0', 'https://github.com/test-running', 'running test deployment', NULL, NOW()) RETURNING id", 
        format!("running-region-{}", unique_id),
        format!("running-component-{}", unique_id)
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}

/// Helper to create a test deployment record in finished state and return its ID
/// Used by tests that need deployments that are already finished
#[allow(dead_code)]
pub async fn create_finished_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(2000); // Use different range to avoid ID conflicts
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let record = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key, start_timestamp, finish_timestamp) 
         VALUES ($1, $2, 'dev', 'v1.0.0', 'https://github.com/test-finished', 'finished test deployment', NULL, NOW() - INTERVAL '10 minutes', NOW()) RETURNING id", 
        format!("finished-region-{}", unique_id),
        format!("finished-component-{}", unique_id)
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}

/// Helper to create a test deployment record in cancelled state and return its ID  
/// Used by tests that need deployments that are already cancelled
#[allow(dead_code)]
pub async fn create_cancelled_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(3000); // Use different range to avoid ID conflicts
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let record = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key, cancellation_timestamp, cancellation_note) 
         VALUES ($1, $2, 'dev', 'v1.0.0', 'https://github.com/test-cancelled', 'cancelled test deployment', NULL, NOW(), 'Test cancellation') RETURNING id", 
        format!("cancelled-region-{}", unique_id),
        format!("cancelled-component-{}", unique_id)
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}

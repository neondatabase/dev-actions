#![allow(unused)]

use anyhow::Result;
use sqlx::{Pool, Postgres};
use std::sync::atomic::{AtomicU32, Ordering};

/// Helper to create a test deployment record and return its ID
/// Used by integration tests that need a simple deployment for testing application logic
pub async fn create_test_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let record = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
        "dev",
        "aws",
        format!("test-region-{}", unique_id),
        1,
        format!("test-component-{}", unique_id),
        "v1.0.0",
        "https://github.com/test",
        "test deployment",
        None::<String>
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}

/// Helper to create a test deployment record in running state and return its ID
/// Used by tests that need deployments that are already started
pub async fn create_running_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(1000); // Use different range to avoid ID conflicts
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let record = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key, start_timestamp)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW()) RETURNING id",
        "dev",
        "aws",
        format!("running-region-{}", unique_id),
        1,
        format!("running-component-{}", unique_id),
        "v1.0.0",
        "https://github.com/test-running",
        "running test deployment",
        None::<String>
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}

/// Helper to create a test deployment record in finished state and return its ID
/// Used by tests that need deployments that are already finished
pub async fn create_finished_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(2000); // Use different range to avoid ID conflicts
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let record = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key, start_timestamp, finish_timestamp)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW() - INTERVAL '10 minutes', NOW()) RETURNING id",
        "dev",
        "aws",
        format!("finished-region-{}", unique_id),
        1,
        format!("finished-component-{}", unique_id),
        "v1.0.0",
        "https://github.com/test-finished",
        "finished test deployment",
        None::<String>
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}

/// Helper to create a test deployment record in cancelled state and return its ID
/// Used by tests that need deployments that are already cancelled
pub async fn create_cancelled_deployment(pool: &Pool<Postgres>) -> Result<i64> {
    static COUNTER: AtomicU32 = AtomicU32::new(3000); // Use different range to avoid ID conflicts
    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let record = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key, cancellation_timestamp, cancellation_note)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), 'Test cancellation') RETURNING id",
        "dev",
        "aws",
        format!("cancelled-region-{}", unique_id),
        1,
        format!("cancelled-component-{}", unique_id),
        "v1.0.0",
        "https://github.com/test-cancelled",
        "cancelled test deployment",
        None::<String>
    )
    .fetch_one(pool)
    .await?;

    Ok(record.id)
}

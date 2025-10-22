// This file contains focused tests for the blocking_deployments.sql query, with each test
// being self-contained and inserting only the specific data needed for its scenario.
//
// Each test can run in parallel since they use isolated database connections.

use anyhow::Result;

#[path = "common/test_db_setup.rs"]
mod database_helpers;

#[path = "fixtures/deployment.rs"]
mod deployment_fixtures;

extern crate deploy_queue;

use sqlx::{Pool, Postgres, Row};

/// Assert that a deployment is blocked by exactly the expected deployments
///
/// # Arguments
/// * `deployment_id` - The deployment to check for blocking
/// * `expected_blockers` - List of deployment IDs that should be blocking this deployment (empty = not blocked)
async fn assert_blocking_deployments(
    pool: &Pool<Postgres>,
    deployment_id: i64,
    expected_blockers: Vec<i64>,
) -> Result<()> {
    // Run the blocking deployments query
    let sql = include_str!("../queries/blocking_deployments.sql");
    let blocker_rows = sqlx::query(sql).bind(deployment_id).fetch_all(pool).await?;

    // Extract actual blocker IDs
    let mut actual_blockers: Vec<i64> = blocker_rows
        .iter()
        .map(|row| row.get::<i64, _>("id"))
        .collect();

    // Sort both vectors for consistent comparison
    actual_blockers.sort();
    let mut expected_sorted = expected_blockers.clone();
    expected_sorted.sort();

    // Assert they match
    assert_eq!(
        actual_blockers, expected_sorted,
        "Deployment {} blocking mismatch:\n  Expected blockers: {:?}\n  Actual blockers: {:?}",
        deployment_id, expected_sorted, actual_blockers
    );

    Ok(())
}

// Scenario 1: Simple blocking case
// aws, us-west-2, cell 1: deployment 1 should block deployment 2
#[tokio::test]
async fn test_blocked_by_running_component_same_region() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, url, note, start_timestamp) 
         VALUES 
             (1001, 'prod', 'aws', 'us-west-2', 1, 'api-server', 'v1.0.0', 'https://github.com/api-server/v1.0.0', 'Running deployment', NOW() - INTERVAL '5 minutes'),
             (1002, 'prod', 'aws', 'us-west-2', 1, 'web-frontend', 'v2.1.0', 'https://github.com/web-frontend/v2.1.0', 'Queued deployment', NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 9002 should be blocked by deployment 9001
    assert_blocking_deployments(&pool, 1002, vec![1001]).await?;

    Ok(())
}

// Scenario 2: Finished within buffer time (should still block)
// aws, us-east-1, cell 1: deployment finished 5 minutes ago, but prod has 10min buffer
#[tokio::test]
async fn test_blocked_by_finished_component_within_buffer_time() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, url, note, start_timestamp, finish_timestamp) 
         VALUES 
             (2003, 'prod', 'aws', 'us-east-1', 1, 'database-service', 'v1.2.0', 'https://github.com/db/v1.2.0', 'Recently finished', NOW() - INTERVAL '15 minutes', NOW() - INTERVAL '5 minutes'),
             (2004, 'prod', 'aws', 'us-east-1', 1, 'auth-service', 'v3.0.0', 'https://github.com/auth/v3.0.0', 'Blocked by buffer time', NULL, NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 2004 should be blocked by deployment 2003 (finished within buffer time)
    assert_blocking_deployments(&pool, 2004, vec![2003]).await?;

    Ok(())
}

// Scenario 3: Finished outside buffer time (should NOT block)
// aws, eu-west-1, cell 1: deployment finished 15 minutes ago, outside 10min buffer
#[tokio::test]
async fn test_not_blocked_by_finished_component_outside_buffer_time() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, url, note, start_timestamp, finish_timestamp) 
         VALUES 
             (3005, 'prod', 'aws', 'eu-west-1', 1, 'notification-service', 'v2.0.0', 'https://github.com/notifications/v2.0.0', 'Finished long ago', NOW() - INTERVAL '30 minutes', NOW() - INTERVAL '15 minutes'),
             (3006, 'prod', 'aws', 'eu-west-1', 1, 'payment-service', 'v1.5.0', 'https://github.com/payments/v1.5.0', 'Should not be blocked', NULL, NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 3006 should NOT be blocked (buffer time expired)
    assert_blocking_deployments(&pool, 3006, vec![]).await?;

    Ok(())
}

// Scenario 4a: Different regions (should NOT block each other)
#[tokio::test]
async fn test_not_blocked_by_running_component_different_region() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp) 
         VALUES 
             (4007, 'prod', 'aws', 'ap-southeast-1', 1, 'cache-service', 'v1.1.0', 'Running in APAC', NOW() - INTERVAL '10 minutes'),
             (4008, 'prod', 'aws', 'us-central-1', 1, 'cache-service', 'v1.1.0', 'Should not be blocked by APAC', NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 4008 should NOT be blocked (different region)
    assert_blocking_deployments(&pool, 4008, vec![]).await?;

    Ok(())
}

// Scenario 4b: Different environments (should NOT block each other)
#[tokio::test]
async fn test_not_blocked_by_running_component_different_environment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp) 
         VALUES 
             (4101, 'prod', 'aws', 'us-west-2', 1, 'api-service', 'v1.0.0', 'Running in prod', NOW() - INTERVAL '5 minutes'),
             (4102, 'dev', 'aws', 'us-west-2', 1, 'api-service', 'v1.0.0', 'Should not be blocked by prod', NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 4102 should NOT be blocked (different environment)
    assert_blocking_deployments(&pool, 4102, vec![]).await?;

    Ok(())
}

// Scenario 4c: Different cell_index (should NOT block each other)
#[tokio::test]
async fn test_not_blocked_by_running_component_different_cell_index() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp) 
         VALUES 
             (4201, 'prod', 'aws', 'us-west-2', 1, 'worker-service', 'v2.0.0', 'Running in cell 1', NOW() - INTERVAL '5 minutes'),
             (4202, 'prod', 'aws', 'us-west-2', 2, 'worker-service', 'v2.0.0', 'Should not be blocked by cell 1', NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 4202 should NOT be blocked (different cell_index)
    assert_blocking_deployments(&pool, 4202, vec![]).await?;

    Ok(())
}

// Scenario 4d: Different cloud_provider (should NOT block each other)
#[tokio::test]
async fn test_not_blocked_by_running_component_different_cloud_provider() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp) 
         VALUES 
             (4301, 'prod', 'aws', 'us-west-2', 1, 'database-service', 'v3.0.0', 'Running in AWS', NOW() - INTERVAL '5 minutes'),
             (4302, 'prod', 'azure', 'us-west-2', 1, 'database-service', 'v3.0.0', 'Should not be blocked by AWS', NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 4302 should NOT be blocked (different cloud_provider)
    assert_blocking_deployments(&pool, 4302, vec![]).await?;

    Ok(())
}

// Scenario 5: Cancelled deployment (should NOT block)
#[tokio::test]
async fn test_not_blocked_by_cancelled_deployment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp, cancellation_timestamp, cancellation_note) 
         VALUES 
             (5009, 'prod', 'aws', 'us-west-1', 1, 'monitoring-service', 'v2.2.0', 'Cancelled deployment', NOW() - INTERVAL '20 minutes', NOW() - INTERVAL '18 minutes', 'Cancelled due to critical bug'),
             (5010, 'prod', 'aws', 'us-west-1', 1, 'logging-service', 'v1.8.0', 'Should not be blocked by cancelled', NULL, NULL, NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 5010 should NOT be blocked (cancelled deployments don't block)
    assert_blocking_deployments(&pool, 5010, vec![]).await?;

    Ok(())
}

// Scenario 6: Dev environment (no buffer time)
#[tokio::test]
async fn test_not_blocked_in_dev_environment_no_buffer() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp, finish_timestamp) 
         VALUES 
             (6011, 'dev', 'aws', 'dev-cluster', 1, 'api-server', 'v1.1.0-beta', 'Dev deployment finished 5min ago', NOW() - INTERVAL '10 minutes', NOW() - INTERVAL '5 minutes'),
             (6012, 'dev', 'aws', 'dev-cluster', 1, 'web-frontend', 'v2.2.0-beta', 'Should not be blocked in dev (no buffer)', NULL, NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 6012 should NOT be blocked (dev environment has no buffer time)
    assert_blocking_deployments(&pool, 6012, vec![]).await?;

    Ok(())
}

// Scenario 7: Same concurrency key (should NOT block each other)
#[tokio::test]
async fn test_not_blocked_by_same_concurrency_key() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp, concurrency_key) 
         VALUES 
             (7013, 'prod', 'aws', 'us-west-3', 1, 'worker-service', 'v1.0.0', 'Part of concurrent deployment group', NOW() - INTERVAL '8 minutes', 'hotfix-2024-001'),
             (7014, 'prod', 'aws', 'us-west-3', 1, 'queue-processor', 'v1.0.1', 'Same concurrency key - should not block', NULL, 'hotfix-2024-001')"
    ).execute(&pool).await?;

    // Expect: deployment 7014 should NOT be blocked (same concurrency key allows parallel deployment)
    assert_blocking_deployments(&pool, 7014, vec![]).await?;

    Ok(())
}

// Scenario 8: Mixed concurrency keys (should block)
#[tokio::test]
async fn test_blocked_by_different_concurrency_keys() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp, concurrency_key) 
         VALUES 
             (8015, 'prod', 'aws', 'us-south-1', 1, 'user-service', 'v2.0.0', 'Different concurrency key', NOW() - INTERVAL '3 minutes', 'feature-2024-002'),
             (8016, 'prod', 'aws', 'us-south-1', 1, 'profile-service', 'v1.9.0', 'Different concurrency - should be blocked', NULL, 'feature-2024-003')"
    ).execute(&pool).await?;

    // Expect: deployment 8016 should be blocked by deployment 8015 (different concurrency keys)
    assert_blocking_deployments(&pool, 8016, vec![8015]).await?;

    Ok(())
}

// Scenario 9: NULL vs non-NULL concurrency keys (should block)
// ap-northeast-1 region: deployment with NULL concurrency key should block deployment with non-NULL key
#[tokio::test]
async fn test_null_vs_nonnull_concurrency_key_blocking() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp, concurrency_key) 
         VALUES 
             (9001, 'prod', 'aws', 'ap-northeast-1', 1, 'redis-service', 'v1.3.0', 'Running with NULL concurrency key', NOW() - INTERVAL '5 minutes', NULL),
             (9002, 'prod', 'aws', 'ap-northeast-1', 1, 'cache-service', 'v2.1.0', 'Queued with non-NULL concurrency key', NULL, 'performance-2024-001')"
    ).execute(&pool).await?;

    // Expect: deployment 9002 should be blocked by deployment 9001 (NULL vs non-NULL concurrency keys)
    assert_blocking_deployments(&pool, 9002, vec![9001]).await?;

    Ok(())
}

// Scenario 10: Sequential deployment blocking (running + queued)
// us-east-2 region: deployments block all subsequent deployments by ID order (both running and queued)
#[tokio::test]
async fn test_sequential_deployments_blocking_by_id_order() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, environment, cloud_provider, region, cell_index, component, version, note, start_timestamp) 
         VALUES 
             (10001, 'prod', 'aws', 'us-east-2', 1, 'api-gateway', 'v2.1.0', 'Running deployment - blocks all others', NOW() - INTERVAL '10 minutes'),
             (10002, 'prod', 'aws', 'us-east-2', 1, 'auth-service', 'v1.5.0', 'Queued - should be blocked', NULL),
             (10003, 'prod', 'aws', 'us-east-2', 1, 'user-service', 'v3.2.0', 'Queued - should be blocked', NULL),
             (10004, 'prod', 'aws', 'us-east-2', 1, 'notification-service', 'v1.8.0', 'Queued - should be blocked', NULL)"
    ).execute(&pool).await?;

    // Expect: queued deployments are blocked by all deployments with lower IDs (running + queued)
    assert_blocking_deployments(&pool, 10002, vec![10001]).await?; // blocked by running 10001
    assert_blocking_deployments(&pool, 10003, vec![10001, 10002]).await?; // blocked by running 10001 + queued 10002  
    assert_blocking_deployments(&pool, 10004, vec![10001, 10002, 10003]).await?; // blocked by running 10001 + queued 10002, 10003

    Ok(())
}

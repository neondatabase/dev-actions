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
// us-west-2 region: deployment 1 should block deployment 2
#[tokio::test]
async fn test_blocked_by_running_component_same_region() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, region, component, environment, version, url, note, start_timestamp) 
         VALUES 
             (1001, 'us-west-2', 'api-server', 'prod', 'v1.0.0', 'https://github.com/api-server/v1.0.0', 'Running deployment', NOW() - INTERVAL '5 minutes'),
             (1002, 'us-west-2', 'web-frontend', 'prod', 'v2.1.0', 'https://github.com/web-frontend/v2.1.0', 'Queued deployment', NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 9002 should be blocked by deployment 9001
    assert_blocking_deployments(&pool, 1002, vec![1001]).await?;

    Ok(())
}

// Scenario 2: Finished within buffer time (should still block)
// us-east-1 region: deployment finished 5 minutes ago, but prod has 10min buffer
#[tokio::test]
async fn test_blocked_by_finished_component_within_buffer_time() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, region, component, environment, version, url, note, start_timestamp, finish_timestamp) 
         VALUES 
             (2003, 'us-east-1', 'database-service', 'prod', 'v1.2.0', 'https://github.com/db/v1.2.0', 'Recently finished', NOW() - INTERVAL '15 minutes', NOW() - INTERVAL '5 minutes'),
             (2004, 'us-east-1', 'auth-service', 'prod', 'v3.0.0', 'https://github.com/auth/v3.0.0', 'Blocked by buffer time', NULL, NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 2004 should be blocked by deployment 2003 (finished within buffer time)
    assert_blocking_deployments(&pool, 2004, vec![2003]).await?;

    Ok(())
}

// Scenario 3: Finished outside buffer time (should NOT block)
// eu-west-1 region: deployment finished 15 minutes ago, outside 10min buffer
#[tokio::test]
async fn test_not_blocked_by_finished_component_outside_buffer_time() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, region, component, environment, version, url, note, start_timestamp, finish_timestamp) 
         VALUES 
             (3005, 'eu-west-1', 'notification-service', 'prod', 'v2.0.0', 'https://github.com/notifications/v2.0.0', 'Finished long ago', NOW() - INTERVAL '30 minutes', NOW() - INTERVAL '15 minutes'),
             (3006, 'eu-west-1', 'payment-service', 'prod', 'v1.5.0', 'https://github.com/payments/v1.5.0', 'Should not be blocked', NULL, NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 3006 should NOT be blocked (buffer time expired)
    assert_blocking_deployments(&pool, 3006, vec![]).await?;

    Ok(())
}

// Scenario 4: Different regions (should NOT block each other)
#[tokio::test]
async fn test_not_blocked_by_running_component_different_region() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, region, component, environment, version, note, start_timestamp) 
         VALUES 
             (4007, 'ap-southeast-1', 'cache-service', 'prod', 'v1.1.0', 'Running in APAC', NOW() - INTERVAL '10 minutes'),
             (4008, 'us-central-1', 'cache-service', 'prod', 'v1.1.0', 'Should not be blocked by APAC', NULL)"
    ).execute(&pool).await?;

    // Expect: deployment 4008 should NOT be blocked (different region)
    assert_blocking_deployments(&pool, 4008, vec![]).await?;

    Ok(())
}

// Scenario 5: Cancelled deployment (should NOT block)
#[tokio::test]
async fn test_not_blocked_by_cancelled_deployment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    sqlx::query!(
        "INSERT INTO deployments (id, region, component, environment, version, note, start_timestamp, cancellation_timestamp, cancellation_note) 
         VALUES 
             (5009, 'us-west-1', 'monitoring-service', 'prod', 'v2.2.0', 'Cancelled deployment', NOW() - INTERVAL '20 minutes', NOW() - INTERVAL '18 minutes', 'Cancelled due to critical bug'),
             (5010, 'us-west-1', 'logging-service', 'prod', 'v1.8.0', 'Should not be blocked by cancelled', NULL, NULL, NULL)"
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
        "INSERT INTO deployments (id, region, component, environment, version, note, start_timestamp, finish_timestamp) 
         VALUES 
             (6011, 'dev-cluster', 'api-server', 'dev', 'v1.1.0-beta', 'Dev deployment finished 5min ago', NOW() - INTERVAL '10 minutes', NOW() - INTERVAL '5 minutes'),
             (6012, 'dev-cluster', 'web-frontend', 'dev', 'v2.2.0-beta', 'Should not be blocked in dev (no buffer)', NULL, NULL)"
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
        "INSERT INTO deployments (id, region, component, environment, version, note, start_timestamp, concurrency_key) 
         VALUES 
             (7013, 'us-west-3', 'worker-service', 'prod', 'v1.0.0', 'Part of concurrent deployment group', NOW() - INTERVAL '8 minutes', 'hotfix-2024-001'),
             (7014, 'us-west-3', 'queue-processor', 'prod', 'v1.0.1', 'Same concurrency key - should not block', NULL, 'hotfix-2024-001')"
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
        "INSERT INTO deployments (id, region, component, environment, version, note, start_timestamp, concurrency_key) 
         VALUES 
             (8015, 'us-south-1', 'user-service', 'prod', 'v2.0.0', 'Different concurrency key', NOW() - INTERVAL '3 minutes', 'feature-2024-002'),
             (8016, 'us-south-1', 'profile-service', 'prod', 'v1.9.0', 'Different concurrency - should be blocked', NULL, 'feature-2024-003')"
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
        "INSERT INTO deployments (id, region, component, environment, version, note, start_timestamp, concurrency_key) 
         VALUES 
             (9001, 'ap-northeast-1', 'redis-service', 'prod', 'v1.3.0', 'Running with NULL concurrency key', NOW() - INTERVAL '5 minutes', NULL),
             (9002, 'ap-northeast-1', 'cache-service', 'prod', 'v2.1.0', 'Queued with non-NULL concurrency key', NULL, 'performance-2024-001')"
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
        "INSERT INTO deployments (id, region, component, environment, version, note, start_timestamp) 
         VALUES 
             (10001, 'us-east-2', 'api-gateway', 'prod', 'v2.1.0', 'Running deployment - blocks all others', NOW() - INTERVAL '10 minutes'),
             (10002, 'us-east-2', 'auth-service', 'prod', 'v1.5.0', 'Queued - should be blocked', NULL),
             (10003, 'us-east-2', 'user-service', 'prod', 'v3.2.0', 'Queued - should be blocked', NULL),
             (10004, 'us-east-2', 'notification-service', 'prod', 'v1.8.0', 'Queued - should be blocked', NULL)"
    ).execute(&pool).await?;

    // Expect: queued deployments are blocked by all deployments with lower IDs (running + queued)
    assert_blocking_deployments(&pool, 10002, vec![10001]).await?; // blocked by running 10001
    assert_blocking_deployments(&pool, 10003, vec![10001, 10002]).await?; // blocked by running 10001 + queued 10002  
    assert_blocking_deployments(&pool, 10004, vec![10001, 10002, 10003]).await?; // blocked by running 10001 + queued 10002, 10003

    Ok(())
}

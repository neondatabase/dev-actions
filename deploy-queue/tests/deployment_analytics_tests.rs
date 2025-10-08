use anyhow::Result;
use sqlx::postgres::types::PgInterval;
use sqlx::{Pool, Postgres};
use time::Duration;

#[path = "common/test_db_setup.rs"]
mod database_helpers;

extern crate deploy_queue;
use deploy_queue::{Deployment, finish_deployment, insert_deployment_record, start_deployment};

/// Convert time::Duration to PgInterval
fn duration_to_interval(duration: Duration) -> PgInterval {
    PgInterval {
        months: 0,
        days: 0,
        microseconds: duration.whole_microseconds() as i64,
    }
}

/// Convert PgInterval to time::Duration
fn interval_to_duration(interval: PgInterval) -> Duration {
    Duration::microseconds(interval.microseconds)
}

/// Calculate average and standard deviation from a slice of Durations
/// Returns (average, stddev) using sample standard deviation (n-1)
fn calculate_duration_stats(durations: &[Duration]) -> (Duration, Duration) {
    let total: Duration = durations.iter().sum();
    let avg = total / (durations.len() as i32);

    let variance: f64 = durations.iter()
        .map(|d| {
            let diff = *d - avg;
            let diff_secs = diff.whole_seconds() as f64;
            diff_secs * diff_secs
        })
        .sum::<f64>() / (durations.len() - 1) as f64;
    let stddev = Duration::seconds_f64(variance.sqrt());

    (avg, stddev)
}

/// Helper to create a finished deployment with specific timing and details
async fn create_finished_deployment_with_details(
    pool: &Pool<Postgres>,
    component: &str,
    region: &str,
    environment: &str,
    start_delay: Duration,
    duration: Duration,
    created_at_offset: Duration,
) -> Result<i64> {
    let deployment = Deployment {
        region: region.to_string(),
        component: component.to_string(),
        environment: environment.to_string(),
        ..Default::default()
    };

    // Insert with specific created_at if needed
    let deployment_id = if !created_at_offset.is_zero() {
        sqlx::query!(
            "INSERT INTO deployments (region, component, environment, concurrency_key, created_at) 
             VALUES ($1, $2, $3, $4, NOW() + $5) RETURNING id",
            deployment.region,
            deployment.component,
            deployment.environment,
            deployment.concurrency_key,
            duration_to_interval(created_at_offset)
        )
        .fetch_one(pool)
        .await?
        .id
    } else {
        insert_deployment_record(pool, deployment).await?
    };

    // Start the deployment with specific timing
    sqlx::query!(
        "UPDATE deployments SET start_timestamp = NOW() + $1 WHERE id = $2",
        duration_to_interval(start_delay),
        deployment_id
    )
    .execute(pool)
    .await?;

    // Finish the deployment with specific duration
    sqlx::query!(
        "UPDATE deployments SET finish_timestamp = start_timestamp + $1 WHERE id = $2",
        duration_to_interval(duration),
        deployment_id
    )
    .execute(pool)
    .await?;

    Ok(deployment_id)
}

/// Helper to create a cancelled deployment (should not appear in analytics)
async fn create_cancelled_deployment_with_details(
    pool: &Pool<Postgres>,
    component: &str,
    region: &str,
    environment: &str,
) -> Result<i64> {
    let deployment = Deployment {
        region: region.to_string(),
        component: component.to_string(),
        environment: environment.to_string(),
        ..Default::default()
    };

    let deployment_id = insert_deployment_record(pool, deployment).await?;

    // Cancel the deployment
    sqlx::query!(
        "UPDATE deployments SET cancellation_timestamp = NOW(), cancellation_note = 'Test cancellation' WHERE id = $1",
        deployment_id
    )
    .execute(pool)
    .await?;

    Ok(deployment_id)
}

#[tokio::test]
async fn test_basic_analytics_calculation() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "test-component";
    let region = "test-region";
    let environment = "dev";

    // Create deployments with known durations
    let durations = vec![
        Duration::seconds(60),
        Duration::seconds(120),
        Duration::seconds(180),
    ];
    for duration in &durations {
        create_finished_deployment_with_details(
            &pool,
            component,
            region,
            environment,
            Duration::ZERO,
            *duration,
            Duration::ZERO,
        )
        .await?;
    }

    // Calculate expected values
    let expected_count = durations.len() as i64;
    let total: Duration = durations.iter().sum();
    let expected_avg = total / (durations.len() as i32);

    // Calculate variance for stddev
    let variance: f64 = durations
        .iter()
        .map(|d| {
            let diff = *d - expected_avg;
            let diff_secs = diff.whole_seconds() as f64;
            diff_secs * diff_secs
        })
        .sum::<f64>()
        / (durations.len() - 1) as f64;
    let expected_stddev = Duration::seconds_f64(variance.sqrt());

    // Check analytics results (trigger should have refreshed the view)
    let row = sqlx::query!(
        "SELECT deployment_count, 
                avg_duration, 
                stddev_duration
         FROM deployment_duration_analytics 
         WHERE component = $1 AND region = $2 AND environment = $3",
        component,
        region,
        environment
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(row.deployment_count, Some(expected_count));

    // Convert PgInterval to Duration and compare
    let actual_avg =
        interval_to_duration(row.avg_duration.expect("avg_duration should not be null"));
    assert_eq!(
        actual_avg, expected_avg,
        "Expected average {:?}, got {:?}",
        expected_avg, actual_avg
    );

    let actual_stddev = interval_to_duration(
        row.stddev_duration
            .expect("stddev_duration should not be null"),
    );
    assert_eq!(
        actual_stddev, expected_stddev,
        "Expected stddev {:?}, got {:?}",
        expected_stddev, actual_stddev
    );

    Ok(())
}

#[tokio::test]
async fn test_time_filtering_three_months() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "time-test-component";
    let region = "time-test-region";
    let environment = "dev";

    // Create deployments: one from 4 months ago (should be excluded), two recent (should be included)
    create_finished_deployment_with_details(
        &pool,
        component,
        region,
        environment,
        Duration::ZERO,
        Duration::seconds(60),
        Duration::days(-120),
    )
    .await?; // 4 months ago
    create_finished_deployment_with_details(
        &pool,
        component,
        region,
        environment,
        Duration::ZERO,
        Duration::seconds(120),
        Duration::days(-30),
    )
    .await?; // 1 month ago
    create_finished_deployment_with_details(
        &pool,
        component,
        region,
        environment,
        Duration::ZERO,
        Duration::seconds(180),
        Duration::ZERO,
    )
    .await?; // now

    // Should only see 2 deployments (the recent ones) - trigger should have refreshed the view
    let row = sqlx::query!(
        "SELECT deployment_count FROM deployment_duration_analytics 
         WHERE component = $1 AND region = $2 AND environment = $3",
        component,
        region,
        environment
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(row.deployment_count, Some(2));

    Ok(())
}

#[tokio::test]
async fn test_row_limiting_hundred_deployments() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "limit-test-component";
    let region = "limit-test-region";
    let environment = "dev";

    // Create 150 deployments - should only see most recent 100
    for i in 0..150 {
        create_finished_deployment_with_details(
            &pool,
            component,
            region,
            environment,
            Duration::ZERO,
            Duration::seconds(60 + i),
            Duration::ZERO,
        )
        .await?;
    }

    // Should only see 100 deployments - trigger should have refreshed the view
    let row = sqlx::query!(
        "SELECT deployment_count FROM deployment_duration_analytics 
         WHERE component = $1 AND region = $2 AND environment = $3",
        component,
        region,
        environment
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(row.deployment_count, Some(100));

    Ok(())
}

#[tokio::test]
async fn test_cancelled_deployments_excluded() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "cancel-test-component";
    let region = "cancel-test-region";
    let environment = "dev";

    // Create 2 successful deployments and 1 cancelled
    create_finished_deployment_with_details(
        &pool,
        component,
        region,
        environment,
        Duration::ZERO,
        Duration::seconds(60),
        Duration::ZERO,
    )
    .await?;
    create_finished_deployment_with_details(
        &pool,
        component,
        region,
        environment,
        Duration::ZERO,
        Duration::seconds(120),
        Duration::ZERO,
    )
    .await?;
    create_cancelled_deployment_with_details(&pool, component, region, environment).await?;

    // Should only see 2 deployments (cancelled one excluded) - trigger should have refreshed the view
    let row = sqlx::query!(
        "SELECT deployment_count FROM deployment_duration_analytics 
         WHERE component = $1 AND region = $2 AND environment = $3",
        component,
        region,
        environment
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(row.deployment_count, Some(2));

    Ok(())
}

#[tokio::test]
async fn test_grouping_by_component_region_environment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Create deployments for different groups
    create_finished_deployment_with_details(
        &pool,
        "comp1",
        "region1",
        "dev",
        Duration::ZERO,
        Duration::seconds(60),
        Duration::ZERO,
    )
    .await?;
    create_finished_deployment_with_details(
        &pool,
        "comp1",
        "region1",
        "prod",
        Duration::ZERO,
        Duration::seconds(120),
        Duration::ZERO,
    )
    .await?;
    create_finished_deployment_with_details(
        &pool,
        "comp1",
        "region2",
        "dev",
        Duration::ZERO,
        Duration::seconds(180),
        Duration::ZERO,
    )
    .await?;
    create_finished_deployment_with_details(
        &pool,
        "comp2",
        "region1",
        "dev",
        Duration::ZERO,
        Duration::seconds(240),
        Duration::ZERO,
    )
    .await?;

    // Should have 4 different groups - trigger should have refreshed the view
    let count = sqlx::query!("SELECT COUNT(*) as total FROM deployment_duration_analytics")
        .fetch_one(&pool)
        .await?;

    assert_eq!(count.total, Some(4));

    Ok(())
}

#[tokio::test]
async fn test_trigger_refreshes_on_deployment_finish() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "trigger-test-component";
    let region = "trigger-test-region";
    let environment = "dev";

    // Create a deployment and start it
    let deployment = Deployment {
        region: region.to_string(),
        component: component.to_string(),
        environment: environment.to_string(),
        ..Default::default()
    };

    let deployment_id = insert_deployment_record(&pool, deployment).await?;
    start_deployment(&pool, deployment_id).await?;

    // Verify baseline (should be empty - no deployments finished yet)
    let initial_count = sqlx::query!(
        "SELECT COUNT(*) as total FROM deployment_duration_analytics 
         WHERE component = $1 AND region = $2 AND environment = $3",
        component,
        region,
        environment
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(initial_count.total, Some(0));

    // Finish the deployment - this should trigger the view refresh via the trigger
    finish_deployment(&pool, deployment_id).await?;

    // Check that the view now contains the finished deployment (trigger should have refreshed it)
    let final_count = sqlx::query!(
        "SELECT deployment_count FROM deployment_duration_analytics 
         WHERE component = $1 AND region = $2 AND environment = $3",
        component,
        region,
        environment
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(final_count.deployment_count, Some(1));

    Ok(())
}

#[tokio::test]
async fn test_incomplete_deployments_excluded() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "incomplete-test-component";
    let region = "incomplete-test-region";
    let environment = "dev";

    // Create various incomplete deployments
    let deployment = Deployment {
        region: region.to_string(),
        component: component.to_string(),
        environment: environment.to_string(),
        ..Default::default()
    };

    // Queued deployment (no start_timestamp)
    insert_deployment_record(&pool, deployment.clone()).await?;

    // Running deployment (has start_timestamp but no finish_timestamp)
    let running_id = insert_deployment_record(&pool, deployment.clone()).await?;
    start_deployment(&pool, running_id).await?;

    // Complete deployment (should be included)
    let complete_id = insert_deployment_record(&pool, deployment).await?;
    start_deployment(&pool, complete_id).await?;
    finish_deployment(&pool, complete_id).await?;

    // Should only see 1 deployment (the complete one) - trigger should have refreshed the view
    let row = sqlx::query!(
        "SELECT deployment_count FROM deployment_duration_analytics 
         WHERE component = $1 AND region = $2 AND environment = $3",
        component,
        region,
        environment
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(row.deployment_count, Some(1));

    Ok(())
}

#[tokio::test]
async fn test_empty_results_when_no_deployments() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Should have no rows (view is created empty by migration)
    let count = sqlx::query!("SELECT COUNT(*) as total FROM deployment_duration_analytics")
        .fetch_one(&pool)
        .await?;

    assert_eq!(count.total, Some(0));

    Ok(())
}

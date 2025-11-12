use anyhow::Result;
use deploy_queue::handler;
use deploy_queue::model::Deployment;
use sqlx::postgres::types::PgInterval;
use sqlx::{Pool, Postgres};
use time::Duration;

#[path = "common/test_db_setup.rs"]
mod database_helpers;

extern crate deploy_queue;

/// Convert time::Duration to PgInterval
fn duration_to_interval(duration: Duration) -> PgInterval {
    PgInterval {
        months: 0,
        days: 0,
        microseconds: duration.whole_microseconds() as i64,
    }
}

/// Helper to create a finished deployment with specific timing
async fn create_finished_deployment(
    pool: &Pool<Postgres>,
    component: &str,
    region: &str,
    environment: &str,
    duration: Duration,
) -> Result<i64> {
    let deployment = Deployment {
        cell: deploy_queue::model::Cell {
            environment: environment.to_string(),
            cloud_provider: "aws".to_string(),
            region: region.to_string(),
            index: 1,
        },
        component: component.to_string(),
        ..Default::default()
    };

    let deployment_id = handler::enqueue_deployment(pool, deployment).await?;

    // Start and finish the deployment with specific duration
    sqlx::query!(
        "UPDATE deployments SET start_timestamp = NOW() WHERE id = $1",
        deployment_id
    )
    .execute(pool)
    .await?;

    sqlx::query!(
        "UPDATE deployments SET finish_timestamp = start_timestamp + $1 WHERE id = $2",
        duration_to_interval(duration),
        deployment_id
    )
    .execute(pool)
    .await?;

    Ok(deployment_id)
}

/// Helper to create a running deployment that started at a specific time in the past
async fn create_running_deployment(
    pool: &Pool<Postgres>,
    component: &str,
    region: &str,
    environment: &str,
    started_ago: Duration,
) -> Result<i64> {
    let deployment = Deployment {
        cell: deploy_queue::model::Cell {
            environment: environment.to_string(),
            cloud_provider: "aws".to_string(),
            region: region.to_string(),
            index: 1,
        },
        component: component.to_string(),
        ..Default::default()
    };

    let deployment_id = handler::enqueue_deployment(pool, deployment).await?;

    // Start the deployment in the past - use negative duration to add
    let negative_offset = duration_to_interval(-started_ago);
    sqlx::query!(
        "UPDATE deployments SET start_timestamp = NOW() + $1 WHERE id = $2",
        negative_offset as PgInterval,
        deployment_id
    )
    .execute(pool)
    .await?;

    Ok(deployment_id)
}

#[tokio::test]
async fn test_outlier_detection_basic() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "test-component";
    let region = "test-region";
    let environment = "dev";

    // Create historical deployments with average duration of ~120 seconds
    // avg = 120, stddev ≈ 20
    let durations = vec![
        Duration::seconds(100),
        Duration::seconds(110),
        Duration::seconds(120),
        Duration::seconds(130),
        Duration::seconds(140),
    ];

    for duration in durations {
        create_finished_deployment(&pool, component, region, environment, duration).await?;
    }

    // Create a running deployment that's taking way longer than expected
    // Should be flagged as outlier (running for 200 seconds > 120 + 2*20 = 160)
    let outlier_id = create_running_deployment(
        &pool,
        component,
        region,
        environment,
        Duration::seconds(200),
    )
    .await?;

    // Create a running deployment that's within normal range
    // Should NOT be flagged (running for 140 seconds < 160)
    create_running_deployment(
        &pool,
        component,
        region,
        environment,
        Duration::seconds(140),
    )
    .await?;

    // Get outliers
    let outliers = handler::fetch::outlier_deployments(&pool).await?;

    // Should only have one outlier
    assert_eq!(outliers.len(), 1);
    assert_eq!(outliers[0].id, outlier_id);
    assert_eq!(outliers[0].component, component);
    assert_eq!(outliers[0].region, region);
    assert_eq!(outliers[0].env, environment);

    Ok(())
}

#[tokio::test]
async fn test_no_outliers_when_all_within_range() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "normal-component";
    let region = "test-region";
    let environment = "dev";

    // Create historical deployments
    for duration in [100, 110, 120, 130, 140].iter() {
        create_finished_deployment(
            &pool,
            component,
            region,
            environment,
            Duration::seconds(*duration),
        )
        .await?;
    }

    // Create running deployments within normal range
    create_running_deployment(
        &pool,
        component,
        region,
        environment,
        Duration::seconds(120),
    )
    .await?;

    let outliers = handler::fetch::outlier_deployments(&pool).await?;
    assert_eq!(outliers.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_no_outliers_when_no_running_deployments() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "finished-component";
    let region = "test-region";
    let environment = "dev";

    // Only create finished deployments
    for duration in [100, 110, 120, 130, 140].iter() {
        create_finished_deployment(
            &pool,
            component,
            region,
            environment,
            Duration::seconds(*duration),
        )
        .await?;
    }

    let outliers = handler::fetch::outlier_deployments(&pool).await?;
    assert_eq!(outliers.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_no_outliers_when_no_analytics_data() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "new-component";
    let region = "new-region";
    let environment = "dev";

    // Create a running deployment with no historical data
    create_running_deployment(
        &pool,
        component,
        region,
        environment,
        Duration::seconds(1000),
    )
    .await?;

    // Without analytics data, can't determine outliers
    let outliers = handler::fetch::outlier_deployments(&pool).await?;
    assert_eq!(outliers.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_outliers_per_component_region_env() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Setup: comp1 in region1/dev has fast deployments
    for duration in [10, 15, 20, 25, 30].iter() {
        create_finished_deployment(
            &pool,
            "comp1",
            "region1",
            "dev",
            Duration::seconds(*duration),
        )
        .await?;
    }

    // Setup: comp1 in region2/dev has slow deployments
    for duration in [100, 110, 120, 130, 140].iter() {
        create_finished_deployment(
            &pool,
            "comp1",
            "region2",
            "dev",
            Duration::seconds(*duration),
        )
        .await?;
    }

    // Running deployment in region1 that's an outlier for region1 (> 20 + 2*~8 ≈ 36)
    let outlier1_id =
        create_running_deployment(&pool, "comp1", "region1", "dev", Duration::seconds(60)).await?;

    // Running deployment in region2 that's normal for region2
    create_running_deployment(&pool, "comp1", "region2", "dev", Duration::seconds(130)).await?;

    let outliers = handler::fetch::outlier_deployments(&pool).await?;

    // Only region1 deployment should be an outlier
    assert_eq!(outliers.len(), 1);
    assert_eq!(outliers[0].id, outlier1_id);
    assert_eq!(outliers[0].region, "region1");

    Ok(())
}

#[tokio::test]
async fn test_outliers_excludes_finished_deployments() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "test-component";
    let region = "test-region";
    let environment = "dev";

    // Create historical deployments (avg ~120 seconds)
    for duration in [100, 110, 120, 130, 140].iter() {
        create_finished_deployment(
            &pool,
            component,
            region,
            environment,
            Duration::seconds(*duration),
        )
        .await?;
    }

    // Create a deployment that took very long but is now finished
    create_finished_deployment(
        &pool,
        component,
        region,
        environment,
        Duration::seconds(300),
    )
    .await?;

    // Should have no outliers (finished deployment shouldn't count)
    let outliers = handler::fetch::outlier_deployments(&pool).await?;
    assert_eq!(outliers.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_outliers_excludes_cancelled_deployments() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "test-component";
    let region = "test-region";
    let environment = "dev";

    // Create historical deployments
    for duration in [100, 110, 120, 130, 140].iter() {
        create_finished_deployment(
            &pool,
            component,
            region,
            environment,
            Duration::seconds(*duration),
        )
        .await?;
    }

    // Create a running deployment and then cancel it
    let deployment = Deployment {
        cell: deploy_queue::model::Cell {
            environment: environment.to_string(),
            cloud_provider: "aws".to_string(),
            region: region.to_string(),
            index: 1,
        },
        component: component.to_string(),
        ..Default::default()
    };

    let deployment_id = handler::enqueue_deployment(&pool, deployment).await?;

    sqlx::query!(
        "UPDATE deployments SET start_timestamp = NOW() - INTERVAL '200 seconds' WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        "UPDATE deployments SET cancellation_timestamp = NOW() WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await?;

    // Should have no outliers (cancelled deployment shouldn't count)
    let outliers = handler::fetch::outlier_deployments(&pool).await?;
    assert_eq!(outliers.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_outliers_optional_fields_omitted() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "test-component";
    let region = "test-region";
    let environment = "dev";

    // Create historical deployments
    for duration in [10, 15, 20, 25, 30].iter() {
        create_finished_deployment(
            &pool,
            component,
            region,
            environment,
            Duration::seconds(*duration),
        )
        .await?;
    }

    // Create outlier without optional fields
    let outlier_id = create_running_deployment(
        &pool,
        component,
        region,
        environment,
        Duration::seconds(100),
    )
    .await?;

    let outliers = handler::fetch::outlier_deployments(&pool).await?;

    assert_eq!(outliers.len(), 1);
    assert_eq!(outliers[0].id, outlier_id);

    Ok(())
}

#[tokio::test]
async fn test_multiple_outliers() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let component = "test-component";
    let region = "test-region";
    let environment = "dev";

    // Create historical deployments (avg ~20 seconds)
    for duration in [10, 15, 20, 25, 30].iter() {
        create_finished_deployment(
            &pool,
            component,
            region,
            environment,
            Duration::seconds(*duration),
        )
        .await?;
    }

    // Create multiple outliers
    let outlier1_id = create_running_deployment(
        &pool,
        component,
        region,
        environment,
        Duration::seconds(100),
    )
    .await?;

    let outlier2_id = create_running_deployment(
        &pool,
        component,
        region,
        environment,
        Duration::seconds(150),
    )
    .await?;

    let outliers = handler::fetch::outlier_deployments(&pool).await?;

    assert_eq!(outliers.len(), 2);

    // Verify both outliers are present (order by id)
    let ids: Vec<i64> = outliers.iter().map(|o| o.id).collect();
    assert!(ids.contains(&outlier1_id));
    assert!(ids.contains(&outlier2_id));

    Ok(())
}

use anyhow::Result;
use sqlx::{Pool, Postgres, Row};

#[path = "common/test_db_setup.rs"]
mod database_helpers;

#[derive(sqlx::Type, Debug, PartialEq, Clone)]
#[sqlx(type_name = "deployment_status", rename_all = "lowercase")]
pub enum DeploymentStatus {
    Pending,
    Queued,
    Running,
    Buffering,
    Finished,
    Cancelled,
}

/// Helper to insert a deployment with specific fields
async fn insert_deployment(
    pool: &Pool<Postgres>,
    region: &str,
    component: &str,
    version: Option<&str>,
    environment: &str,
) -> Result<i64> {
    let result = sqlx::query(
        r#"
        INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
        RETURNING id
        "#,
    )
    .bind(environment)
    .bind("aws")
    .bind(region)
    .bind(1)
    .bind(component)
    .bind(version)
    .bind(format!("https://example.com/job/{}", region))
    .fetch_one(pool)
    .await?;

    Ok(result.get("id"))
}

/// Helper to start a deployment
async fn start_deployment(pool: &Pool<Postgres>, id: i64) -> Result<()> {
    sqlx::query("UPDATE deployments SET start_timestamp = NOW() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Helper to finish a deployment
async fn finish_deployment(pool: &Pool<Postgres>, id: i64) -> Result<()> {
    sqlx::query("UPDATE deployments SET finish_timestamp = NOW() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Helper to cancel a deployment
async fn cancel_deployment(pool: &Pool<Postgres>, id: i64) -> Result<()> {
    sqlx::query("UPDATE deployments SET cancellation_timestamp = NOW() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn test_prod_latest_deployments_basic() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert deployments with different versions
    let id1 = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    let id2 = insert_deployment(&pool, "us-west-1", "api", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id1).await?;
    finish_deployment(&pool, id1).await?;
    start_deployment(&pool, id2).await?;

    // Query the view
    let results = sqlx::query("SELECT * FROM prod_latest_deployments ORDER BY region")
        .fetch_all(&pool)
        .await?;

    assert_eq!(results.len(), 2);

    // Both regions should have the same version
    let region1: String = results[0].get("region");
    let version1: String = results[0].get("version");
    let status1: DeploymentStatus = results[0].get("component_status");

    assert_eq!(region1, "us-east-1");
    assert_eq!(version1, "v1.0.0");
    assert_eq!(status1, DeploymentStatus::Buffering); // Within buffer time

    let region2: String = results[1].get("region");
    let version2: String = results[1].get("version");
    let status2: DeploymentStatus = results[1].get("component_status");

    assert_eq!(region2, "us-west-1");
    assert_eq!(version2, "v1.0.0");
    assert_eq!(status2, DeploymentStatus::Running);

    Ok(())
}

#[tokio::test]
async fn test_prod_latest_deployments_pending_status() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert deployments where one region is behind
    // Insert v1.0.0 to us-west-1 first (lower ID)
    let id1 = insert_deployment(&pool, "us-west-1", "api", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id1).await?;
    finish_deployment(&pool, id1).await?;

    // Then insert v2.0.0 to us-east-1 (higher ID, so this is the "max" version)
    let id2 = insert_deployment(&pool, "us-east-1", "api", Some("v2.0.0"), "prod").await?;
    start_deployment(&pool, id2).await?;
    finish_deployment(&pool, id2).await?;

    // Query the view
    let results = sqlx::query(
        "SELECT * FROM prod_latest_deployments WHERE component = 'api' ORDER BY region",
    )
    .fetch_all(&pool)
    .await?;

    assert_eq!(results.len(), 2);

    // us-east-1 has the latest version (highest ID), should show actual status
    let region1: String = results[0].get("region");
    let max_version1: String = results[0].get("version");
    let status1: DeploymentStatus = results[0].get("component_status");

    assert_eq!(region1, "us-east-1");
    assert_eq!(max_version1, "v2.0.0");
    assert_eq!(status1, DeploymentStatus::Buffering);

    // us-west-1 is behind, should show 'pending' status
    let region2: String = results[1].get("region");
    let max_version2: String = results[1].get("version");
    let status2: DeploymentStatus = results[1].get("component_status");
    let url2: String = results[1].get("job_url");

    assert_eq!(region2, "us-west-1");
    assert_eq!(max_version2, "v2.0.0"); // Shows the max version, not the region version
    assert_eq!(status2, DeploymentStatus::Pending);
    assert_eq!(url2, ""); // URL should be empty when pending

    Ok(())
}

#[tokio::test]
async fn test_prod_latest_deployments_excludes_cancelled() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert a cancelled deployment
    let id1 = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    cancel_deployment(&pool, id1).await?;

    // Insert a non-cancelled deployment
    let id2 = insert_deployment(&pool, "us-east-1", "api", Some("v2.0.0"), "prod").await?;
    start_deployment(&pool, id2).await?;

    // Query the view
    let results = sqlx::query(
        "SELECT * FROM prod_latest_deployments WHERE component = 'api' AND region = 'us-east-1'",
    )
    .fetch_all(&pool)
    .await?;

    assert_eq!(results.len(), 1);
    let version: String = results[0].get("version");
    assert_eq!(version, "v2.0.0"); // Should use v2.0.0, not the cancelled v1.0.0

    Ok(())
}

#[tokio::test]
async fn test_prod_latest_deployments_excludes_null_versions() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert deployment without version
    let _id1 = insert_deployment(&pool, "us-east-1", "api", None, "prod").await?;

    // Insert deployment with version
    let id2 = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id2).await?;

    // Query the view
    let results = sqlx::query(
        "SELECT * FROM prod_latest_deployments WHERE component = 'api' AND region = 'us-east-1'",
    )
    .fetch_all(&pool)
    .await?;

    assert_eq!(results.len(), 1);
    let version: String = results[0].get("version");
    assert_eq!(version, "v1.0.0");

    Ok(())
}

#[tokio::test]
async fn test_prod_latest_deployments_uses_highest_id_for_max_version() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert v2.0.0 first (lower ID)
    let _id1 = insert_deployment(&pool, "us-east-1", "api", Some("v2.0.0"), "prod").await?;

    // Insert v1.0.0 second (higher ID) - this should be considered the "latest" version
    let id2 = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id2).await?;

    // Query the view
    let results = sqlx::query(
        "SELECT * FROM prod_latest_deployments WHERE component = 'api' AND region = 'us-east-1'",
    )
    .fetch_all(&pool)
    .await?;

    assert_eq!(results.len(), 1);
    let version: String = results[0].get("version");
    let status: DeploymentStatus = results[0].get("component_status");

    // Should use v1.0.0 as the max version (highest ID), not v2.0.0
    assert_eq!(version, "v1.0.0");
    assert_eq!(status, DeploymentStatus::Running);

    Ok(())
}

#[tokio::test]
async fn test_prod_current_deployments_excludes_finished() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert a finished deployment (outside buffer time)
    let id1 = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id1).await?;
    // Finish it long enough ago to be outside buffer time (prod buffer is 5 minutes)
    sqlx::query(
        "UPDATE deployments SET finish_timestamp = NOW() - INTERVAL '10 minutes' WHERE id = $1",
    )
    .bind(id1)
    .execute(&pool)
    .await?;

    // Insert a running deployment
    let id2 = insert_deployment(&pool, "us-east-1", "web", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id2).await?;

    // Query the view
    let results = sqlx::query("SELECT * FROM prod_current_deployments ORDER BY deployment_id")
        .fetch_all(&pool)
        .await?;

    // Should only show the running deployment, not the finished one
    assert_eq!(results.len(), 1);
    let component: String = results[0].get("component");
    let status: DeploymentStatus = results[0].get("current_status");
    assert_eq!(component, "web");
    assert_eq!(status, DeploymentStatus::Running);

    Ok(())
}

#[tokio::test]
async fn test_prod_current_deployments_includes_buffering() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert a deployment that finished recently (within buffer time)
    let id1 = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id1).await?;
    finish_deployment(&pool, id1).await?;

    // Query the view
    let results = sqlx::query("SELECT * FROM prod_current_deployments WHERE component = 'api'")
        .fetch_all(&pool)
        .await?;

    assert_eq!(results.len(), 1);
    let status: DeploymentStatus = results[0].get("current_status");
    assert_eq!(status, DeploymentStatus::Buffering);

    Ok(())
}

#[tokio::test]
async fn test_prod_current_deployments_shows_analytics() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert and finish some deployments to generate analytics
    for _ in 0..5 {
        let id = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
        start_deployment(&pool, id).await?;
        finish_deployment(&pool, id).await?;
    }

    // Insert a currently running deployment
    let id = insert_deployment(&pool, "us-east-1", "api", Some("v2.0.0"), "prod").await?;
    start_deployment(&pool, id).await?;

    // Query the view
    let results = sqlx::query("SELECT * FROM prod_current_deployments WHERE deployment_id = $1")
        .bind(id)
        .fetch_all(&pool)
        .await?;

    assert_eq!(results.len(), 1);
    let status: DeploymentStatus = results[0].get("current_status");

    assert_eq!(status, DeploymentStatus::Running);

    // Verify analytics columns exist and are not null (they're intervals now, not strings)
    use sqlx::postgres::types::PgInterval;
    let avg_duration: Option<PgInterval> = results[0].get("avg_duration");
    let outlier_duration: Option<PgInterval> = results[0].get("outlier_duration");
    assert!(avg_duration.is_some());
    assert!(outlier_duration.is_some());

    Ok(())
}

#[tokio::test]
async fn test_prod_finished_deployments_only_shows_finished() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert various deployment states
    let _id1 = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    // Queued (no start)

    let id2 = insert_deployment(&pool, "us-east-1", "web", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id2).await?;
    // Running

    let id3 = insert_deployment(&pool, "us-east-1", "worker", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id3).await?;
    finish_deployment(&pool, id3).await?;
    // Buffering (finished within buffer time)

    let id4 = insert_deployment(&pool, "us-east-1", "db", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id4).await?;
    // Finish it long ago (outside buffer time)
    sqlx::query(
        "UPDATE deployments SET finish_timestamp = NOW() - INTERVAL '10 minutes' WHERE id = $1",
    )
    .bind(id4)
    .execute(&pool)
    .await?;
    // Finished

    let id5 = insert_deployment(&pool, "us-east-1", "cache", Some("v1.0.0"), "prod").await?;
    cancel_deployment(&pool, id5).await?;
    // Cancelled

    // Query the view
    let results = sqlx::query("SELECT * FROM prod_finished_deployments ORDER BY deployment_id")
        .fetch_all(&pool)
        .await?;

    // Should only show id4 (the one that's finished outside buffer time)
    assert_eq!(results.len(), 1);
    let deployment_id: i64 = results[0].get("deployment_id");
    let component: String = results[0].get("component");
    let status: DeploymentStatus = results[0].get("current_status");

    assert_eq!(deployment_id, id4);
    assert_eq!(component, "db");
    assert_eq!(status, DeploymentStatus::Finished);

    Ok(())
}

#[tokio::test]
async fn test_prod_finished_deployments_excludes_cancelled() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert a finished deployment
    let id1 = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id1).await?;
    sqlx::query(
        "UPDATE deployments SET finish_timestamp = NOW() - INTERVAL '10 minutes' WHERE id = $1",
    )
    .bind(id1)
    .execute(&pool)
    .await?;

    // Insert a cancelled deployment (don't finish it - can't be both finished and cancelled)
    let id2 = insert_deployment(&pool, "us-east-1", "web", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, id2).await?;
    cancel_deployment(&pool, id2).await?;

    // Query the view
    let results = sqlx::query("SELECT * FROM prod_finished_deployments")
        .fetch_all(&pool)
        .await?;

    // Should only show the non-cancelled one
    assert_eq!(results.len(), 1);
    let deployment_id: i64 = results[0].get("deployment_id");
    assert_eq!(deployment_id, id1);

    Ok(())
}

#[tokio::test]
async fn test_views_only_show_prod_environment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Insert deployments in both environments
    let prod_id = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "prod").await?;
    start_deployment(&pool, prod_id).await?;

    let dev_id = insert_deployment(&pool, "us-east-1", "api", Some("v1.0.0"), "dev").await?;
    start_deployment(&pool, dev_id).await?;

    // Check prod_latest_deployments
    let latest_results = sqlx::query("SELECT * FROM prod_latest_deployments")
        .fetch_all(&pool)
        .await?;
    assert_eq!(latest_results.len(), 1);

    // Check prod_current_deployments
    let current_results = sqlx::query("SELECT * FROM prod_current_deployments")
        .fetch_all(&pool)
        .await?;
    assert_eq!(current_results.len(), 1);
    let current_id: i64 = current_results[0].get("deployment_id");
    assert_eq!(current_id, prod_id);

    Ok(())
}

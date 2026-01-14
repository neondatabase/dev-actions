use anyhow::Result;
use deploy_queue::{constants::HEARTBEAT_TIMEOUT, handler};
use time::{Duration as TimeDuration, OffsetDateTime};

#[path = "common/test_db_setup.rs"]
mod database_helpers;

#[path = "fixtures/deployment.rs"]
mod deployment_fixtures;

extern crate deploy_queue;

#[tokio::test]
async fn heartbeat_loop_sets_timestamp() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_test_deployment(&pool).await?;

    // Run the heartbeat and wait a few milliseconds (so it can write the timestamp)
    let heartbeat_pool = pool.clone();
    let handle = tokio::spawn(async move {
        handler::run_heartbeat_loop(&heartbeat_pool, deployment_id)
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Check that the heartbeat timestamp was set
    let (heartbeat_timestamp,): (Option<OffsetDateTime>,) =
        sqlx::query_as("SELECT heartbeat_timestamp FROM deployments WHERE id = $1")
            .bind(deployment_id)
            .fetch_one(&pool)
            .await?;

    assert!(
        heartbeat_timestamp.is_some(),
        "Heartbeat loop should set heartbeat_timestamp"
    );

    // Stop the heartbeat loop
    handle.abort();

    Ok(())
}

#[tokio::test]
async fn stale_heartbeat_detection_flags_old_running_deployments() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_running_deployment(&pool).await?;

    // Set heartbeat older than the timeout
    let stale_at =
        OffsetDateTime::now_utc() - TimeDuration::seconds(HEARTBEAT_TIMEOUT.as_secs() as i64 + 60);
    sqlx::query("UPDATE deployments SET heartbeat_timestamp = $1 WHERE id = $2")
        .bind(stale_at)
        .bind(deployment_id)
        .execute(&pool)
        .await?;

    // Should be returned as stale
    let stale = handler::fetch::stale_heartbeat_deployments(&pool, HEARTBEAT_TIMEOUT).await?;
    assert!(
        stale.iter().any(|d| d.id == deployment_id),
        "Deployment with stale heartbeat should be flagged"
    );

    // Make the heartbeat fresh and ensure it is no longer reported
    let fresh_at =
        OffsetDateTime::now_utc() - TimeDuration::seconds(HEARTBEAT_TIMEOUT.as_secs() as i64 - 60);
    sqlx::query("UPDATE deployments SET heartbeat_timestamp = $1 WHERE id = $2")
        .bind(fresh_at)
        .bind(deployment_id)
        .execute(&pool)
        .await?;

    let stale_again = handler::fetch::stale_heartbeat_deployments(&pool, HEARTBEAT_TIMEOUT).await?;
    assert!(
        !stale_again.iter().any(|d| d.id == deployment_id),
        "Deployment with fresh heartbeat should not be flagged"
    );

    Ok(())
}

#[tokio::test]
async fn stale_blocker_gets_cancelled_when_waiting_for_blockers() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Create a running deployment with a stale heartbeat that will block others
    let blocking = deployment_fixtures::create_running_deployment(&pool).await?;
    let stale_at =
        OffsetDateTime::now_utc() - TimeDuration::seconds(HEARTBEAT_TIMEOUT.as_secs() as i64 + 60);
    sqlx::query(
        "UPDATE deployments
         SET heartbeat_timestamp = $1
         WHERE id = $2",
    )
    .bind(stale_at)
    .bind(blocking)
    .execute(&pool)
    .await?;

    // Create a new deployment and check for blocking deployments
    let waiter = deployment_fixtures::create_test_deployment(&pool).await?;
    handler::wait_for_blocking_deployments(&pool, waiter).await?;

    // Verify the blocking deployment was cancelled with the expected note
    let (cancellation_timestamp, cancellation_note): (Option<OffsetDateTime>, Option<String>) =
        sqlx::query_as(
            "SELECT cancellation_timestamp, cancellation_note FROM deployments WHERE id = $1",
        )
        .bind(blocking)
        .fetch_one(&pool)
        .await?;

    assert!(
        cancellation_timestamp.is_some(),
        "Blocking deployment should be cancelled"
    );
    let note = cancellation_note.expect("cancellation_note should be set");
    assert!(
        note.contains(&format!("Cancelled by deployment {}", waiter)),
        "Cancellation note should mention the cancelling deployment id; got {note}"
    );

    Ok(())
}

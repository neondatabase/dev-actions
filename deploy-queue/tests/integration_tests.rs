use anyhow::Result;
use time::OffsetDateTime;

#[path = "common/test_db_setup.rs"]
mod database_helpers;

#[path = "fixtures/deployment.rs"]
mod deployment_fixtures;

// Import the functions we need from the main module
// For binaries, we need to compile them as a library for testing
extern crate deploy_queue;
use deploy_queue::{Deployment, insert_deployment_record, get_deployment_info, start_deployment, finish_deployment, cancel_deployment};

#[tokio::test]
async fn test_insert_deployment_record() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    
    let region = "insert-test-region".to_string();
    let component = "insert-test-component".to_string();
    let environment = "prod";
    let version = Some("v2.1.0".to_string());
    let url = Some("https://github.com/example/test".to_string());
    let note = Some("Integration test deployment".to_string());
    let concurrency_key = None;
    
    let deployment = Deployment {
        region: region.clone(),
        component: component.clone(),
        environment: environment.to_string(),
        version: version.clone(),
        url: url.clone(),
        note: note.clone(),
        concurrency_key,
        ..Default::default()
    };
    
    // Test the insert function
    let deployment_id = insert_deployment_record(&pool, deployment).await?;
    
    // Verify it was inserted
    assert!(deployment_id > 0, "Deployment ID should be positive");

    let row = sqlx::query!(
        "SELECT id, region, component, environment, version, url, note, 
                start_timestamp, finish_timestamp, cancellation_timestamp, cancellation_note
         FROM deployments 
         WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    
    // Verify the fields were inserted correctly
    assert_eq!(row.id, deployment_id);
    assert_eq!(row.region, region);
    assert_eq!(row.component, component);
    assert_eq!(row.environment, environment);
    assert_eq!(row.version, version);
    assert_eq!(row.url, url);
    assert_eq!(row.note, note);
    
    // Timestamps should be None initially 
    assert!(row.start_timestamp.is_none());
    assert!(row.finish_timestamp.is_none());
    assert!(row.cancellation_timestamp.is_none());
    assert!(row.cancellation_note.is_none());
    
    // No cleanup needed - using unique test data
    Ok(())
}

#[tokio::test]
async fn test_insert_deployment_record_minimal_data() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    
    // Test with minimal required fields only
    let region = "minimal-test-region".to_string();
    let component = "minimal-test-component".to_string();
    let environment = "dev";
    
    let deployment = Deployment {
        region,
        component,
        environment: environment.to_string(),
        ..Default::default()
    };
    
    let deployment_id = insert_deployment_record(&pool, deployment).await?;
    assert!(deployment_id > 0);
    
    // Verify optional fields are None
    let row = sqlx::query!(
        "SELECT version, url, note FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    
    assert_eq!(row.version, None);
    assert_eq!(row.url, None);
    assert_eq!(row.note, None);
    
    Ok(())
}

#[tokio::test]
async fn test_get_deployment_info() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    
    let region = "get-info-test-region";
    let component = "get-info-test-component";
    let environment = "prod";
    let version = "v1.2.3";
    let url = "https://github.com/example/get-info";
    let note = "Test deployment for get_deployment_info";
    let concurrency_key = "test-key-123";
    
    // Insert test data
    let record = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key) 
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        region,
        component, 
        environment,
        Some(version),
        Some(url),
        Some(note),
        Some(concurrency_key)
    )
    .fetch_one(&pool)
    .await?;
    
    let deployment_id = record.id;
    
    // Test successful retrieval
    let retrieved = get_deployment_info(&pool, deployment_id).await?;
    assert!(retrieved.is_some(), "Should retrieve existing deployment");
    
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, deployment_id);
    assert_eq!(retrieved.region, region);
    assert_eq!(retrieved.component, component);
    assert_eq!(retrieved.environment, environment);
    assert_eq!(retrieved.version, Some(version.to_string()));
    assert_eq!(retrieved.url, Some(url.to_string()));
    assert_eq!(retrieved.note, Some(note.to_string()));
    assert_eq!(retrieved.concurrency_key, Some(concurrency_key.to_string()));
    assert_eq!(retrieved.buffer_time, 10); // prod environment has 10 minute buffer
    
    // Initially all timestamps should be None
    assert!(retrieved.start_timestamp.is_none());
    assert!(retrieved.finish_timestamp.is_none());
    assert!(retrieved.cancellation_timestamp.is_none());
    assert!(retrieved.cancellation_note.is_none());
    
    Ok(())
}

#[tokio::test]
async fn test_start_deployment_success() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_test_deployment(&pool).await?;

    // Initially, start_timestamp should be None
    let row = sqlx::query!(
        "SELECT start_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.start_timestamp.is_none());

    // Start the deployment
    start_deployment(&pool, deployment_id).await?;

    // Verify start_timestamp is now set
    let row = sqlx::query!(
        "SELECT start_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.start_timestamp.is_some());
    
    // Should be very recent (within last 10 seconds)
    let now = OffsetDateTime::now_utc();
    let started = row.start_timestamp.unwrap();
    let duration = now - started;
    let diff_seconds = duration.whole_seconds().abs();
    assert!(diff_seconds < 10, "Start timestamp should be recent, but was {} seconds ago", diff_seconds);

    // No cleanup needed - using unique test data
    Ok(())
}

#[tokio::test]
async fn test_finish_deployment_success() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_running_deployment(&pool).await?;

    // Initially, finish_timestamp should be None
    let row = sqlx::query!(
        "SELECT finish_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.finish_timestamp.is_none());

    // Finish the deployment
    finish_deployment(&pool, deployment_id).await?;

    // Verify finish_timestamp is now set
    let row = sqlx::query!(
        "SELECT finish_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.finish_timestamp.is_some());
    
    // Should be very recent (within last 10 seconds)
    let now = OffsetDateTime::now_utc();
    let finished = row.finish_timestamp.unwrap();
    let duration = now - finished;
    let diff_seconds = duration.whole_seconds().abs();
    assert!(diff_seconds < 10, "Finish timestamp should be recent, but was {} seconds ago", diff_seconds);

    // No cleanup needed - using unique test data
    Ok(())
}

#[tokio::test]
async fn test_cancel_queued_deployment_with_note() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_test_deployment(&pool).await?;

    // Cancel the deployment with a note
    let cancel_note = "Test cancellation";
    cancel_deployment(&pool, deployment_id, Some(cancel_note)).await?;

    // Verify cancellation fields are set
    let row = sqlx::query!(
        "SELECT cancellation_timestamp, cancellation_note FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.cancellation_timestamp.is_some());
    assert!(row.cancellation_note.is_some());
    assert_eq!(row.cancellation_note.as_ref().unwrap(), cancel_note);
    
    // Should be very recent (within last 10 seconds)
    let now = OffsetDateTime::now_utc();
    let cancelled = row.cancellation_timestamp.unwrap();
    let duration = now - cancelled;
    let diff_seconds = duration.whole_seconds().abs();
    assert!(diff_seconds < 10, "Cancellation timestamp should be recent, but was {} seconds ago", diff_seconds);

    // No cleanup needed - using unique test data
    Ok(())
}

#[tokio::test]
async fn test_cancel_running_deployment_without_note() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_running_deployment(&pool).await?;

    // Cancel the deployment without a note
    cancel_deployment(&pool, deployment_id, None).await?;

    // Verify cancellation timestamp is set but note is None
    let row = sqlx::query!(
        "SELECT cancellation_timestamp, cancellation_note FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.cancellation_timestamp.is_some());
    assert!(row.cancellation_note.is_none());

    // No cleanup needed - using unique test data
    Ok(())
}


#[tokio::test]
async fn test_deployment_state_transitions() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_test_deployment(&pool).await?;

    // Initial state: queued (no timestamps)
    let row = sqlx::query!(
        "SELECT start_timestamp, finish_timestamp, cancellation_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.start_timestamp.is_none());
    assert!(row.finish_timestamp.is_none());
    assert!(row.cancellation_timestamp.is_none());

    // Transition to running
    start_deployment(&pool, deployment_id).await?;
    let row = sqlx::query!(
        "SELECT start_timestamp, finish_timestamp, cancellation_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.start_timestamp.is_some());
    assert!(row.finish_timestamp.is_none());
    assert!(row.cancellation_timestamp.is_none());

    // Transition to finished
    finish_deployment(&pool, deployment_id).await?;
    let row = sqlx::query!(
        "SELECT start_timestamp, finish_timestamp, cancellation_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(row.start_timestamp.is_some());
    assert!(row.finish_timestamp.is_some());
    assert!(row.cancellation_timestamp.is_none());

    // No cleanup needed - using unique test data
    Ok(())
}

#[tokio::test]
async fn test_invalid_state_transitions() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    
    // Test finishing a deployment that was never started (queued → finished is invalid)
    let queued_deployment_id = deployment_fixtures::create_test_deployment(&pool).await?;
    let result = finish_deployment(&pool, queued_deployment_id).await;
    assert!(result.is_err(), "Should not be able to finish a queued deployment");
    
    Ok(())
}

#[tokio::test]
async fn test_operations_on_finished_deployment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    
    // Create a finished deployment
    let deployment_id = deployment_fixtures::create_finished_deployment(&pool).await?;
    
    // All further operations on finished deployment should fail
    let result = start_deployment(&pool, deployment_id).await;
    assert!(result.is_err(), "Should not be able to start a finished deployment");
    
    let result = finish_deployment(&pool, deployment_id).await;
    assert!(result.is_err(), "Should not be able to finish a finished deployment again");
    
    let result = cancel_deployment(&pool, deployment_id, Some("test")).await;
    assert!(result.is_err(), "Should not be able to cancel a finished deployment");
    
    Ok(())
}

#[tokio::test]
async fn test_operations_on_cancelled_deployment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    
    // Create a cancelled deployment
    let deployment_id = deployment_fixtures::create_cancelled_deployment(&pool).await?;
    
    // All further operations on cancelled deployment should fail
    let result = start_deployment(&pool, deployment_id).await;
    assert!(result.is_err(), "Should not be able to start a cancelled deployment");
    
    let result = finish_deployment(&pool, deployment_id).await;
    assert!(result.is_err(), "Should not be able to finish a cancelled deployment");
    
    let result = cancel_deployment(&pool, deployment_id, Some("test again")).await;
    assert!(result.is_err(), "Should not be able to cancel a cancelled deployment again");
    
    Ok(())
}

#[tokio::test]
async fn test_database_constraint_violations() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    
    // Test invalid environment value (should fail due to CHECK constraint)
    let result = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key) 
         VALUES ('test-region', 'test-component', 'invalid-env', 'v1.0.0', NULL, NULL, NULL)"
    )
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should not be able to insert deployment with invalid environment");
    
    // Test NULL required fields (should fail)
    let result = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key) 
         VALUES (NULL, 'test-component', 'dev', 'v1.0.0', NULL, NULL, NULL)"
    )
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should not be able to insert deployment with NULL region");
    
    let result = sqlx::query!(
        "INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key) 
         VALUES ('test-region', NULL, 'dev', 'v1.0.0', NULL, NULL, NULL)"
    )
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should not be able to insert deployment with NULL component");
    
    Ok(())
}
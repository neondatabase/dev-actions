use anyhow::Result;
use deploy_queue::{handler, model::Deployment};
use time::OffsetDateTime;

#[path = "common/test_db_setup.rs"]
mod database_helpers;

#[path = "fixtures/deployment.rs"]
mod deployment_fixtures;

// Import the functions we need from the main module
// For binaries, we need to compile them as a library for testing
extern crate deploy_queue;

#[tokio::test]
async fn test_insert_deployment_record() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    let environment = "prod";
    let cloud_provider = "aws";
    let region = "insert-test-region".to_string();
    let cell_index = 1;
    let component = "insert-test-component".to_string();
    let version = Some("v2.1.0".to_string());
    let url = Some("https://github.com/example/test".to_string());
    let note = Some("Integration test deployment".to_string());
    let concurrency_key = None;

    let deployment = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: region.clone(),
        cell_index,
        component: component.clone(),
        version: version.clone(),
        url: url.clone(),
        note: note.clone(),
        concurrency_key,
        ..Default::default()
    };

    // Test the insert function
    let deployment_id = handler::enqueue_deployment(&pool, deployment).await?;

    // Verify it was inserted
    assert!(deployment_id > 0, "Deployment ID should be positive");

    let row = sqlx::query!(
        "SELECT id, environment, cloud_provider, region, cell_index, component, version, url, note,
                start_timestamp, finish_timestamp, cancellation_timestamp, cancellation_note
         FROM deployments
         WHERE id = $1",
        deployment_id
    )
    .fetch_one(&pool)
    .await?;

    // Verify the fields were inserted correctly
    assert_eq!(row.id, deployment_id);
    assert_eq!(row.environment, environment);
    assert_eq!(row.cloud_provider, cloud_provider);
    assert_eq!(row.region, region);
    assert_eq!(row.cell_index, cell_index);
    assert_eq!(row.component, component);
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
    let environment = "dev";
    let cloud_provider = "aws";
    let region = "minimal-test-region".to_string();
    let cell_index = 1;
    let component = "minimal-test-component".to_string();

    let deployment = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region,
        cell_index,
        component,
        ..Default::default()
    };

    let deployment_id = handler::enqueue_deployment(&pool, deployment).await?;
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

    let environment = "prod";
    let cloud_provider = "aws";
    let region = "get-info-test-region";
    let cell_index = 1;
    let component = "get-info-test-component";
    let version = "v1.2.3";
    let url = "https://github.com/example/get-info";
    let note = "Test deployment for get_deployment_info";
    let concurrency_key = "test-key-123";

    // Insert test data
    let record = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
        environment,
        cloud_provider,
        region,
        cell_index,
        component,
        Some(version),
        Some(url),
        Some(note),
        Some(concurrency_key)
    )
    .fetch_one(&pool)
    .await?;

    let deployment_id = record.id;

    // Test successful retrieval
    let retrieved = handler::fetch::deployment(&pool, deployment_id).await?;
    assert!(retrieved.is_some(), "Should retrieve existing deployment");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, deployment_id);
    assert_eq!(retrieved.environment, environment);
    assert_eq!(retrieved.cloud_provider, cloud_provider);
    assert_eq!(retrieved.cell_index, cell_index);
    assert_eq!(retrieved.region, region);
    assert_eq!(retrieved.component, component);
    assert_eq!(retrieved.version, Some(version.to_string()));
    assert_eq!(retrieved.url, Some(url.to_string()));
    assert_eq!(retrieved.note, Some(note.to_string()));
    assert_eq!(retrieved.concurrency_key, Some(concurrency_key.to_string()));
    assert_eq!(retrieved.buffer_time, time::Duration::minutes(10)); // prod environment has 10 minute buffer

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
    handler::start_deployment(&pool, deployment_id).await?;

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
    assert!(
        diff_seconds < 10,
        "Start timestamp should be recent, but was {} seconds ago",
        diff_seconds
    );

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
    handler::finish_deployment(&pool, deployment_id).await?;

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
    assert!(
        diff_seconds < 10,
        "Finish timestamp should be recent, but was {} seconds ago",
        diff_seconds
    );

    // No cleanup needed - using unique test data
    Ok(())
}

#[tokio::test]
async fn test_cancel_queued_deployment_with_note() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_test_deployment(&pool).await?;

    // Cancel the deployment with a note
    let cancel_note = "Test cancellation";
    handler::cancel::deployment(&pool, deployment_id, Some(cancel_note)).await?;

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
    assert!(
        diff_seconds < 10,
        "Cancellation timestamp should be recent, but was {} seconds ago",
        diff_seconds
    );

    // No cleanup needed - using unique test data
    Ok(())
}

#[tokio::test]
async fn test_cancel_running_deployment_without_note() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;
    let deployment_id = deployment_fixtures::create_running_deployment(&pool).await?;

    // Cancel the deployment without a note
    handler::cancel::deployment(&pool, deployment_id, Option::<String>::None).await?;

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
    handler::start_deployment(&pool, deployment_id).await?;
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
    handler::finish_deployment(&pool, deployment_id).await?;
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
    let result = handler::finish_deployment(&pool, queued_deployment_id).await;
    assert!(
        result.is_err(),
        "Should not be able to finish a queued deployment"
    );

    Ok(())
}

#[tokio::test]
async fn test_operations_on_finished_deployment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Create a finished deployment
    let deployment_id = deployment_fixtures::create_finished_deployment(&pool).await?;

    // All further operations on finished deployment should fail
    let result = handler::start_deployment(&pool, deployment_id).await;
    assert!(
        result.is_err(),
        "Should not be able to start a finished deployment"
    );

    let result = handler::finish_deployment(&pool, deployment_id).await;
    assert!(
        result.is_err(),
        "Should not be able to finish a finished deployment again"
    );

    let result = handler::cancel::deployment(&pool, deployment_id, Some("test")).await;
    assert!(
        result.is_err(),
        "Should not be able to cancel a finished deployment"
    );

    Ok(())
}

#[tokio::test]
async fn test_operations_on_cancelled_deployment() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Create a cancelled deployment
    let deployment_id = deployment_fixtures::create_cancelled_deployment(&pool).await?;

    // All further operations on cancelled deployment should fail
    let result = handler::start_deployment(&pool, deployment_id).await;
    assert!(
        result.is_err(),
        "Should not be able to start a cancelled deployment"
    );

    let result = handler::finish_deployment(&pool, deployment_id).await;
    assert!(
        result.is_err(),
        "Should not be able to finish a cancelled deployment"
    );

    let result = handler::cancel::deployment(&pool, deployment_id, Some("test again")).await;
    assert!(
        result.is_err(),
        "Should not be able to cancel a cancelled deployment again"
    );

    Ok(())
}

#[tokio::test]
async fn test_database_constraint_violations() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Test invalid environment value (should fail due to CHECK constraint)
    let result = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key)
         VALUES ('invalid-env', 'aws', 'test-region', 1, 'test-component', 'v1.0.0', NULL, NULL, NULL)"
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to insert deployment with invalid environment"
    );

    // Test NULL required fields (should fail)
    let result = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key)
         VALUES ('dev', NULL, 'test-region', 1, 'test-component', 'v1.0.0', NULL, NULL, NULL)"
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to insert deployment with NULL cloud provider"
    );

    let result = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key)
         VALUES ('dev', 'aws', NULL, 1, 'test-component', 'v1.0.0', NULL, NULL, NULL)"
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to insert deployment with NULL region"
    );

    let result = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key)
         VALUES ('dev', 'aws', 'test-region', NULL, 'test-component', 'v1.0.0', NULL, NULL, NULL)"
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to insert deployment with NULL cell index"
    );

    let result = sqlx::query!(
        "INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key)
         VALUES ('dev', 'aws', 'test-region', 1, NULL, 'v1.0.0', NULL, NULL, NULL)"
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to insert deployment with NULL component"
    );

    Ok(())
}

#[tokio::test]
async fn test_immutable_fields_cannot_be_modified() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Create a test deployment
    let deployment_id = deployment_fixtures::create_test_deployment(&pool).await?;

    // Try to modify environment (should fail)
    let result = sqlx::query!(
        "UPDATE deployments SET environment = 'prod' WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to modify environment field"
    );
    if let Err(e) = result {
        let error_msg = e.to_string();
        assert!(
            error_msg.contains("Cannot modify immutable fields"),
            "Error should mention immutable fields, got: {}",
            error_msg
        );
    }

    // Try to modify cloud_provider (should fail)
    let result = sqlx::query!(
        "UPDATE deployments SET cloud_provider = 'azure' WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to modify cloud_provider field"
    );

    // Try to modify region (should fail)
    let result = sqlx::query!(
        "UPDATE deployments SET region = 'eu-west-1' WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should not be able to modify region field");

    // Try to modify cell_index (should fail)
    let result = sqlx::query!(
        "UPDATE deployments SET cell_index = 99 WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to modify cell_index field"
    );

    // Try to modify component (should fail)
    let result = sqlx::query!(
        "UPDATE deployments SET component = 'different-component' WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to modify component field"
    );

    // Try to modify version (should fail)
    let result = sqlx::query!(
        "UPDATE deployments SET version = 'v99.0.0' WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "Should not be able to modify version field"
    );

    // Try to modify url (should fail)
    let result = sqlx::query!(
        "UPDATE deployments SET url = 'https://different-url.com' WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should not be able to modify url field");

    // Try to modify note (should fail)
    let result = sqlx::query!(
        "UPDATE deployments SET note = 'different note' WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should not be able to modify note field");

    // Verify that mutable fields CAN still be modified
    // Try to set start_timestamp (should succeed)
    let result = sqlx::query!(
        "UPDATE deployments SET start_timestamp = NOW() WHERE id = $1",
        deployment_id
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_ok(),
        "Should be able to modify start_timestamp (mutable field)"
    );

    Ok(())
}

#[tokio::test]
async fn test_cancel_deployments_by_component_version() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Create test deployments for the same component/version across different regions
    let environment = "dev";
    let component = "test-component-cancel-cv";
    let version = "v1.0.0";

    // Create 3 deployments: 2 for aws, 1 for azure
    let deployment_1 = Deployment {
        environment: environment.to_string(),
        cloud_provider: "aws".to_string(),
        region: "us-east-1".to_string(),
        cell_index: 1,
        component: component.to_string(),
        version: Some(version.to_string()),
        ..Default::default()
    };

    let deployment_2 = Deployment {
        environment: environment.to_string(),
        cloud_provider: "aws".to_string(),
        region: "us-west-2".to_string(),
        cell_index: 2,
        component: component.to_string(),
        version: Some(version.to_string()),
        ..Default::default()
    };

    let deployment_3 = Deployment {
        environment: environment.to_string(),
        cloud_provider: "azure".to_string(),
        region: "east-us".to_string(),
        cell_index: 1,
        component: component.to_string(),
        version: Some(version.to_string()),
        ..Default::default()
    };

    // Create a deployment with different version (should NOT be cancelled)
    let deployment_4 = Deployment {
        environment: environment.to_string(),
        cloud_provider: "aws".to_string(),
        region: "us-east-1".to_string(),
        cell_index: 1,
        component: component.to_string(),
        version: Some("v2.0.0".to_string()),
        ..Default::default()
    };

    let id1 = handler::enqueue_deployment(&pool, deployment_1).await?;
    let id2 = handler::enqueue_deployment(&pool, deployment_2).await?;
    let id3 = handler::enqueue_deployment(&pool, deployment_3).await?;
    let id4 = handler::enqueue_deployment(&pool, deployment_4).await?;

    // Start one of them to make it running
    handler::start_deployment(&pool, id2).await?;

    // Cancel all deployments for this component/version/environment
    let cancellation_note = "Cancelling test-component-cancel-cv v1.0.0";
    let cancelled_count =
        handler::cancel::by_component_version(&pool, component, version, Some(cancellation_note))
            .await?;

    // Should have cancelled 3 deployments (id1, id2, id3)
    assert_eq!(cancelled_count, 3, "Should cancel 3 deployments");

    // Verify deployments 1, 2, 3 are cancelled
    for deployment_id in [id1, id2, id3] {
        let row = sqlx::query!(
            "SELECT cancellation_timestamp, cancellation_note FROM deployments WHERE id = $1",
            deployment_id
        )
        .fetch_one(&pool)
        .await?;
        assert!(
            row.cancellation_timestamp.is_some(),
            "Deployment {} should be cancelled",
            deployment_id
        );
        assert_eq!(
            row.cancellation_note.as_deref(),
            Some(cancellation_note),
            "Deployment {} should have cancellation note",
            deployment_id
        );
    }

    // Verify deployment 4 (different version) is NOT cancelled
    let row = sqlx::query!(
        "SELECT cancellation_timestamp FROM deployments WHERE id = $1",
        id4
    )
    .fetch_one(&pool)
    .await?;
    assert!(
        row.cancellation_timestamp.is_none(),
        "Deployment 4 (different version) should NOT be cancelled"
    );

    Ok(())
}

#[tokio::test]
async fn test_cancel_deployments_by_location() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Create test deployments in the same location across different components
    let environment = "dev";
    let cloud_provider = "aws";
    let region = "us-east-1";
    let cell_index = 5;

    // Create 3 deployments in the same location with different components
    let deployment_1 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: region.to_string(),
        cell_index,
        component: "compute-node".to_string(),
        version: Some("v1.0.0".to_string()),
        ..Default::default()
    };

    let deployment_2 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: region.to_string(),
        cell_index,
        component: "storage-controller".to_string(),
        version: Some("v2.0.0".to_string()),
        ..Default::default()
    };

    let deployment_3 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: region.to_string(),
        cell_index,
        component: "proxy".to_string(),
        version: Some("v3.0.0".to_string()),
        ..Default::default()
    };

    // Create a deployment in the same region but different cell_index (should NOT be cancelled)
    let deployment_4 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: region.to_string(),
        cell_index: 99,
        component: "compute-node".to_string(),
        version: Some("v1.0.0".to_string()),
        ..Default::default()
    };

    // Create a deployment in different region (should NOT be cancelled)
    let deployment_5 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: "us-west-2".to_string(),
        cell_index,
        component: "compute-node".to_string(),
        version: Some("v1.0.0".to_string()),
        ..Default::default()
    };

    let id1 = handler::enqueue_deployment(&pool, deployment_1).await?;
    let id2 = handler::enqueue_deployment(&pool, deployment_2).await?;
    let id3 = handler::enqueue_deployment(&pool, deployment_3).await?;
    let id4 = handler::enqueue_deployment(&pool, deployment_4).await?;
    let id5 = handler::enqueue_deployment(&pool, deployment_5).await?;

    // Start one to make it running
    handler::start_deployment(&pool, id1).await?;

    // Cancel all deployments in this location (with cell_index specified)
    let cancellation_note = "Cancelling all deployments in us-east-1 cell 5";
    let cancelled_count = handler::cancel::by_location(
        &pool,
        environment,
        cloud_provider,
        region,
        Some(cell_index),
        Some(cancellation_note),
    )
    .await?;

    // Should have cancelled 3 deployments (id1, id2, id3)
    assert_eq!(cancelled_count, 3, "Should cancel 3 deployments");

    // Verify deployments 1, 2, 3 are cancelled
    for deployment_id in [id1, id2, id3] {
        let row = sqlx::query!(
            "SELECT cancellation_timestamp, cancellation_note FROM deployments WHERE id = $1",
            deployment_id
        )
        .fetch_one(&pool)
        .await?;
        assert!(
            row.cancellation_timestamp.is_some(),
            "Deployment {} should be cancelled",
            deployment_id
        );
        assert_eq!(
            row.cancellation_note.as_deref(),
            Some(cancellation_note),
            "Deployment {} should have cancellation note",
            deployment_id
        );
    }

    // Verify deployment 4 (different cell) and 5 (different region) are NOT cancelled
    for deployment_id in [id4, id5] {
        let row = sqlx::query!(
            "SELECT cancellation_timestamp FROM deployments WHERE id = $1",
            deployment_id
        )
        .fetch_one(&pool)
        .await?;
        assert!(
            row.cancellation_timestamp.is_none(),
            "Deployment {} should NOT be cancelled",
            deployment_id
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_cancel_deployments_by_location_without_cell_index() -> Result<()> {
    let pool = database_helpers::setup_test_db().await?;

    // Create test deployments in the same region across different cells
    let environment = "dev";
    let cloud_provider = "aws";
    let region = "eu-west-1";

    // Create deployments in the same region but different cells
    let deployment_1 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: region.to_string(),
        cell_index: 1,
        component: "compute-node".to_string(),
        ..Default::default()
    };

    let deployment_2 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: region.to_string(),
        cell_index: 2,
        component: "storage-controller".to_string(),
        ..Default::default()
    };

    let deployment_3 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: region.to_string(),
        cell_index: 3,
        component: "proxy".to_string(),
        ..Default::default()
    };

    // Create a deployment in different region (should NOT be cancelled)
    let deployment_4 = Deployment {
        environment: environment.to_string(),
        cloud_provider: cloud_provider.to_string(),
        region: "ap-southeast-1".to_string(),
        cell_index: 1,
        component: "compute-node".to_string(),
        ..Default::default()
    };

    let id1 = handler::enqueue_deployment(&pool, deployment_1).await?;
    let id2 = handler::enqueue_deployment(&pool, deployment_2).await?;
    let id3 = handler::enqueue_deployment(&pool, deployment_3).await?;
    let id4 = handler::enqueue_deployment(&pool, deployment_4).await?;

    // Cancel all deployments in this region (without cell_index filter)
    let cancelled_count = handler::cancel::by_location(
        &pool,
        environment,
        cloud_provider,
        region,
        None,
        Option::<String>::None,
    )
    .await?;

    // Should have cancelled 3 deployments (id1, id2, id3) across all cells in the region
    assert_eq!(
        cancelled_count, 3,
        "Should cancel 3 deployments in the region"
    );

    // Verify deployments 1, 2, 3 are cancelled
    for deployment_id in [id1, id2, id3] {
        let row = sqlx::query!(
            "SELECT cancellation_timestamp FROM deployments WHERE id = $1",
            deployment_id
        )
        .fetch_one(&pool)
        .await?;
        assert!(
            row.cancellation_timestamp.is_some(),
            "Deployment {} should be cancelled",
            deployment_id
        );
    }

    // Verify deployment 4 (different region) is NOT cancelled
    let row = sqlx::query!(
        "SELECT cancellation_timestamp FROM deployments WHERE id = $1",
        id4
    )
    .fetch_one(&pool)
    .await?;
    assert!(
        row.cancellation_timestamp.is_none(),
        "Deployment 4 (different region) should NOT be cancelled"
    );

    Ok(())
}

use std::time::Duration;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use time::OffsetDateTime;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Mode, Environment};
use env_logger;
use log::info;
use tokio::time::sleep;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres, Error as SqlxError};

pub(crate) mod cli;

// We don't read all of the fields
#[allow(dead_code)]
struct Deployment {
    id: i64,
    region: String,
    environment: String,
    component: String, 
    url: Option<String>, 
    note: Option<String>, 
    start_timestamp: Option<OffsetDateTime>,
    finish_timestamp: Option<OffsetDateTime>,
    cancellation_timestamp: Option<OffsetDateTime>,
    cancellation_note: Option<String>,
    buffer_time: i32,
}

enum DeploymentState {
    Queued,
    Running,
    FinishedInBuffer,
    Finished,
    Cancelled
}

impl std::fmt::Display for DeploymentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentState::Queued => write!(f, "QUEUED"),
            DeploymentState::Running => write!(f, "RUNNING"),
            DeploymentState::FinishedInBuffer => write!(f, "FINISHED WITHIN BUFFER TIME"),
            DeploymentState::Finished => write!(f, "FINISHED"),
            DeploymentState::Cancelled => write!(f, "CANCELLED"),
        }
    }
}

impl From<&Deployment> for DeploymentState {
    fn from(deployment: &Deployment) -> Self {
        if deployment.start_timestamp.is_none() {
            DeploymentState::Queued
        } else if deployment.cancellation_timestamp.is_some() {
            DeploymentState::Cancelled
        } else if deployment.finish_timestamp.is_none() {
            DeploymentState::Running
        } else if deployment.finish_timestamp.is_some() && deployment.finish_timestamp.unwrap() < OffsetDateTime::now_utc() - Duration::from_minutes(deployment.buffer_time) {
            DeploymentState::FinishedInBuffer
        } else {
            DeploymentState::Finished
        }
    }
}

const FAILURE_RETRY: Duration = Duration::from_secs(2);
const BUSY_RETRY: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<()> {
    let log_env = env_logger::Env::default().filter_or("MUTEXBOT_LOG_LEVEL", "info");
    env_logger::Builder::from_env(log_env).init();
    let args = cli::Cli::parse();

    // Create a single database connection for all operations
    let db_client = create_db_connection().await?;

    match &args.mode {
        Mode::Start {
            region,
            component,
            environment,
            version,
            url,
            note,
        } => {

            // Insert deployment record into database
            let deployment_id = match insert_deployment_record(
                &db_client,
                region,
                component,
                environment.as_str(),
                version,
                url,
                note,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    log::error!("Failed to insert deployment record: {}", e);
                    anyhow::bail!("Database insertion failed: {}", e);
                }
            };

            loop {
                // Check for blocking deployments in the same region
                let blocking_deployments = check_blocking_deployments(&db_client, deployment_id).await?;
                
                if blocking_deployments.is_empty() {
                    info!("No blocking deployments found. Deployment can be started.");

                    // Update deployment record to set start_timestamp
                    match update_deployment_record(&db_client, deployment_id, DeploymentState::Running, None).await {
                        Ok(()) => {
                            info!("Successfully started deployment with ID: {}", deployment_id);
                            break;
                        }
                        Err(e) => {
                            log::error!("Failed to start deployment: {}", e);
                            anyhow::bail!("Database update failed: {}", e);
                        }
                    }
                } else {
                    // Print information about blocking deployments
                    info!("Found {} blocking deployment(s) with smaller queue positions:", 
                        blocking_deployments.len());
                    for pending_deployment in &blocking_deployments {
                        let deployment_state: DeploymentState = pending_deployment.into();
                        let deployment_note = pending_deployment.url.or(pending_deployment.note).unwrap_or_else(|| String::new());
                        info!("  - Deployment ID: {}, Component: {}, State: {}, Note: {}", pending_deployment.id, pending_deployment.component, deployment_state, deployment_note);
                    }
                    info!("Retrying in 5 seconds.");
                    sleep(BUSY_RETRY).await;
                }
            }
        }
        Mode::Finish { deployment_id } => {
            log::info!("Finishing deployment with ID: {}", deployment_id);

            // Verify deployment exists and is in a valid state for finishing
            match verify_deployment_can_be_finished(&db_client, *deployment_id).await {
                Ok(true) => {
                    // Update deployment record to set finish_timestamp
                    match update_deployment_record(&db_client, *deployment_id, DeploymentState::Finished, None).await {
                        Ok(()) => {
                            log::info!("Successfully finished deployment with ID: {}", deployment_id);
                        }
                        Err(e) => {
                            log::error!("Failed to update deployment record: {}", e);
                            anyhow::bail!("Database update failed: {}", e);
                        }
                    }
                }
                Ok(false) => {
                    anyhow::bail!("Deployment {} cannot be finished (not started or already finished/cancelled)", deployment_id);
                }
                Err(e) => {
                    log::error!("Failed to verify deployment state: {}", e);
                    anyhow::bail!("Database query failed: {}", e);
                }
            }
        }
        Mode::Cancel { deployment_id, cancellation_note } => {
            log::info!("Cancelling deployment with ID: {}", deployment_id);

            // Verify deployment exists and is in a valid state for cancellation
            match verify_deployment_can_be_cancelled(&db_client, *deployment_id).await {
                Ok(true) => {
                    // Update deployment record to set cancellation_timestamp
                    match update_deployment_record(&db_client, *deployment_id, DeploymentState::Cancelled, cancellation_note.as_deref()).await {
                        Ok(()) => {
                            log::info!("Successfully cancelled deployment with ID: {}", deployment_id);
                        }
                        Err(e) => {
                            log::error!("Failed to update deployment record: {}", e);
                            anyhow::bail!("Database update failed: {}", e);
                        }
                    }
                }
                Ok(false) => {
                    anyhow::bail!("Deployment {} cannot be cancelled (already finished or cancelled)", deployment_id);
                }
                Err(e) => {
                    log::error!("Failed to verify deployment state: {}", e);
                    anyhow::bail!("Database query failed: {}", e);
                }
            }
        }
    }

    Ok(())
}

/// Create a database connection pool and return it
async fn create_db_connection() -> Result<Pool<Postgres>, SqlxError> {
    let database_url = "postgres://user:password@hostname:port/db-name";

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await?;

    Ok(pool)
}

/// Insert a new deployment record into the PostgreSQL database and return the ID
async fn insert_deployment_record(
    client: &Pool<Postgres>,
    region: &str,
    component: &str,
    environment: &str,
    version: &str,
    url: &str,
    note: &str,
) -> Result<i64, SqlxError> {
    // Insert the deployment record and return the ID
    let record = sqlx::query!("INSERT INTO deployments (region, component, environment, version, url, note) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id", 
        region, component, environment, version, url, note)
        .fetch_one(client)
        .await?;
    
    let deployment_id = record.id;

    log::info!("Successfully inserted deployment record: id={}, region={}, component={}", deployment_id, region, component);

    // Store the deployment_id as a GitHub output if running in GitHub Actions
    if let Ok(github_output_path) = env::var("GITHUB_OUTPUT") {
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&github_output_path) 
        {
            Ok(mut file) => {
                if let Err(e) = writeln!(file, "deployment-id={}", deployment_id) {
                    log::warn!("Failed to write to GitHub output file: {}", e);
                } else {
                    log::info!("Successfully wrote deployment-id={} to GitHub output", deployment_id);
                }
            }
            Err(e) => {
                log::warn!("Failed to open GitHub output file '{}': {}", github_output_path, e);
            }
        }
    }

    Ok(deployment_id)
}

/// Verify that a deployment can be finished (must be started and not already finished/cancelled)
async fn verify_deployment_can_be_finished(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<bool, SqlxError> {
    let record = sqlx::query!(
        "SELECT start_timestamp, finish_timestamp, cancellation_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_optional(client)
    .await?;
    
    match record {
        Some(deployment) => {
            // Can finish if: started, not finished, and not cancelled
            Ok(deployment.start_timestamp.is_some() 
                && deployment.finish_timestamp.is_none() 
                && deployment.cancellation_timestamp.is_none())
        }
        None => {
            // Deployment doesn't exist
            Ok(false)
        }
    }
}

/// Verify that a deployment can be cancelled (must exist and not already finished/cancelled)
async fn verify_deployment_can_be_cancelled(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<bool, SqlxError> {
    let record = sqlx::query!(
        "SELECT finish_timestamp, cancellation_timestamp FROM deployments WHERE id = $1",
        deployment_id
    )
    .fetch_optional(client)
    .await?;
    
    match record {
        Some(deployment) => {
            // Can cancel if: not finished and not already cancelled
            Ok(deployment.finish_timestamp.is_none() && deployment.cancellation_timestamp.is_none())
        }
        None => {
            // Deployment doesn't exist
            Ok(false)
        }
    }
}

/// Check for blocking deployments in the same region 
async fn check_blocking_deployments(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<Vec<Deployment>, SqlxError> {
    // Query for deployments in the same region by other components with smaller ID (queue position)
    // that haven't finished yet (finish_timestamp IS NULL and cancellation_timestamp IS NULL) 
    // or have finished within the environment-specific buffer_time
    let results = sqlx::query_as!(
        Deployment,
        "SELECT d2.id, 
                d2.region,
                d2.environment,
                d2.component, 
                d2.url, 
                d2.note, 
                d2.start_timestamp,
                d2.finish_timestamp,
                d2.cancellation_timestamp,
                d2.cancellation_note,
                e.buffer_time
         FROM deployments d1
         JOIN environments e ON d1.environment = e.environment  
         JOIN deployments d2 ON (d1.region = d2.region AND d1.component != d2.component)
         WHERE d1.id = $1
           AND d2.id < d1.id
           AND (d2.finish_timestamp IS NULL 
                OR d2.finish_timestamp > NOW() - INTERVAL '1 minute' * e.buffer_time)
           AND d2.cancellation_timestamp IS NULL
         ORDER BY d2.id ASC",
        deployment_id
    )
    .fetch_all(client)
    .await?;

    Ok(results)
}

/// Update the deployment record with appropriate timestamp based on state
async fn update_deployment_record(
    client: &Pool<Postgres>,
    deployment_id: i64,
    state: DeploymentState,
    cancellation_note: Option<&str>,
) -> Result<(), SqlxError> {
    match state {
        DeploymentState::Running => {
            sqlx::query!("UPDATE deployments SET start_timestamp = NOW() WHERE id = $1", deployment_id)
                .execute(client)
                .await?;
        }
        DeploymentState::Finished => {
            sqlx::query!("UPDATE deployments SET finish_timestamp = NOW() WHERE id = $1", deployment_id)
                .execute(client)
                .await?;
        }
        DeploymentState::Cancelled => {
            sqlx::query!("UPDATE deployments SET cancellation_timestamp = NOW(), cancellation_note = $2 WHERE id = $1", deployment_id, cancellation_note)
                .execute(client)
                .await?;
        }
        DeploymentState::Queued => {
            // No timestamp update needed for queued state
        }
        DeploymentState::FinishedInBuffer => {
            // No timestamp update needed for finished in buffer state
        }
    }
    Ok(())
}

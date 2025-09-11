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

#[derive(sqlx::FromRow)]
// We don't read all of the fields
#[allow(dead_code)]
struct PendingDeployment {
    id: i64,
    component: String, 
    url: Option<String>, 
    note: Option<String>, 
    start_timestamp: Option<OffsetDateTime>
}

enum DeploymentState {
    Queued,
    Running,
    Finished,
    Cancelled
}

impl std::fmt::Display for DeploymentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentState::Queued => write!(f, "QUEUED"),
            DeploymentState::Running => write!(f, "RUNNING"),
            DeploymentState::Finished => write!(f, "FINISHED"),
            DeploymentState::Cancelled => write!(f, "CANCELLED"),
        }
    }
}

impl From<&PendingDeployment> for DeploymentState {
    fn from(pending_deployment: &PendingDeployment) -> Self {
        if pending_deployment.start_timestamp.is_some() {
            DeploymentState::Running
        } else {
            DeploymentState::Queued
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
    let db_client = match create_db_connection().await {
        Ok(client) => client,
        Err(e) => {
            log::error!("Failed to connect to database: {}", e);
            anyhow::bail!("Database connection failed: {}", e);
        }
    };

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
                match check_blocking_deployments(&db_client, deployment_id, component, region, environment.as_str()).await {
                    Ok(blocking_deployments) => {
                        if blocking_deployments.is_empty() {
                            info!("No blocking deployments found. Deployment can be started.");

                            //TO DO: Update deployment record to set start_timestamp
                            break;
                        } else {
                            // Print information about blocking deployments
                            info!("Found {} blocking deployment(s) in region '{}' with smaller queue positions:", 
                                blocking_deployments.len(), region);
                            for pending_deployment in &blocking_deployments {
                                let deployment_state: DeploymentState = pending_deployment.into();
                                let deployment_note = pending_deployment.url.or(pending_deployment.note).unwrap_or_else(|| String::new());
                                info!("  - Deployment ID: {}, Component: {}, State: {}, Note: {}", pending_deployment.id, pending_deployment.component, deployment_state, deployment_note);
                            }
                            info!("Retrying in 5 seconds.");
                            sleep(BUSY_RETRY).await;
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to check blocking deployments: {}", e);
                        anyhow::bail!("Database query failed: {}", e);
                    }
                }
            }
        }
        Mode::Release { .. } | Mode::ForceRelease { .. } => {
     
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

/// Get buffer time for an environment from the environments table
async fn get_environment_buffer_time(
    client: &Pool<Postgres>,
    environment: &str,
) -> Result<Duration, SqlxError> {
    let record = sqlx::query!("SELECT buffer_time FROM environments WHERE environment = $1", environment)
        .fetch_one(client)
        .await?;
    
    let buffer_minutes = record.buffer_time;
    Ok(Duration::from_secs(buffer_minutes as u64 * 60))
}

/// Check for blocking deployments in the same region
async fn check_blocking_deployments(
    client: &Pool<Postgres>,
    deployment_id: i64,
    component: &str,
    region: &str,
    environment: &str,
) -> Result<Vec<PendingDeployment>, SqlxError> {
    // Get the buffer time for this environment
    let buffer_time = get_environment_buffer_time(client, environment).await?;
    
    // Query for deployments in the same region by other components with smaller ID (queue position)
    // that haven't finished yet (finish_timestamp IS NULL and cancellation_timestamp IS NULL) 
    // or have finished within the environment-specific buffer_time
    let buffer_seconds = buffer_time.as_secs();
    let query = format!("
        SELECT id, component, url, note, start_timestamp
        FROM deployments 
        WHERE region = $1 
          AND component != $2 
          AND id < $3 
          AND (finish_timestamp IS NULL OR finish_timestamp > NOW() - INTERVAL '{} seconds')
          AND cancellation_timestamp IS NULL
        ORDER BY id ASC
    ", buffer_seconds);
    
    let results: Vec<PendingDeployment> = sqlx::query_as::<_, PendingDeployment>(&query)
        .bind(region)
        .bind(component)
        .bind(deployment_id)
        .fetch_all(client)
        .await?;

    Ok(results)
}

use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration as StdDuration;
use time::OffsetDateTime;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Mode;
use env_logger;
use log::info;
use tokio::time::sleep;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres, migrate::Migrator};

pub(crate) mod cli;

// Embed migrations into the binary
static MIGRATOR: Migrator = sqlx::migrate!();

// We don't read all of the fields
#[allow(dead_code)]
#[derive(Default)]
struct Deployment {
    id: i64,
    region: String,
    environment: String,
    component: String, 
    version: Option<String>,
    url: Option<String>, 
    note: Option<String>, 
    start_timestamp: Option<OffsetDateTime>,
    finish_timestamp: Option<OffsetDateTime>,
    cancellation_timestamp: Option<OffsetDateTime>,
    cancellation_note: Option<String>,
    concurrency_key: Option<String>,
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
        } else if deployment.finish_timestamp.is_some() && deployment.finish_timestamp.unwrap() < OffsetDateTime::now_utc() - time::Duration::minutes(deployment.buffer_time.into()) {
            DeploymentState::FinishedInBuffer
        } else {
            DeploymentState::Finished
        }
    }
}

const BUSY_RETRY: StdDuration = StdDuration::from_secs(5);

#[tokio::main]
async fn main() -> Result<()> {
    let log_env = env_logger::Env::default().filter_or("DEPLOY_QUEUE_LOG_LEVEL", "info");
    env_logger::Builder::from_env(log_env).init();
    let args = cli::Cli::parse();

    // Create a single database connection for all operations
    let db_client = create_db_connection().await?;

    // Run new migrations after connecting to DB
    run_migrations(&db_client).await?;

    match args.mode {
        Mode::Start {
            region,
            component,
            environment,
            version,
            url,
            note,
            concurrency_key,
        } => {

            // Insert deployment record into database
            let deployment_id = insert_deployment_record(
                &db_client,
                Deployment {
                   region,
                   component,
                   environment: environment.to_string(),
                   version,
                   url,
                   note,
                   concurrency_key,
                   ..Default::default()
                }
            ).await.context("Failed to enqueue new deployment")?;

            loop {
                // Check for blocking deployments in the same region
                let blocking_deployments = check_blocking_deployments(&db_client, deployment_id).await?;
                
                if blocking_deployments.is_empty() {
                    info!("No blocking deployments found. Deployment can be started.");

                    start_deployment(&db_client, deployment_id).await
                        .context("Failed to start deployment")?;
                    info!("Successfully started deployment with ID: {}", deployment_id);
                    break;
                } else {
                    // Print information about blocking deployments
                    info!("Found {} blocking deployment(s) with smaller queue positions:", 
                        blocking_deployments.len());
                    for pending_deployment in blocking_deployments {
                        info!(
                            " - {}", pending_deployment.summary()
                        );
                    }
                    info!("Retrying in 5 seconds.");
                    sleep(BUSY_RETRY).await;
                }
            }
        }
        Mode::Finish { deployment_id } => {
            log::info!("Finishing deployment with ID: {}", deployment_id);

            finish_deployment(&db_client, deployment_id).await
                .context("Failed to set deployment to finished")?;
            log::info!("Successfully finished deployment with ID: {}", deployment_id);
        }
        Mode::Cancel { deployment_id, cancellation_note } => {
            log::info!("Cancelling deployment with ID: {}", deployment_id);

            cancel_deployment(&db_client, deployment_id, cancellation_note.as_deref()).await
                .context("Failed to set deployment to cancelled")?;
            log::info!("Successfully cancelled deployment with ID: {}", deployment_id);
        }
        Mode::Info { deployment_id } => {
            log::info!("Fetching info for deployment ID: {}", deployment_id);

            show_deployment_info(&db_client, deployment_id).await
                .context("Failed to fetch deployment info")?;
        }
    }

    Ok(())
}

/// Create a database connection pool and return it
async fn create_db_connection() -> Result<Pool<Postgres>> {
    let database_url = std::env::var("DEPLOY_QUEUE_DATABASE_URL")
        .context("Failed to fetch database url from DEPLOY_QUEUE_DATABASE_URL environment variable")?;

    let pool = PgPoolOptions::new()
        .connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    Ok(pool)
}

/// Run database migrations
async fn run_migrations(pool: &Pool<Postgres>) -> Result<()> {
    info!("Running database migrations...");
    MIGRATOR.run(pool).await
        .context("Failed to run database migrations")?;
    info!("Database migrations completed successfully");
    Ok(())
}

/// Insert a new deployment record into the PostgreSQL database and return the ID
async fn insert_deployment_record(
    client: &Pool<Postgres>,
    deployment: Deployment,
) -> Result<i64> {
    // Insert the deployment record and return the ID
    let record = sqlx::query!("INSERT INTO deployments (region, component, environment, version, url, note) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id", 
        deployment.region, deployment.component, deployment.environment, deployment.version, deployment.url, deployment.note)
        .fetch_one(client)
        .await?;
    
    let deployment_id = record.id;

    log::info!("Successfully inserted deployment record: id={}, region={}, component={}", deployment_id, deployment.region, deployment.component);

    // Store the deployment_id as a GitHub output if running in GitHub Actions
    if let Ok(github_output_path) = env::var("GITHUB_OUTPUT") {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&github_output_path)
            .context("Failed to open GITHUB_OUTPUT")?; 
        writeln!(file, "deployment-id={}", deployment_id)
        	.context("Failed to write deployment_id to GITHUB_OUTPUT")?;
    }

    Ok(deployment_id)
}

impl Deployment {
    /// Generate a compact summary of this deployment's information
    fn summary(&self) -> String {
        let state_verb = DeploymentState::from(self).to_string().to_lowercase();
        
        let version = self.version.as_deref().unwrap_or("unknown");
        let note = self.note.as_deref().unwrap_or("");
        let url = self.url.as_deref().unwrap_or("");
        
        format!("{} {} {}(@{}): ({}) ({})", 
                self.id, 
                state_verb, 
                self.component, 
                version,
                note, 
                url)
    }
}

/// Show detailed info about a deployment for debugging purposes
async fn show_deployment_info(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<()> {
    let deployment: Option<Deployment> = sqlx::query_as!(
        Deployment,
        "SELECT d.id, 
                d.region,
                d.environment,
                d.component, 
                d.version,
                d.url, 
                d.note, 
                d.start_timestamp,
                d.finish_timestamp,
                d.cancellation_timestamp,
                d.cancellation_note,
                d.concurrency_key,
                e.buffer_time
         FROM deployments d
         JOIN environments e ON d.environment = e.environment  
         WHERE d.id = $1",
        deployment_id
    )
    .fetch_optional(client)
    .await?;

    match deployment {
        Some(dep) => {
            println!("{}", dep.summary());
            println!("{dep:#?}");
        },
        None => println!("Deployment with ID {} not found", deployment_id),
    }
    
    Ok(())
}

/// Check for blocking deployments in the same region 
async fn check_blocking_deployments(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<Vec<Deployment>> {
    // Query for deployments in the same region by other components with smaller ID (queue position)
    // that haven't finished yet (finish_timestamp IS NULL and cancellation_timestamp IS NULL) 
    // or have finished within the environment-specific buffer_time
    let results = sqlx::query_as!(
        Deployment,
        "SELECT d2.id,
                d2.region,
                d2.environment,
                d2.component,
                d2.version,
                d2.url,
                d2.note,
                d2.start_timestamp,
                d2.finish_timestamp,
                d2.cancellation_timestamp,
                d2.cancellation_note,
                d2.concurrency_key,
                e.buffer_time
         FROM
           (SELECT *
            FROM deployments
            WHERE id = $1) d1
         JOIN environments e ON d1.environment = e.environment
         JOIN deployments d2 ON (d1.region = d2.region
                                 AND (
                                   d1.concurrency_key IS NULL
                                   OR d2.concurrency_key IS NULL
                                   OR d1.concurrency_key != d2.concurrency_key
                                 )
                                 AND d2.id < d1.id
                                 AND (d2.finish_timestamp IS NULL
                                      OR d2.finish_timestamp > NOW() - INTERVAL '1 minute' * e.buffer_time)
                                 AND d2.cancellation_timestamp IS NULL)
         ORDER BY d2.id ASC",
        deployment_id
    )
    .fetch_all(client)
    .await?;

    Ok(results)
}

/// Update the deployment record with start timestamp
async fn start_deployment(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<()> {
    sqlx::query!("UPDATE deployments SET start_timestamp = NOW() WHERE id = $1", deployment_id)
        .execute(client)
        .await?;
    
    Ok(())
}

/// Update the deployment record with finish timestamp
async fn finish_deployment(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<()> {
    sqlx::query!("UPDATE deployments SET finish_timestamp = NOW() WHERE id = $1", deployment_id)
        .execute(client)
        .await?;
    
    Ok(())
}

/// Update the deployment record with cancellation timestamp and note
async fn cancel_deployment(
    client: &Pool<Postgres>,
    deployment_id: i64,
    cancellation_note: Option<&str>,
) -> Result<()> {
    sqlx::query!("UPDATE deployments SET cancellation_timestamp = NOW(), cancellation_note = $2 WHERE id = $1", deployment_id, cancellation_note)
        .execute(client)
        .await?;
    
    Ok(())
}

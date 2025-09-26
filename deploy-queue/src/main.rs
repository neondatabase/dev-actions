use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration as StdDuration;
use time::OffsetDateTime;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Mode;
use log::info;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use tokio::time::sleep;

pub(crate) mod cli;

// We don't read all of the fields
#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct Deployment {
    pub id: i64,
    pub region: String,
    pub environment: String,
    pub component: String,
    pub version: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
    pub start_timestamp: Option<OffsetDateTime>,
    pub finish_timestamp: Option<OffsetDateTime>,
    pub cancellation_timestamp: Option<OffsetDateTime>,
    pub cancellation_note: Option<String>,
    pub concurrency_key: Option<String>,
    pub buffer_time: i32,
}

enum DeploymentState {
    Queued,
    Running,
    FinishedInBuffer,
    Finished,
    Cancelled,
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
        } else if deployment.finish_timestamp.is_some()
            && deployment.finish_timestamp.unwrap()
                < OffsetDateTime::now_utc() - time::Duration::minutes(deployment.buffer_time.into())
        {
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
                },
            )
            .await
            .context("Failed to enqueue new deployment")?;

            loop {
                // Check for blocking deployments in the same region
                let blocking_deployments =
                    check_blocking_deployments(&db_client, deployment_id).await?;

                if blocking_deployments.is_empty() {
                    info!("No blocking deployments found. Deployment can be started.");

                    start_deployment(&db_client, deployment_id)
                        .await
                        .context("Failed to start deployment")?;
                    info!("Successfully started deployment with ID: {}", deployment_id);
                    break;
                } else {
                    // Print information about blocking deployments
                    info!(
                        "Found {} blocking deployment(s) with smaller queue positions:",
                        blocking_deployments.len()
                    );
                    for pending_deployment in blocking_deployments {
                        info!(" - {}", pending_deployment.summary());
                    }
                    info!("Retrying in 5 seconds.");
                    sleep(BUSY_RETRY).await;
                }
            }
        }
        Mode::Finish { deployment_id } => {
            log::info!("Finishing deployment with ID: {}", deployment_id);

            finish_deployment(&db_client, deployment_id)
                .await
                .context("Failed to set deployment to finished")?;
            log::info!(
                "Successfully finished deployment with ID: {}",
                deployment_id
            );
        }
        Mode::Cancel {
            deployment_id,
            cancellation_note,
        } => {
            log::info!("Cancelling deployment with ID: {}", deployment_id);

            cancel_deployment(&db_client, deployment_id, cancellation_note.as_deref())
                .await
                .context("Failed to set deployment to cancelled")?;
            log::info!(
                "Successfully cancelled deployment with ID: {}",
                deployment_id
            );
        }
        Mode::Info { deployment_id } => {
            log::info!("Fetching info for deployment ID: {}", deployment_id);

            show_deployment_info(&db_client, deployment_id)
                .await
                .context("Failed to fetch deployment info")?;
        }
    }

    Ok(())
}

/// Create a database connection pool and return it
async fn create_db_connection() -> Result<Pool<Postgres>> {
    let database_url = std::env::var("DEPLOY_QUEUE_DATABASE_URL").context(
        "Failed to fetch database url from DEPLOY_QUEUE_DATABASE_URL environment variable",
    )?;

    let pool = PgPoolOptions::new()
        .connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    Ok(pool)
}

/// Run database migrations
async fn run_migrations(pool: &Pool<Postgres>) -> Result<()> {
    info!("Running database migrations...");
    sqlx::migrate!()
        .set_ignore_missing(true)
        .run(pool)
        .await
        .context("Failed to run database migrations")?;
    info!("Database migrations completed successfully");
    Ok(())
}

/// Insert a new deployment record into the PostgreSQL database and return the ID
pub async fn insert_deployment_record(
    client: &Pool<Postgres>,
    deployment: Deployment,
) -> Result<i64> {
    // Insert the deployment record and return the ID
    let record = sqlx::query!("INSERT INTO deployments (region, component, environment, version, url, note, concurrency_key) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id", 
        deployment.region, deployment.component, deployment.environment, deployment.version, deployment.url, deployment.note, deployment.concurrency_key)
        .fetch_one(client)
        .await?;

    let deployment_id = record.id;

    log::info!(
        "Successfully inserted deployment record: id={}, region={}, component={}",
        deployment_id,
        deployment.region,
        deployment.component
    );

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
        let id = &self.id;
        let state_verb = DeploymentState::from(self).to_string().to_lowercase();
        let component = &self.component;
        let version = self
            .version
            .as_ref()
            .map(|version| format!("@{version}"))
            .unwrap_or_default();
        let note = self
            .note
            .as_ref()
            .map(|note| format!(" {note}"))
            .unwrap_or_default();
        let url = self
            .url
            .as_ref()
            .map(|url| format!(" {url}"))
            .unwrap_or_default();

        format!("{id} {state_verb} {component}{version}:{note}{url}")
    }
}

/// Fetch deployment information from the database
pub async fn get_deployment_info(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<Option<Deployment>> {
    let deployment = sqlx::query_as!(
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

    Ok(deployment)
}

/// Show detailed info about a deployment for debugging purposes
async fn show_deployment_info(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    let deployment = get_deployment_info(client, deployment_id).await?;

    match deployment {
        Some(dep) => {
            println!("{}", dep.summary());
            println!("{dep:#?}");
        }
        None => println!("Deployment with ID {} not found", deployment_id),
    }

    Ok(())
}

/// Check for blocking deployments in the same region
async fn check_blocking_deployments(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<Vec<Deployment>> {
    let results = sqlx::query_file_as!(
        Deployment,
        "queries/blocking_deployments.sql",
        deployment_id
    )
    .fetch_all(client)
    .await?;

    Ok(results)
}

/// Update the deployment record with start timestamp
pub async fn start_deployment(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    sqlx::query!(
        "UPDATE deployments SET start_timestamp = NOW() WHERE id = $1",
        deployment_id
    )
    .execute(client)
    .await?;

    Ok(())
}

/// Update the deployment record with finish timestamp
pub async fn finish_deployment(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    sqlx::query!(
        "UPDATE deployments SET finish_timestamp = NOW() WHERE id = $1",
        deployment_id
    )
    .execute(client)
    .await?;

    Ok(())
}

/// Update the deployment record with cancellation timestamp and note
pub async fn cancel_deployment(
    client: &Pool<Postgres>,
    deployment_id: i64,
    cancellation_note: Option<&str>,
) -> Result<()> {
    sqlx::query!("UPDATE deployments SET cancellation_timestamp = NOW(), cancellation_note = $2 WHERE id = $1", deployment_id, cancellation_note)
        .execute(client)
        .await?;

    Ok(())
}

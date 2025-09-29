use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration as StdDuration;
use time::OffsetDateTime;

use anyhow::{Context, Result};
use log::info;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use tokio::time::sleep;

pub mod cli;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeploymentState {
    Queued,
    Running,
    Finished,
    Cancelled,
}

impl std::fmt::Display for DeploymentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentState::Queued => write!(f, "queued"),
            DeploymentState::Running => write!(f, "running"),
            DeploymentState::Finished => write!(f, "finished"),
            DeploymentState::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl DeploymentState {
    pub fn from_timestamps(
        start: Option<OffsetDateTime>,
        finish: Option<OffsetDateTime>,
        cancel: Option<OffsetDateTime>,
    ) -> Self {
        match (start, finish, cancel) {
            (_, _, Some(_)) => DeploymentState::Cancelled,
            (_, Some(_), None) => DeploymentState::Finished,
            (Some(_), None, None) => DeploymentState::Running,
            (None, None, None) => DeploymentState::Queued,
        }
    }

    fn state_verb(&self) -> &'static str {
        match self {
            DeploymentState::Queued => "queued",
            DeploymentState::Running => "deploying",
            DeploymentState::Finished => "deployed",
            DeploymentState::Cancelled => "cancelled",
        }
    }
}

const BUSY_RETRY: StdDuration = StdDuration::from_secs(5);

pub async fn create_db_connection() -> Result<Pool<Postgres>> {
    let database_url = env::var("DEPLOY_QUEUE_DATABASE_URL")
        .context("DEPLOY_QUEUE_DATABASE_URL environment variable is not set")?;

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    Ok(pool)
}

pub async fn run_migrations(pool: &Pool<Postgres>) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("Failed to run database migrations")?;

    info!("Database migrations completed successfully");
    Ok(())
}

pub async fn insert_deployment_record(
    client: &Pool<Postgres>,
    deployment: Deployment,
) -> Result<i64> {
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
    Ok(deployment_id)
}

impl Deployment {
    /// Generate a compact summary of this deployment's information
    pub fn summary(&self) -> String {
        let state = DeploymentState::from_timestamps(
            self.start_timestamp,
            self.finish_timestamp,
            self.cancellation_timestamp,
        );
        let state_verb = state.state_verb();

        let mut summary = format!(
            "{} {} {}(@{})",
            self.id,
            state_verb,
            self.component,
            self.version.as_deref().unwrap_or("unknown")
        );

        if let Some(ref note) = self.note {
            summary.push_str(&format!(": ({})", note));
        }

        if let Some(ref url) = self.url {
            summary.push_str(&format!(" ({})", url));
        }

        summary
    }
}

pub async fn get_deployment_info(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<Option<Deployment>> {
    let row = sqlx::query!(
        r#"
        SELECT 
            d.id, d.region, d.component, d.environment, d.version, d.url, d.note, d.concurrency_key,
            d.start_timestamp, d.finish_timestamp, d.cancellation_timestamp, d.cancellation_note,
            e.buffer_time
        FROM deployments d
        JOIN environments e ON d.environment = e.environment
        WHERE d.id = $1
        "#,
        deployment_id
    )
    .fetch_optional(client)
    .await?;

    if let Some(row) = row {
        Ok(Some(Deployment {
            id: row.id,
            region: row.region,
            environment: row.environment,
            component: row.component,
            version: row.version,
            url: row.url,
            note: row.note,
            concurrency_key: row.concurrency_key,
            start_timestamp: row.start_timestamp,
            finish_timestamp: row.finish_timestamp,
            cancellation_timestamp: row.cancellation_timestamp,
            cancellation_note: row.cancellation_note,
            buffer_time: row.buffer_time,
        }))
    } else {
        Ok(None)
    }
}

async fn show_deployment_info(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    if let Some(deployment) = get_deployment_info(client, deployment_id).await? {
        println!("{}", deployment.summary());
    } else {
        println!("Deployment with ID {} not found", deployment_id);
    }
    Ok(())
}

async fn check_blocking_deployments(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<Vec<i64>> {
    let rows = sqlx::query_file!("queries/blocking_deployments.sql", deployment_id)
        .fetch_all(client)
        .await?;

    let blocking_ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    Ok(blocking_ids)
}

pub async fn start_deployment(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    sqlx::query!(
        "UPDATE deployments SET start_timestamp = NOW() WHERE id = $1",
        deployment_id
    )
    .execute(client)
    .await?;
    log::info!("Deployment {} has been started", deployment_id);
    Ok(())
}

pub async fn finish_deployment(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    sqlx::query!(
        "UPDATE deployments SET finish_timestamp = NOW() WHERE id = $1",
        deployment_id
    )
    .execute(client)
    .await?;
    log::info!("Deployment {} has been finished", deployment_id);
    Ok(())
}

pub async fn cancel_deployment(
    client: &Pool<Postgres>,
    deployment_id: i64,
    cancellation_note: Option<&str>,
) -> Result<()> {
    sqlx::query!("UPDATE deployments SET cancellation_timestamp = NOW(), cancellation_note = $2 WHERE id = $1", deployment_id, cancellation_note)
        .execute(client)
        .await?;
    log::info!("Deployment {} has been cancelled", deployment_id);
    Ok(())
}

pub async fn run_deploy_queue(mode: cli::Mode) -> Result<()> {
    // Create a single database connection for all operations
    let db_client = create_db_connection().await?;

    // Run new migrations after connecting to DB
    run_migrations(&db_client).await?;

    match mode {
        cli::Mode::Start {
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
                    region: region.clone(),
                    component: component.clone(),
                    environment: environment.to_string(),
                    version,
                    url,
                    note,
                    concurrency_key,
                    ..Default::default()
                },
            )
            .await?;

            // Write deployment ID to GitHub outputs
            if let Ok(github_output) = env::var("GITHUB_OUTPUT") {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(github_output)?;
                writeln!(file, "deployment_id={}", deployment_id)?;
            }

            // Check for conflicting deployments
            loop {
                let blocking_deployments =
                    check_blocking_deployments(&db_client, deployment_id).await?;

                if blocking_deployments.is_empty() {
                    info!("No conflicting deployments found. Starting deployment...");
                    break;
                } else {
                    info!(
                        "Found {} conflicting deployments: {:?}. Waiting {} seconds...",
                        blocking_deployments.len(),
                        blocking_deployments,
                        BUSY_RETRY.as_secs()
                    );
                    sleep(BUSY_RETRY).await;
                }
            }

            // Mark deployment as started
            start_deployment(&db_client, deployment_id).await?;

            info!("Deployment {} started successfully!", deployment_id);
        }
        cli::Mode::Finish { deployment_id } => {
            finish_deployment(&db_client, deployment_id).await?;
            info!("Deployment {} marked as finished", deployment_id);
        }
        cli::Mode::Cancel {
            deployment_id,
            cancellation_note,
        } => {
            cancel_deployment(&db_client, deployment_id, cancellation_note.as_deref()).await?;
            info!("Deployment {} cancelled", deployment_id);
        }
        cli::Mode::Info { deployment_id } => {
            show_deployment_info(&db_client, deployment_id).await?;
        }
    }

    Ok(())
}

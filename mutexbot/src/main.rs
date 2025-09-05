use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Mode;
use log::info;
use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tokio_postgres::{NoTls, Error as PgError};

pub(crate) mod cli;

struct State {
    http: Client,
    failure_count: usize,
}

impl State {
    fn new(api_key: &str) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        let mut auth_value = header::HeaderValue::from_str(api_key)
            .context("Failure creating auth header from API key")?;
        auth_value.set_sensitive(true);
        headers.insert("X-API-Key", auth_value);

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failure creating http client")?;

        Ok(Self {
            http,
            failure_count: 0,
        })
    }
    async fn request_failure(&mut self, error: reqwest::Error) -> Result<()> {
        if self.failure_count >= 15 {
            Err(error).context("Failed to send request!")?;
        }
        self.failure_count += 1;
        info!("Failed to send request! Retrying in 2 seconds...");
        sleep(FAILURE_RETRY).await;
        Ok(())
    }
    async fn status_code(&mut self, status_code: StatusCode) -> Result<()> {
        if !status_code.is_server_error() || self.failure_count >= 15 {
            anyhow::bail!("Unexpected status code: {status_code}")
        }
        self.failure_count += 1;
        info!("Server error, status code {status_code}. Retrying in 2 seconds...");
        sleep(FAILURE_RETRY).await;
        Ok(())
    }
}

#[derive(Serialize)]
struct ReservePayload {
    notes: String,
    duration: Option<String>,
    isolation_channel: Option<String>,
}

#[derive(Serialize)]
struct ReleasePayload {
    isolation_channel: Option<String>,
}

#[derive(Serialize)]
struct CreatePayload {
    name: String,
    isolation_channel: Option<String>,
}

#[derive(Deserialize)]
// We don't read all of the fields
#[allow(dead_code)]
struct ResourceListItem {
    name: String,
    description: String,
    isolated: bool,
    isolation_channel_name: Option<String>,
    active_reservation: Option<String>,
    active_reservation_user_name: Option<String>,
    active_reservation_reason: Option<String>,
}

const FAILURE_RETRY: Duration = Duration::from_secs(2);
const BUSY_RETRY: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<()> {
    let log_env = env_logger::Env::default().filter_or("MUTEXBOT_LOG_LEVEL", "info");
    env_logger::Builder::from_env(log_env).init();
    let args = cli::Cli::parse();
    let mut state = State::new(&args.api_key()?).context("Failure to initialize action state")?;

    match &args.mode {
        Mode::Reserve {
            duration,
            notes,
            resource_name,
        } => {
            let region = args.isolation_channel.clone().unwrap_or_else(|| "default".to_string());

            // Create a single database connection for all operations
            let db_client = match create_db_connection().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to connect to database: {}", e);
                    anyhow::bail!("Database connection failed: {}", e);
                }
            };

            // Insert deployment record into database
            let deployment_id = match insert_deployment_record(
                &db_client,
                notes,
                resource_name,
                &region,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    log::error!("Failed to insert deployment record: {}", e);
                    anyhow::bail!("Database insertion failed: {}", e);
                }
            };

            loop {
                // Check for blocking deployments in the same region
                match check_blocking_deployments(&db_client, deployment_id, resource_name, &region).await {
                    Ok(blocking_deployments) => {
                        if blocking_deployments.is_empty() {
                            info!("No blocking deployments found. Resource can be reserved.");

                            //TO DO: Update deployment record to set start_timestamp
                            break;
                        } else {
                            // Print information about blocking deployments
                            info!("Found {} blocking deployment(s) in region '{}' with smaller queue positions:", 
                                blocking_deployments.len(), region);
                            for (id, component) in &blocking_deployments {
                                info!("  - Deployment ID: {}, Component: {}", id, component);
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
            let payload = ReleasePayload {
                isolation_channel: args.isolation_channel,
            };
            loop {
                match state
                    .http
                    .post(args.mode.api_endpoint())
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(resp) => match resp.status() {
                        StatusCode::OK => {
                            info!("Resource released successfully.");
                            break;
                        }
                        StatusCode::ALREADY_REPORTED => {
                            anyhow::bail!("Resource not reserved, aborting.");
                        }
                        StatusCode::CONFLICT => {
                            anyhow::bail!("Resource by someone else, aborting.");
                        }
                        StatusCode::BAD_REQUEST => {
                            anyhow::bail!("Bad request. Check your input data.");
                        }
                        StatusCode::UNAUTHORIZED => {
                            anyhow::bail!("Unauthorized. Check your API keys.")
                        }
                        StatusCode::NOT_FOUND => {
                            anyhow::bail!("Resource not found.")
                        }
                        status_code => state
                            .status_code(status_code)
                            .await
                            .context("Failure releasing resource")?,
                    },
                    Err(error) => state
                        .request_failure(error)
                        .await
                        .context("Failure releasing resource")?,
                }
            }
        }
    }

    Ok(())
}

/// Create a database connection and return the client
async fn create_db_connection() -> Result<tokio_postgres::Client, PgError> {
    // Database connection configuration
    let db_config = "host=ep-tiny-bonus-a2qksa7f-pooler.eu-central-1.aws.neon.tech user=neondb_owner password=npg_RQzVs7DbYrU2 dbname=neondb";
    
    // Connect to the database
    let (client, connection) = tokio_postgres::connect(db_config, NoTls).await?;
    
    // Spawn the connection on a separate task
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("Database connection error: {}", e);
        }
    });
    
    Ok(client)
}

/// Insert a new deployment record into the PostgreSQL database and return the ID
async fn insert_deployment_record(
    client: &tokio_postgres::Client,
    note: &str,
    component: &str,
    region: &str,
) -> Result<i64, PgError> {
    // Insert the deployment record and return the ID
    let query = "INSERT INTO deployments (note, component, region) VALUES ($1, $2, $3) RETURNING id";
    
    let row = client.query_one(query, &[&note, &component, &region]).await?;
    let deployment_id: i64 = row.get(0);
    
    log::info!("Successfully inserted deployment record: id={}, component={}, region={}", deployment_id, component, region);
    
    Ok(deployment_id)
}

/// Check for blocking deployments in the same region
async fn check_blocking_deployments(
    client: &tokio_postgres::Client,
    deployment_id: i64,
    component: &str,
    region: &str,
) -> Result<Vec<(i64, String)>, PgError> {
    // Query for deployments in the same region by other components with smaller ID (queue position)
    // that haven't finished yet (finish_timestamp IS NULL)
    let query = "
        SELECT id, component 
        FROM deployments 
        WHERE region = $1 
          AND component != $2 
          AND id < $3 
          AND finish_timestamp IS NULL
        ORDER BY id ASC
    ";
    
    let rows = client.query(query, &[&region, &component, &deployment_id]).await?;
    
    let blocking_deployments: Vec<(i64, String)> = rows
        .into_iter()
        .map(|row| {
            let id: i64 = row.get(0);
            let component: String = row.get(1);
            (id, component)
        })
        .collect();
    
    Ok(blocking_deployments)
}

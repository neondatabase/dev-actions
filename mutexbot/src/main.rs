use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Mode;
use log::info;
use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

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
            let payload = ReservePayload {
                notes: notes.clone(),
                duration: duration.clone(),
                isolation_channel: args.isolation_channel.clone(),
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
                        StatusCode::CREATED => {
                            info!("Resource reserved successfully");
                            break;
                        }
                        StatusCode::CONFLICT => {
                            info!("Resource already reserved, fetching reservation data.");
                            match state
                                .http
                                .get("https://mutexbot.com/api/resources")
                                .send()
                                .await
                            {
                                Ok(resp) => match resp.status() {
                                    StatusCode::OK => {
                                        let resource = resp
                                            .json::<Vec<ResourceListItem>>()
                                            .await?
                                            .into_iter()
                                            .find(|resource| {
                                                &resource.name == resource_name
                                                    && (args.isolation_channel.is_none()
                                                        || (resource.isolated
                                                            && resource.isolation_channel_name
                                                                == args.isolation_channel))
                                            })
                                            .context("Could not find resource!")?;
                                        if resource.active_reservation.is_none() {
                                            info!("No active reservation.");
                                        } else {
                                            let user = resource.active_reservation_user_name.context("Resource doesn't have active_reservation_user_name!")?;
                                            let reason = resource.active_reservation_reason.context("Resource doesn't have active_reservation_reason!")?;

                                            if let Some(workflow_url) =
                                                reason.split_whitespace().last()
                                            {
                                                if workflow_url.contains("/actions/runs/") {
                                                    info!(
                                                        "Existing reservation by component {user} in {workflow_url}"
                                                    );
                                                } else {
                                                    info!(
                                                        "Existing reservation by user {user} with reason \"{reason}\""
                                                    );
                                                }
                                            } else {
                                                info!(
                                                    "Existing reservation by user {user} with reason \"{reason}\""
                                                );
                                            }
                                        }
                                    }
                                    StatusCode::BAD_REQUEST => {
                                        anyhow::bail!("Bad request. Check your input data.");
                                    }
                                    StatusCode::UNAUTHORIZED => {
                                        anyhow::bail!("Unauthorized. Check your API keys.");
                                    }
                                    status_code => state
                                        .status_code(status_code)
                                        .await
                                        .context("Failure creating missing resource")?,
                                },
                                Err(error) => state
                                    .request_failure(error)
                                    .await
                                    .context("Failure fetching resource data")?,
                            }
                            info!("Retrying in 5 seconds.");
                            sleep(BUSY_RETRY).await;
                        }
                        StatusCode::BAD_REQUEST => {
                            anyhow::bail!("Bad request. Check your input data.");
                        }
                        StatusCode::UNAUTHORIZED => {
                            anyhow::bail!("Unauthorized. Check your API keys.");
                        }
                        StatusCode::NOT_FOUND => {
                            info!("Resource not found.");
                            match state
                                .http
                                .post("https://mutexbot.com/api/resources")
                                .json(&CreatePayload {
                                    name: resource_name.clone(),
                                    isolation_channel: args.isolation_channel.clone(),
                                })
                                .send()
                                .await
                            {
                                Ok(resp) => match resp.status() {
                                    StatusCode::CREATED => {
                                        info!("Resource created")
                                    }
                                    StatusCode::CONFLICT => {
                                        info!("Resource already exists, trying again.");
                                    }
                                    StatusCode::BAD_REQUEST => {
                                        anyhow::bail!("Bad request. Check your input data.");
                                    }
                                    StatusCode::UNAUTHORIZED => {
                                        anyhow::bail!("Unauthorized. Check your API keys.");
                                    }
                                    status_code => state
                                        .status_code(status_code)
                                        .await
                                        .context("Failure creating missing resource")?,
                                },
                                Err(error) => state
                                    .request_failure(error)
                                    .await
                                    .context("Failure creating missing resource")?,
                            }
                        }
                        status_code => state
                            .status_code(status_code)
                            .await
                            .context("Failure reserving resource")?,
                    },

                    Err(error) => state
                        .request_failure(error)
                        .await
                        .context("Failure reserving resource")?,
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

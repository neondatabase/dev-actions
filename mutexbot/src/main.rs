use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use cli::Mode;
use log::info;
use rand::Rng;
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

impl State {
    /// Fetch resource data from the API and find a specific resource.
    /// Returns `Ok(None)` if the resource is not found.
    async fn fetch_resource_data(
        &mut self,
        resource_name: &str,
        isolation_channel: &Option<String>,
    ) -> Result<Option<ResourceListItem>> {
        match self.http.get("https://mutexbot.com/api/resources").send().await {
            Ok(resp) => match resp.status() {
                StatusCode::OK => {
                    let resources = resp.json::<Vec<ResourceListItem>>().await?;
                    Ok(resources.into_iter().find(|resource| {
                        &resource.name == resource_name
                            && (isolation_channel.is_none()
                                || (resource.isolated
                                    && resource.isolation_channel_name == *isolation_channel))
                    }))
                }
                StatusCode::BAD_REQUEST => {
                    anyhow::bail!("Bad request. Check your input data.");
                }
                StatusCode::UNAUTHORIZED => {
                    anyhow::bail!("Unauthorized. Check your API keys.");
                }
                status_code => {
                    self.status_code(status_code)
                        .await
                        .context("Failure fetching resource data")?;
                    Err(anyhow::anyhow!("Retry needed"))
                }
            },
            Err(error) => {
                self.request_failure(error)
                    .await
                    .context("Failure fetching resource data")?;
                Err(anyhow::anyhow!("Retry needed"))
            }
        }
    }

    /// Create a resource when it doesn't exist.
    async fn create_resource_if_missing(
        &mut self,
        resource_name: &str,
        isolation_channel: &Option<String>,
    ) -> Result<()> {
        info!("Resource not found, creating it.");
        match self
            .http
            .post("https://mutexbot.com/api/resources")
            .json(&CreatePayload {
                name: resource_name.to_string(),
                isolation_channel: isolation_channel.clone(),
            })
            .send()
            .await
        {
            Ok(resp) => match resp.status() {
                StatusCode::CREATED => {
                    info!("Resource created");
                    Ok(())
                }
                StatusCode::CONFLICT => {
                    info!("Resource already exists, trying again.");
                    Ok(())
                }
                StatusCode::BAD_REQUEST => {
                    anyhow::bail!("Bad request. Check your input data.");
                }
                StatusCode::UNAUTHORIZED => {
                    anyhow::bail!("Unauthorized. Check your API keys.");
                }
                status_code => {
                    self.status_code(status_code)
                        .await
                        .context("Failure creating missing resource")?;
                    Err(anyhow::anyhow!("Retry needed"))
                }
            },
            Err(error) => {
                self.request_failure(error)
                    .await
                    .context("Failure creating missing resource")?;
                Err(anyhow::anyhow!("Retry needed"))
            }
        }
    }



    /// Attempt to reserve a resource and handle common status codes.
    async fn attempt_reservation(
        &mut self,
        endpoint: &str,
        payload: &ReservePayload,
        resource_name: &str,
        isolation_channel: &Option<String>,
    ) -> Result<ReservationResult> {
        match self.http.post(endpoint).json(payload).send().await {
            Ok(resp) => match resp.status() {
                StatusCode::CREATED => {
                    info!("Resource reserved successfully");
                    Ok(ReservationResult::Success)
                }
                StatusCode::CONFLICT => Ok(ReservationResult::Conflict),
                StatusCode::BAD_REQUEST => {
                    anyhow::bail!("Bad request. Check your input data.");
                }
                StatusCode::UNAUTHORIZED => {
                    anyhow::bail!("Unauthorized. Check your API keys.");
                }
                StatusCode::NOT_FOUND => {
                    self.create_resource_if_missing(resource_name, isolation_channel)
                        .await?;
                    Ok(ReservationResult::Retry)
                }
                status_code => {
                    self.status_code(status_code)
                        .await
                        .context("Failure reserving resource")?;
                    Ok(ReservationResult::Retry)
                }
            },
            Err(error) => {
                self.request_failure(error)
                    .await
                    .context("Failure reserving resource")?;
                Ok(ReservationResult::Retry)
            }
        }
    }
}

/// Log information about an existing reservation.
fn log_reservation_info(resource: &ResourceListItem) -> Result<()> {
    if resource.active_reservation.is_none() {
        info!("No active reservation.");
        return Ok(());
    }
    let user = resource
        .active_reservation_user_name
        .as_ref()
        .context("Resource doesn't have active_reservation_user_name!")?;
    let reason = resource
        .active_reservation_reason
        .as_ref()
        .context("Resource doesn't have active_reservation_reason!")?;

    // Build the basic reservation message.
    let base_message = if let Some(workflow_url) = reason.split_whitespace().last() {
        if workflow_url.contains("/actions/runs/") {
            format!("Existing reservation by component {user} in {workflow_url}")
        } else {
            format!("Existing reservation by user {user} with reason \"{reason}\"")
        }
    } else {
        format!("Existing reservation by user {user} with reason \"{reason}\"")
    };

    // Add expiration information if available.
    if let Some(expires_at) = parse_expiration_time(&resource.active_reservation) {
        info!("{}. Expires at: {}.", base_message, expires_at);
    } else {
        info!("{}.", base_message);
    }
    Ok(())
}

/// Check if a resource has an active (non-expired) reservation.
fn has_active_reservation(resource: &ResourceListItem) -> bool {
    if resource.active_reservation.is_none() {
        return false;
    }

    // If we can parse the expiration time, check if it's in the future.
    if let Some(expires_at) = parse_expiration_time(&resource.active_reservation) {
        return expires_at > Utc::now();
    }
    // Conservatively assume the reservation is still active.
    true
}

/// Parse expiration time in a resource.
fn parse_expiration_time(active_reservation: &Option<String>) -> Option<DateTime<Utc>> {
    match active_reservation {
        Some(timestamp) => {
            match DateTime::parse_from_rfc3339(timestamp.as_str()) {
                Ok(datetime) => Some(datetime.with_timezone(&Utc)),
                Err(_) => {
                    info!("Active reservation {} is not a valid ISO 8601 timestamp", timestamp);
                    None
                }
            }
        },
        _ => None,
    }
}

/// Calculate wait time based on reservation expiration.
fn calculate_wait_time(resource: &ResourceListItem) -> Duration {
    let max_wait = Duration::from_secs(5 * 60);
    let no_wait = Duration::from_secs(1);

    // Try to parse expiration time from various possible fields.
    let expiration_time = parse_expiration_time(&resource.active_reservation);
    
    let base_wait = match expiration_time {
        None => no_wait,
        Some(expires_at) => {
            let now = Utc::now();
            if expires_at > now {
                let time_until_expiration = (expires_at - now).to_std().unwrap_or(Duration::ZERO);
                return std::cmp::min(time_until_expiration, max_wait);
            }
            return no_wait;
        }
    };

    // Pick a random duration between [base_wait, base_wait * 1.3333].
    let jitter_range = base_wait.as_millis() as u64 / 3;
    let jitter_offset = rand::thread_rng().gen_range(0..=jitter_range);
    base_wait + Duration::from_millis(jitter_offset)
}

#[derive(Debug)]
enum ReservationResult {
    Success,
    Conflict,
    Retry,
}

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
                    .attempt_reservation(&args.mode.api_endpoint(), &payload, resource_name, &args.isolation_channel)
                    .await?
                {
                    ReservationResult::Success => break,
                    ReservationResult::Conflict => {
                        info!("Resource already reserved, fetching reservation data.");
                        match state.fetch_resource_data(resource_name, &args.isolation_channel).await {
                            Ok(Some(resource)) => {
                                log_reservation_info(&resource)?;
                            }
                            _ => {
                                info!("Could not find resource after conflict.");
                            }
                        }
                        info!("Retrying in {} seconds.", BUSY_RETRY.as_secs());
                        sleep(BUSY_RETRY).await;
                    }
                    ReservationResult::Retry => {
                        // Continue the loop for retry.
                    }
                }
            }
        }
        Mode::ReserveExclusive {
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
                // First, check if resource exists (create it if it doesn't) and has an active reservation.
                let resource_data = match state.fetch_resource_data(resource_name, &args.isolation_channel).await {
                    Ok(data) => data,
                    Err(_) => continue,
                };

                if resource_data.is_none() {
                    if let Err(_) = state.create_resource_if_missing(resource_name, &args.isolation_channel).await {
                        continue;
                    }
                }

                // Check if there's an active (non-expired) reservation.
                if let Some(resource) = resource_data {
                
                    if has_active_reservation(&resource) {
                        log_reservation_info(&resource)?;

                        // Calculate wait time based on reservation expiration.
                        let wait_time = calculate_wait_time(&resource);
                        info!("Resource is reserved, waiting {:.1} seconds before retrying...",
                                wait_time.as_secs_f64());
                        sleep(wait_time).await;
                        continue;
                    }
                }

                match state
                    .attempt_reservation(&args.mode.api_endpoint(), &payload, resource_name, &args.isolation_channel)
                    .await?
                {
                    ReservationResult::Success => break,
                    ReservationResult::Conflict => {
                        // Another process reserved it between our check and reservation attempt.
                        info!("Resource became reserved between check and reservation attempt");
                        
                        // Fetch updated resource data to get expiration info for smart wait
                        let wait_time = match state.fetch_resource_data(resource_name, &args.isolation_channel).await {
                            Ok(Some(resource)) => calculate_wait_time(&resource),
                            _ => {
                                // Fallback to short wait if we can't fetch resource data.
                                Duration::from_millis(rand::thread_rng().gen_range(1000..=5000))
                            }
                        };
                        info!("Waiting {:.1} seconds before retrying...", wait_time.as_secs_f64());
                        sleep(wait_time).await;
                    }
                    ReservationResult::Retry => {
                        // Continue the loop for retry.
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

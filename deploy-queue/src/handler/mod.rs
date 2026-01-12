pub mod cancel;
pub mod fetch;
pub mod list;

use anyhow::Result;
use log::{info, warn};
use sqlx::{Pool, Postgres};
use time::Duration;
use tokio::task::JoinHandle;

use crate::{
    constants::{BUSY_RETRY, HEARTBEAT_INTERVAL, HEARTBEAT_TIMEOUT, HEARTBEAT_UPDATE_TIMEOUT},
    model::Deployment,
    util::{duration::DurationExt, github},
};

pub async fn enqueue_deployment(client: &Pool<Postgres>, deployment: Deployment) -> Result<i64> {
    let record = sqlx::query!("INSERT INTO deployments (environment, cloud_provider, region, cell_index, component, version, url, note, concurrency_key) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
        deployment.cell.environment, deployment.cell.cloud_provider, deployment.cell.region, deployment.cell.index, deployment.component, deployment.version, deployment.url, deployment.note, deployment.concurrency_key)
        .fetch_one(client)
        .await?;
    let deployment_id = record.id;
    log::info!(
        "Successfully inserted deployment record: id={}, environment={}, cloud_provider={}, region={}, cell_index={}, component={}",
        deployment_id,
        deployment.cell.environment,
        deployment.cell.cloud_provider,
        deployment.cell.region,
        deployment.cell.index,
        deployment.component
    );

    // Write deployment ID to GitHub outputs
    github::write_output("deployment-id", || Ok(deployment_id.to_string()))?;

    Ok(deployment_id)
}

/// Cancel deployments with stale heartbeats
async fn cancel_stale_heartbeat_deployments(
    client: &Pool<Postgres>,
    canceller_deployment_id: i64,
) -> Result<()> {
    let stale_deployments = fetch::stale_heartbeat_deployments(client, HEARTBEAT_TIMEOUT).await?;

    let cancellation_note = format!(
        "Cancelled by deployment {} due to stale heartbeat",
        canceller_deployment_id
    );

    for deployment in stale_deployments {
        log::warn!(
            "Cancelling deployment {} ({}, version={}) due to stale heartbeat: last seen {} ago at {}",
            deployment.id,
            deployment.component,
            deployment.version.as_deref().unwrap_or("unknown"),
            deployment.time_since_heartbeat.format_human(),
            deployment.heartbeat_timestamp.to_string(),
        );

        cancel::deployment(client, deployment.id, Some(cancellation_note.as_str())).await?;
    }

    Ok(())
}

pub async fn wait_for_blocking_deployments(
    pg_pool: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<()> {
    loop {
        // Check for and cancel any deployments with stale heartbeats
        cancel_stale_heartbeat_deployments(pg_pool, deployment_id).await?;

        let blocking_deployments = fetch::blocking_deployments(pg_pool, deployment_id).await?;

        if blocking_deployments.is_empty() {
            info!("No conflicting deployments found. Starting deployment...");
            break;
        } else {
            let blocking_ids: Vec<i64> = blocking_deployments
                .iter()
                .map(|b| b.deployment.id)
                .collect();

            // Calculate total ETA and per-component breakdown
            let mut total_remaining = Duration::ZERO;
            let mut component_times: std::collections::HashMap<String, Duration> =
                std::collections::HashMap::new();
            let mut has_unknown_eta = false;

            for blocking in &blocking_deployments {
                let (deployment_time, buffer_time) = blocking.remaining_time()?;

                if let Some(deployment_time) = deployment_time {
                    // Include both deployment time and buffer time in total
                    let total_time = deployment_time + buffer_time;
                    total_remaining += total_time;
                    *component_times
                        .entry(blocking.deployment.component.clone())
                        .or_insert(Duration::ZERO) += total_time;
                } else if buffer_time > Duration::ZERO {
                    // Finished deployment - only buffer time remains
                    total_remaining += buffer_time;
                    *component_times
                        .entry(blocking.deployment.component.clone())
                        .or_insert(Duration::ZERO) += buffer_time;
                } else {
                    has_unknown_eta = true;
                }
            }

            info!(
                "Found {} conflicting deployments: {:?}. Waiting {} seconds...",
                blocking_deployments.len(),
                blocking_ids,
                BUSY_RETRY.as_secs()
            );

            // Print total ETA
            if total_remaining > Duration::ZERO {
                info!("Total ETA: ~{}", total_remaining.format_human());

                // Print per-component breakdown
                let mut components: Vec<_> = component_times.iter().collect();
                components.sort_by_key(|(name, _)| *name);
                for (component, duration) in components {
                    if *duration > Duration::ZERO {
                        info!("  {}: ~{}", component, duration.format_human());
                    }
                }
            } else if has_unknown_eta {
                info!("Total ETA: unknown (missing analytics data)");
            }

            info!("Blocking deployments:");
            for blocking in &blocking_deployments {
                info!("  {}", blocking.summary()?);
            }

            tokio::time::sleep(BUSY_RETRY).await;
        }
    }
    Ok(())
}

pub async fn show_deployment_info(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    if let Some(deployment) = fetch::deployment(client, deployment_id).await? {
        println!("{}", deployment.summary());
    } else {
        println!("Deployment with ID {} not found", deployment_id);
    }
    Ok(())
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

/// Update the heartbeat timestamp for a deployment
/// This is the core function that can be called from anywhere (e.g., as a background task)
pub async fn update_heartbeat(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    sqlx::query!(
        "UPDATE deployments SET heartbeat_timestamp = NOW() WHERE id = $1",
        deployment_id
    )
    .execute(client)
    .await?;
    log::debug!("Heartbeat sent for deployment {}", deployment_id);
    Ok(())
}

/// Run heartbeat in a loop with periodic intervals until terminated
pub async fn run_heartbeat_loop(client: &Pool<Postgres>, deployment_id: i64) -> Result<()> {
    info!(
        "Starting heartbeat loop for deployment {} (interval: {}s)",
        deployment_id,
        HEARTBEAT_INTERVAL.as_secs()
    );

    const HEARTBEAT_MAX_CONSECUTIVE_FAILURES: u32 = 3;

    let mut consecutive_failures: u32 = 0;
    let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        interval.tick().await;

        let result = tokio::time::timeout(
            HEARTBEAT_UPDATE_TIMEOUT,
            update_heartbeat(client, deployment_id),
        )
        .await;

        if let Ok(Ok(())) = result {
            consecutive_failures = 0;
        } else {
            consecutive_failures += 1;
            let reason = match result {
                Ok(Err(err)) => err.to_string(),
                Err(_) => format!("timed out after {:?}", HEARTBEAT_UPDATE_TIMEOUT),
                _ => "unknown error".to_string(),
            };
            warn!(
                "Failed to send heartbeat for deployment {} (attempt {}/{}): {}",
                deployment_id, consecutive_failures, HEARTBEAT_MAX_CONSECUTIVE_FAILURES, reason
            );
        }

        if consecutive_failures >= HEARTBEAT_MAX_CONSECUTIVE_FAILURES {
            anyhow::bail!(
                "Heartbeat loop failed {} times consecutively for deployment {}",
                consecutive_failures,
                deployment_id
            );
        }
    }
}

/// Start a background heartbeat loop; returns a JoinHandle so caller can abort it
pub fn start_heartbeat_background(client: &Pool<Postgres>, deployment_id: i64) -> JoinHandle<()> {
    let heartbeat_client = client.clone();
    tokio::spawn(async move {
        if let Err(err) = run_heartbeat_loop(&heartbeat_client, deployment_id).await {
            warn!(
                "Heartbeat loop exited for deployment {}: {}",
                deployment_id, err
            );
        }
    })
}

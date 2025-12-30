use anyhow::{Context, Result};
use clap::Parser;

pub mod cli;
pub mod constants;
pub mod handler;
pub mod model;
pub mod util;

/// Main entry point for the deploy-queue application
pub async fn main() -> Result<()> {
    let log_env = env_logger::Env::default().filter_or("DEPLOY_QUEUE_LOG_LEVEL", "info");
    env_logger::Builder::from_env(log_env).init();
    let args = cli::Cli::parse();

    run_deploy_queue(args.mode, args.skip_migrations).await
}

pub async fn run_deploy_queue(mode: cli::Mode, skip_migrations: bool) -> Result<()> {
    // Create a connection pool for talking to the database
    let db_client = util::database::connect(skip_migrations).await?;

    match mode {
        cli::Mode::Start(deployment) => {
            // Insert deployment record into database
            let deployment_id = handler::enqueue_deployment(&db_client, deployment.clone().into())
                .await
                .context("Faild to enqueue deployment")?;

            // Start heartbeat loop in the background so we can abort it after starting
            let heartbeat_handle = handler::start_heartbeat_background(&db_client, deployment_id);

            // Wait for all blocking deployments to finish
            handler::wait_for_blocking_deployments(&db_client, deployment_id)
                .await
                .with_context(|| format!("Failed to wait for blocks of {deployment_id}"))?;

            // Mark deployment as started
            handler::start_deployment(&db_client, deployment_id)
                .await
                .with_context(|| format!("Failed to start deployment {deployment_id}"))?;

            // Stop the heartbeat loop now that the deployment has started
            heartbeat_handle.abort();
            let _ = heartbeat_handle.await;
        }
        cli::Mode::Finish { deployment_id } => {
            handler::finish_deployment(&db_client, deployment_id)
                .await
                .with_context(|| format!("Failed to finish deployment {deployment_id}"))?;
        }
        cli::Mode::Cancel {
            cancellation_note,
            target,
        } => match target {
            cli::CancelTarget::Deployment { deployment_id } => {
                handler::cancel::deployment(&db_client, deployment_id, cancellation_note)
                    .await
                    .with_context(|| format!("Failed to cancel deployment {deployment_id}"))?;
            }
            cli::CancelTarget::Version { component, version } => {
                handler::cancel::by_component_version(
                    &db_client,
                    component,
                    version,
                    cancellation_note,
                )
                .await
                .context("Failed to cancel deployments matching the given component and version")?;
            }
            cli::CancelTarget::Location {
                environment,
                cloud_provider,
                region,
                cell_index,
            } => {
                handler::cancel::by_location(
                    &db_client,
                    environment.as_ref(),
                    &cloud_provider,
                    &region,
                    cell_index,
                    cancellation_note.as_deref(),
                )
                .await
                .context("Failed to cancel deployments matching the given location")?;
            }
        },
        cli::Mode::Info { deployment_id } => {
            handler::show_deployment_info(&db_client, deployment_id)
                .await
                .with_context(|| format!("Failed to show info for deployment {deployment_id}"))?;
        }
        cli::Mode::List {
            entity: cli::ListEntity::Outliers,
        } => {
            handler::list::outliers(&db_client)
                .await
                .context("Failed to list outliers")?;
        }
        cli::Mode::List {
            entity: cli::ListEntity::Cells { environment },
        } => {
            handler::list::cells(&db_client, environment)
                .await
                .context("Failed to list cells")?;
        }
        cli::Mode::Heartbeat { target } => match target {
            cli::HeartbeatTarget::Deployment { deployment_id } => {
                handler::run_heartbeat_loop(&db_client, deployment_id)
                    .await
                    .with_context(|| {
                        format!("Failed to run heartbeat loop for deployment {deployment_id}")
                    })?;
            }
            cli::HeartbeatTarget::Url { url } => {
                let deployment_id = handler::fetch::deployment_id_by_url(&db_client, &url)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("No deployment found with URL: {}", url))?;

                handler::run_heartbeat_loop(&db_client, deployment_id)
                    .await
                    .with_context(|| {
                        format!("Failed to run heartbeat loop for deployment {deployment_id}")
                    })?;
            }
        },
    }

    Ok(())
}

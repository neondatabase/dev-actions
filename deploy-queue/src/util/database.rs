use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use backon::{ExponentialBuilder, Retryable};
use log::{info, warn};
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use tokio::time::timeout;

use crate::constants::{ACQUIRE_TIMEOUT, CONNECTION_TIMEOUT, IDLE_TIMEOUT};

pub async fn connect(skip_migrations: bool) -> Result<Pool<Postgres>> {
    let pool = create_db_connection().await?;
    if skip_migrations {
        info!("Skipping database migrations (--skip-migrations flag set)");
    } else {
        run_migrations(&pool).await?;
    }
    Ok(pool)
}

async fn create_db_connection() -> Result<Pool<Postgres>> {
    let database_url = std::env::var("DEPLOY_QUEUE_DATABASE_URL")
        .context("DEPLOY_QUEUE_DATABASE_URL environment variable is not set")?;

    (async || {
        let connect_future = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .idle_timeout(Some(IDLE_TIMEOUT))
            .connect(&database_url);

        timeout(CONNECTION_TIMEOUT, connect_future)
            .await
            .context("Connection attempt timed out")?
            .context("Failed to connect to database")
    })
    .retry(ExponentialBuilder::default())
    .notify(|err: &anyhow::Error, dur: StdDuration| {
        warn!(
            "Failed to connect to database: {}. Retrying in {:?}...",
            err, dur
        );
    })
    .await
}

async fn run_migrations(pool: &Pool<Postgres>) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("Failed to run database migrations")?;

    info!("Database migrations completed successfully");
    Ok(())
}

use anyhow::{Context, Result, bail};
use sqlx::{Pool, Postgres};
use time::Duration;

use crate::{
    cli::Environment,
    model::{BlockingDeployment, Cell, Deployment, OutlierDeployment},
    util::duration::DurationExt,
};

pub async fn deployment(client: &Pool<Postgres>, deployment_id: i64) -> Result<Option<Deployment>> {
    let row = sqlx::query!(
        r#"
        SELECT
            d.id, d.environment, d.cloud_provider, d.region, d.cell_index, d.component, d.version, d.url, d.note, d.concurrency_key,
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
            cell: Cell {
                environment: row.environment,
                cloud_provider: row.cloud_provider,
                region: row.region,
                index: row.cell_index,
            },
            component: row.component,
            version: row.version,
            url: row.url,
            note: row.note,
            concurrency_key: row.concurrency_key,
            start_timestamp: row.start_timestamp,
            finish_timestamp: row.finish_timestamp,
            cancellation_timestamp: row.cancellation_timestamp,
            cancellation_note: row.cancellation_note,
            buffer_time: row
                .buffer_time
                .to_duration()
                .context("Failed to convert buffer_time from database")?,
        }))
    } else {
        Ok(None)
    }
}

pub async fn deployment_id_by_url(client: &Pool<Postgres>, url: &str) -> Result<Option<i64>> {
    let row = sqlx::query!(
        r#"
        SELECT id
        FROM deployments
        WHERE url = $1
        ORDER BY id DESC
        LIMIT 1
        "#,
        url
    )
    .fetch_optional(client)
    .await?;

    if let Some(row) = row {
        Ok(Some(row.id))
    } else {
        Ok(None)
    }
}

pub async fn blocking_deployments(
    client: &Pool<Postgres>,
    deployment_id: i64,
) -> Result<Vec<BlockingDeployment>> {
    let rows = sqlx::query_file!("queries/blocking_deployments.sql", deployment_id)
        .fetch_all(client)
        .await?;

    let blocking_deployments: Vec<BlockingDeployment> = rows
        .into_iter()
        .map(|row| {
            let buffer_time = row.buffer_time.to_duration().with_context(|| {
                format!("Failed to convert buffer_time for deployment {}", row.id)
            })?;
            let avg_duration = match row.avg_duration {
                Some(i) => Some(i.to_duration().with_context(|| {
                    format!("Failed to convert avg_duration for deployment {}", row.id)
                })?),
                None => None,
            };
            let stddev_duration = match row.stddev_duration {
                Some(i) => Some(i.to_duration().with_context(|| {
                    format!(
                        "Failed to convert stddev_duration for deployment {}",
                        row.id
                    )
                })?),
                None => None,
            };

            Ok(BlockingDeployment {
                deployment: Deployment {
                    id: row.id,
                    cell: Cell {
                        environment: row.environment,
                        cloud_provider: row.cloud_provider,
                        region: row.region,
                        index: row.cell_index,
                    },
                    component: row.component,
                    version: row.version,
                    url: row.url,
                    note: row.note,
                    start_timestamp: row.start_timestamp,
                    finish_timestamp: row.finish_timestamp,
                    cancellation_timestamp: row.cancellation_timestamp,
                    cancellation_note: row.cancellation_note,
                    concurrency_key: row.concurrency_key,
                    buffer_time,
                },
                avg_duration,
                stddev_duration,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(blocking_deployments)
}

pub async fn outlier_deployments(client: &Pool<Postgres>) -> Result<Vec<OutlierDeployment>> {
    let rows = sqlx::query_file!("queries/active_outliers.sql")
        .fetch_all(client)
        .await?;

    let outliers: Vec<OutlierDeployment> = rows
        .into_iter()
        .map(|row| {
            let current_duration = match row.current_duration {
                Some(i) => i.to_duration().with_context(|| {
                    format!(
                        "Failed to convert current_duration for deployment {}",
                        row.id
                    )
                })?,
                None => Duration::ZERO,
            };
            let avg_duration = match row.avg_duration {
                Some(i) => i.to_duration().with_context(|| {
                    format!("Failed to convert avg_duration for deployment {}", row.id)
                })?,
                None => Duration::ZERO,
            };
            let stddev_duration = match row.stddev_duration {
                Some(i) => i.to_duration().with_context(|| {
                    format!(
                        "Failed to convert stddev_duration for deployment {}",
                        row.id
                    )
                })?,
                None => Duration::ZERO,
            };

            Ok(OutlierDeployment {
                id: row.id,
                env: row.env,
                cloud_provider: row.cloud_provider,
                region: row.region,
                cell_index: row.cell_index,
                component: row.component,
                url: row.url,
                note: row.note,
                version: row.version,
                current_duration,
                avg_duration,
                stddev_duration,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(outliers)
}

pub(crate) async fn cells(client: &Pool<Postgres>, environment: Environment) -> Result<Vec<Cell>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            environment,
            cloud_provider,
            region,
            cell_index
        FROM cells
        WHERE environment = $1
        ORDER BY cloud_provider, region, cell_index
        "#,
        environment.to_string()
    )
    .fetch_all(client)
    .await?;

    let cells = rows
        .into_iter()
        .map(|row| -> Result<Cell> {
            if let (Some(environment), Some(cloud_provider), Some(region), Some(index)) = (
                row.environment,
                row.cloud_provider,
                row.region,
                row.cell_index,
            ) {
                Ok(Cell {
                    environment,
                    cloud_provider,
                    region,
                    index,
                })
            } else {
                bail!("'cells' materialized view contained 'NULL' value, aborting!")
            }
        })
        .collect::<Result<Vec<Cell>>>()?;

    Ok(cells)
}

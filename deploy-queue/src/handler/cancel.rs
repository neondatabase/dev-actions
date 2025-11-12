use anyhow::Result;
use log::info;
use sqlx::{Pool, Postgres};

pub async fn deployment(
    client: &Pool<Postgres>,
    deployment_id: i64,
    cancellation_note: Option<impl AsRef<str>>,
) -> Result<()> {
    let cancellation_note: Option<&str> = cancellation_note.as_ref().map(|note| note.as_ref());

    log::info!("Cancelling deployment {}", deployment_id);
    sqlx::query!("UPDATE deployments SET cancellation_timestamp = NOW(), cancellation_note = $2 WHERE id = $1", deployment_id, cancellation_note)
        .execute(client)
        .await?;
    log::info!("Deployment {} has been cancelled", deployment_id);
    Ok(())
}

pub async fn by_component_version(
    client: &Pool<Postgres>,
    component: impl AsRef<str>,
    version: impl AsRef<str>,
    cancellation_note: Option<impl AsRef<str>>,
) -> Result<u64> {
    let component: &str = component.as_ref();
    let version: &str = version.as_ref();

    let cancellation_note: Option<&str> = cancellation_note.as_ref().map(|note| note.as_ref());

    // Cancel by environment + component + version
    info!(
        "Cancelling all deployments for component {} and version {}",
        component, version
    );

    let result = sqlx::query!(
        "UPDATE deployments
         SET cancellation_timestamp = NOW(), cancellation_note = $1
         WHERE component = $2
           AND version = $3",
        cancellation_note,
        component,
        version
    )
    .execute(client)
    .await?;

    let rows_affected = result.rows_affected();
    log::info!(
        "Cancelled {} deployment(s) for component {} version {}",
        rows_affected,
        component,
        version,
    );
    Ok(rows_affected)
}

pub async fn by_location(
    client: &Pool<Postgres>,
    environment: impl AsRef<str>,
    cloud_provider: impl AsRef<str>,
    region: impl AsRef<str>,
    cell_index: Option<i32>,
    cancellation_note: Option<impl AsRef<str>>,
) -> Result<u64> {
    let environment: &str = environment.as_ref();
    let cloud_provider: &str = cloud_provider.as_ref();
    let region: &str = region.as_ref();

    let cancellation_note: Option<&str> = cancellation_note.as_ref().map(|note| note.as_ref());

    // Cancel by location (environment + cloud_provider + region + optional cell_index)
    info!(
        "Cancelling all deployments for environment {} on cloud provider {} in region {}{}",
        environment,
        cloud_provider,
        region,
        cell_index
            .map(|cell_index| format!(" and cell index {}", cell_index))
            .unwrap_or_default()
    );

    let result = if let Some(cell_index) = cell_index {
        sqlx::query!(
            "UPDATE deployments
             SET cancellation_timestamp = NOW(), cancellation_note = $1
             WHERE environment = $2
               AND cloud_provider = $3
               AND region = $4
               AND cell_index = $5",
            cancellation_note,
            environment,
            cloud_provider,
            region,
            cell_index
        )
        .execute(client)
        .await?
    } else {
        sqlx::query!(
            "UPDATE deployments
             SET cancellation_timestamp = NOW(), cancellation_note = $1
             WHERE environment = $2
               AND cloud_provider = $3
               AND region = $4",
            cancellation_note,
            environment,
            cloud_provider,
            region
        )
        .execute(client)
        .await?
    };

    let rows_affected = result.rows_affected();
    if let Some(cell_index) = cell_index {
        log::info!(
            "Cancelled {} deployment(s) in environment {} / {} / {} / cell {}",
            rows_affected,
            environment,
            cloud_provider,
            region,
            cell_index
        );
    } else {
        log::info!(
            "Cancelled {} deployment(s) in environment {} / {} / {}",
            rows_affected,
            environment,
            cloud_provider,
            region
        );
    }
    Ok(rows_affected)
}

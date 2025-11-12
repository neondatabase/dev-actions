use anyhow::{Context, Result};
use sqlx::{Pool, Postgres};

use crate::{cli::Environment, handler::fetch, util::github};

pub(crate) async fn outliers(client: &Pool<Postgres>) -> Result<()> {
    let outliers = fetch::outlier_deployments(client).await?;

    github::write_output("active-outliers", || {
        serde_json::to_string(&outliers).context("Failed to serialize outliers to JSON")
    })?;

    let json_output = serde_json::to_string_pretty(&outliers)?;
    println!("{}", json_output);

    Ok(())
}

pub(crate) async fn cells(client: &Pool<Postgres>, environment: Environment) -> Result<()> {
    let cells = fetch::cells(client, environment.clone()).await?;

    github::write_output("cells", || {
        serde_json::to_string(&cells).context("Failed to serialize cells to JSON")
    })?;

    println!("Known cells for environment {}:", environment);
    for cell in cells {
        println!(
            "  - {}-{}-{}-{}",
            cell.environment, cell.cloud_provider, cell.region, cell.index
        );
    }
    Ok(())
}

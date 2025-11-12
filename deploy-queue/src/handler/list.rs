use anyhow::{Context, Result};
use sqlx::{Pool, Postgres};

use crate::{handler::fetch, util::github};

pub(crate) async fn outliers(client: &Pool<Postgres>) -> Result<()> {
    let outliers = fetch::outlier_deployments(client).await?;

    github::write_output("active-outliers", || {
        serde_json::to_string(&outliers).context("Failed to serialize outliers to JSON")
    })?;

    let json_output = serde_json::to_string_pretty(&outliers)?;
    println!("{}", json_output);

    Ok(())
}

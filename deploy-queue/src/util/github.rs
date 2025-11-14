use std::{fs::OpenOptions, io::Write};

use anyhow::Result;
use uuid::Uuid;

/// Write a key-value pair to GitHub Actions output file
/// The value is computed lazily via the provided closure, only if GITHUB_OUTPUT is set
pub fn write_output<F>(key: &str, value_fn: F) -> Result<()>
where
    F: FnOnce() -> Result<String>,
{
    if let Ok(github_output) = std::env::var("GITHUB_OUTPUT") {
        let delimiter = Uuid::new_v4();
        let value = value_fn()?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(github_output)?;
        writeln!(file, "{key}<<{delimiter}")?;
        writeln!(file, "{value}")?;
        writeln!(file, "{delimiter}")?;
    }
    Ok(())
}

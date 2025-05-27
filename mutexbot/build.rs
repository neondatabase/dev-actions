use std::fs::create_dir_all;

use anyhow::Result;
use clap_allgen::{render_manpages, render_shell_completions};

#[allow(dead_code)]
mod cli {
    include!("src/cli.rs");
}

fn main() -> Result<()> {
    create_dir_all("assets/man")?;
    render_manpages::<cli::Cli>("assets/man")?;
    create_dir_all("assets/completions")?;
    render_shell_completions::<cli::Cli>("assets/completions")?;
    Ok(())
}

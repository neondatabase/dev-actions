use clap::{Parser, Subcommand, ValueEnum};

/// Environment enum for deployment targets
#[derive(Clone, Debug, ValueEnum)]
pub enum Environment {
    Dev,
    Prod,
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

impl AsRef<str> for Environment {
    fn as_ref(&self) -> &str {
        match self {
            Environment::Dev => "dev",
            Environment::Prod => "prod",
        }
    }
}

/// CLI for starting and finishing and canceling deployments.
/// This CLI is used by the Deploy Queue GitHub Action.
#[derive(Parser)]
#[command(version, about, long_about)]
pub struct Cli {
    #[command(subcommand)]
    pub mode: Mode,
}

#[derive(Subcommand, Clone)]
pub enum Mode {
    /// Start deployment for a component
    Start {
        #[arg(long)]
        /// Environment where to deploy
        environment: Environment,
        #[arg(long = "provider")]
        /// Cloud provider to deploy
        cloud_provider: String,
        #[arg(long)]
        /// Region to deploy
        region: String,
        #[arg(long)]
        /// Cell index to deploy
        cell_index: i32,
        #[arg(long)]
        /// Component to deploy
        component: String,
        /// Environment where to deploy
        environment: Environment,
        #[arg(long)]
        /// Version of the component to deploy
        version: Option<String>,
        #[arg(long)]
        /// URL to the specific GitHub Actions job
        url: Option<String>,
        #[arg(long)]
        /// Note for this deployment (for manual deployments)
        note: Option<String>,
        #[arg(long)]
        /// Concurrency key for this deployment
        concurrency_key: Option<String>,
    },
    /// Finish deployment for a component
    Finish {
        /// Deployment ID to finish
        deployment_id: i64,
    },
    /// Cancel deployment for a component
    Cancel {
        /// Deployment ID to cancel
        deployment_id: i64,
        /// Cancellation note for this deployment
        cancellation_note: Option<String>,
    },
    /// Get info about a deployment
    Info {
        /// Deployment ID to get info for
        deployment_id: i64,
    },
}

use clap::{Parser, Subcommand, ValueEnum};

/// Environment enum for deployment targets
#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum Environment {
    Dev,
    Prod,
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Environment::Dev => write!(f, "dev"),
            Environment::Prod => write!(f, "prod"),
        }
    }
}

impl Environment {
    /// Convert to string for database operations
    pub(crate) fn as_str(&self) -> &'static str {
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
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) mode: Mode,
}

#[derive(Subcommand, Clone)]
pub(crate) enum Mode {
    /// Start deployment for a component
    Start {
        /// Region to deploy
        region: String,
        /// Component to deploy
        component: String,
        /// Environment where to deploy
        environment: Environment,
        /// Version of the component to deploy
        version: String,
        /// URL to the specific GitHub Actions job
        url: Option<String>,
        /// Note for this deployment (for manual deployments)
        note: Option<String>,
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
}

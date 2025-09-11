use clap::{Parser, Subcommand};

/// CLI for starting and finishing and canceling deployments.
/// This CLI is used by the MutexBot GitHub Action.
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
        environment: String,
        /// Version of the component to deploy
        version: String,
        /// URL to the specific GitHub Actions job
        url: Option<String>,
        /// Note for this reservation (for manual deployments)
        note: Option<String>,
    },
    /// Finish deployment for a component
    Finish {
        /// Region to finish the deployment for
        region: String,
        /// Component to finish the deployment for
        component: String,
    },
    /// Cancel deployment for a component
    Cancel {
        /// Resource to cancel the deployment for
        region: String,
        /// Component to cancel the deployment for
        component: String,
    },
}

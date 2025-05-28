use clap::{Parser, Subcommand};

/// CLI for reserving and (force-)releasing MutexBot resources.
#[derive(Parser)]
#[command(version, about, long_about)]
pub(crate) struct Cli {
    /// MutexBot API Key
    #[arg(env, long)]
    pub(crate) mutexbot_api_key: String,
    /// Isolation channel for resource
    #[arg(long)]
    pub(crate) isolation_channel: Option<String>,
    #[command(subcommand)]
    pub(crate) mode: Mode,
}

#[derive(Subcommand, Clone)]
pub(crate) enum Mode {
    /// Reserve a resource
    Reserve {
        /// Resource to reserve
        resource_name: String,
        /// Notes for this reservation
        notes: String,
        /// Duration to reserve resource for. Defaults to value set in MutexBot if omitted
        duration: Option<String>,
    },
    /// Release a resource
    Release {
        /// Resource to release
        resource_name: String,
    },
    /// Force Release a resource
    ForceRelease {
        /// Resource to force-release
        resource_name: String,
    },
}

impl Mode {
    pub(crate) fn api_endpoint(&self) -> String {
        match self {
            Mode::Reserve { resource_name, .. } => format!(
                "https://mutexbot.com/api/resources/global/{}/reserve",
                resource_name,
            ),
            Mode::Release { resource_name } => format!(
                "https://mutexbot.com/api/resources/global/{}/release",
                resource_name,
            ),
            Mode::ForceRelease { resource_name } => format!(
                "https://mutexbot.com/api/resources/global/{}/force-release",
                resource_name,
            ),
        }
    }
}

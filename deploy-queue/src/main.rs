use anyhow::Result;
use clap::Parser;
use deploy_queue::{cli, run_deploy_queue};

#[tokio::main]
async fn main() -> Result<()> {
    let log_env = env_logger::Env::default().filter_or("DEPLOY_QUEUE_LOG_LEVEL", "info");
    env_logger::Builder::from_env(log_env).init();
    let args = cli::Cli::parse();

    run_deploy_queue(args.mode).await
}

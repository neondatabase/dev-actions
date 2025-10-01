use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    deploy_queue::main().await
}

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    agent_application::run().await
}

//! Binary entrypoint for the notes service.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    elowen_notes::run().await
}

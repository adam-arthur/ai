use llm::{Client, ModelId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let client = Client::from_env()?;
    let answer = client.query(ModelId::GEMMA_4_E2B_Q4, "What is 2 + 2?").await?;

    println!("{answer}");
    Ok(())
}

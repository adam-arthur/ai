use llm::{AskRequest, ModelId, ask};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let answer = ask(AskRequest::builder()
        .model(ModelId::GEMMA_4_E2B_Q4)
        .prompt("What is 2 + 2?")
        .build())
    .await?;

    println!("{answer}");
    Ok(())
}

use llm::{AskRequest, JsonSchema, ModelId, ask};
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
struct Answer {
    answer: u32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let answer: Answer = ask(AskRequest::builder()
        .model(ModelId::GEMMA_4_E2B_Q4)
        .prompt("What is 2 + 2? Return the result in the answer field.")
        .build())
    .await?;

    println!("{}", answer.answer);
    Ok(())
}

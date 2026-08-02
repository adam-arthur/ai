use std::{env, net::SocketAddr, sync::Arc};

use anyhow::{Context as _, Result};
use language_tutor::KoreanTutor;
use llm_google::GeminiClient;
use llm_openai::OpenAiClient;
use tokio::net::TcpListener;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::EnvFilter;
use voice_session::SessionService;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    // Preserve the previous development key location during the migration.
    dotenvy::from_filename("packages/llm/.env").ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("tutor_server=info,tower_http=info")),
        )
        .init();

    let openai = Arc::new(OpenAiClient::new(required_env("OPENAI_API_KEY")?));
    let gemini = Arc::new(GeminiClient::new(required_env("GEMINI_API_KEY")?));
    let tutor = Arc::new(KoreanTutor::new(
        gemini,
        openai.clone(),
        openai.clone(),
        openai,
    ));
    let sessions = Arc::new(SessionService::new(tutor));
    let app = tutor_api::router(sessions)
        .fallback_service(ServeDir::new("apps/tutor/dist").append_index_html_on_directories(true))
        .layer(TraceLayer::new_for_http());
    let address = env::var("TUTOR_ADDRESS")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_owned())
        .parse::<SocketAddr>()
        .context("TUTOR_ADDRESS must be a socket address such as 127.0.0.1:3000")?;
    let listener = TcpListener::bind(address).await?;
    info!(%address, "tutor server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn required_env(name: &str) -> Result<String> {
    env::var(name)
        .with_context(|| format!("set {name} in the environment or a repository-root .env file"))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

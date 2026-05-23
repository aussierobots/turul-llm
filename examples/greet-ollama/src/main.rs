//! Tiny example: render a greeting prompt + JSON Schema, call Ollama
//! through `turul-llm-ollama`, print the structured greeting.
//!
//! Two modes:
//!
//! - **Offline stub (default)** — no Ollama, no network. Prints a
//!   deterministic JSON object that satisfies the output schema. Use
//!   this to inspect the wiring without running a model.
//! - **Live Ollama (opt-in)** — set `OLLAMA_BASE_URL=http://host:11434`
//!   or `RUN_OLLAMA_SMOKE=1` (defaults to `http://localhost:11434`).
//!   The example will POST to `/api/chat` and parse the structured
//!   response.
//!
//! Run:
//!   cargo run -p greet-ollama
//!   OLLAMA_BASE_URL=http://localhost:11434 cargo run -p greet-ollama -- Ada formal

use serde_json::{Value, json};
use turul_llm_core::{CompletionRequest, ExecutionHints, LlmClient, LlmError};
use turul_llm_ollama::OllamaClient;

fn output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"greeting": {"type": "string"}},
        "required": ["greeting"]
    })
}

fn render_prompt(name: &str, style: &str) -> String {
    format!(
        "Produce a JSON object with a single field `greeting` that warmly greets a person named \
         {name}. The style hint is `{style}`. Respond with JSON only — no prose."
    )
}

fn ollama_base_url() -> Option<String> {
    if let Ok(v) = std::env::var("OLLAMA_BASE_URL")
        && !v.trim().is_empty()
    {
        return Some(v.trim().trim_end_matches('/').to_string());
    }
    if std::env::var("RUN_OLLAMA_SMOKE").ok().as_deref() == Some("1") {
        return Some("http://localhost:11434".to_string());
    }
    None
}

fn model() -> String {
    std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.1".to_string())
}

struct OfflineStub;

#[async_trait::async_trait]
impl LlmClient for OfflineStub {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<turul_llm_core::CompletionResponse, LlmError> {
        // Echo a deterministic structured payload. The name is parsed
        // out of the prompt so the offline output reflects the
        // arguments the user passed.
        let name = request
            .rendered_prompt
            .split("named ")
            .nth(1)
            .and_then(|tail| tail.split('.').next())
            .map(|s| s.trim().trim_end_matches('.').to_string())
            .unwrap_or_else(|| "friend".to_string());
        Ok(turul_llm_core::CompletionResponse::new(json!({
            "greeting": format!("Hi, {name}! (offline stub)")
        })))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "Ada".to_string());
    let style = args.next().unwrap_or_else(|| "casual".to_string());

    let prompt = render_prompt(&name, &style);
    let schema = output_schema();

    // async_trait makes the trait object easy here.
    let (mode, client): (&str, Box<dyn LlmClient>) = if let Some(base) = ollama_base_url() {
        ("live-ollama", Box::new(OllamaClient::new(base, model())))
    } else {
        ("offline-stub", Box::new(OfflineStub))
    };

    println!("Mode: {mode}");
    println!("Prompt: {prompt}");

    let response = client
        .complete(
            CompletionRequest::new(prompt)
                .with_output_schema(schema)
                .with_execution_hints(
                    ExecutionHints::new()
                        .with_max_tokens(128)
                        .with_temperature(0.2),
                ),
        )
        .await?;

    println!(
        "Response: {}",
        serde_json::to_string_pretty(&response.parsed_output)?
    );
    Ok(())
}

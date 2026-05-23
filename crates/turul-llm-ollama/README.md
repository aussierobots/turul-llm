# turul-llm-ollama

[Ollama](https://ollama.com) adapter for the
[`turul-llm-core::LlmClient`](https://docs.rs/turul-llm-core/latest/turul_llm_core/client/trait.LlmClient.html)
trait. Targets `/api/chat` with structured output via the `format`
field.

## Quick start

```toml
[dependencies]
turul-llm-core = "0.1"
turul-llm-ollama = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

```rust
use std::sync::Arc;
use serde_json::json;
use turul_llm_core::{CompletionRequest, ExecutionHints, LlmClient};
use turul_llm_ollama::OllamaClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client: Arc<dyn LlmClient> =
        Arc::new(OllamaClient::new("http://localhost:11434", "llama3.1"));

    let request = CompletionRequest::new(
        "Produce a JSON object with a single field `greeting` warmly greeting Ada.",
    )
    .with_output_schema(json!({
        "type": "object",
        "properties": { "greeting": { "type": "string" } },
        "required": ["greeting"]
    }))
    .with_execution_hints(
        ExecutionHints::new().with_max_tokens(64).with_temperature(0.2),
    );

    let response = client.complete(request).await?;
    println!("{}", serde_json::to_string_pretty(&response.parsed_output)?);
    Ok(())
}
```

## How the request maps to Ollama

The adapter POSTs to `{base_url}/api/chat` with:

- `model` — the value passed to [`OllamaClient::new`](https://docs.rs/turul-llm-ollama/latest/turul_llm_ollama/struct.OllamaClient.html).
- `stream: false` — the trait is non-streaming.
- A single `user` message carrying [`CompletionRequest::rendered_prompt`](https://docs.rs/turul-llm-core/latest/turul_llm_core/request/struct.CompletionRequest.html).
- `format` — the JSON Schema from `output_schema`, forwarded verbatim
  (Ollama's structured-output mechanism).
- `options.{num_predict, temperature, top_p}` — derived from
  `ExecutionHints`, only emitted when the hint is `Some(_)`.

The Ollama envelope is decoded by parsing `message.content` as JSON.
If the content fails to parse, the adapter returns
[`LlmError::SchemaViolation`](https://docs.rs/turul-llm-core/latest/turul_llm_core/error/enum.LlmError.html#variant.SchemaViolation).

## Tests are hermetic

The crate's own test suite uses
[`wiremock`](https://crates.io/crates/wiremock) to pin the request body
shape and the response decoding. `cargo test` requires no live Ollama.

## Live integration

Live mode is opt-in in the example crate (`examples/greet-ollama` in
the workspace) via environment variables:

| Variable | Effect |
|---|---|
| `OLLAMA_BASE_URL` | Base URL to target, e.g. `http://localhost:11434`. |
| `RUN_OLLAMA_SMOKE` | Set to `1` to use `http://localhost:11434` as the base URL without setting `OLLAMA_BASE_URL` explicitly. |
| `OLLAMA_MODEL` | Override the default model identifier. |

For your own application code, no env vars are needed — pass the base
URL and model directly to `OllamaClient::new`.

## See also

- [Workspace repository](https://github.com/aussierobots/turul-llm) — trait, sibling adapters, runnable example.
- [API documentation](https://docs.rs/turul-llm-ollama/latest/turul_llm_ollama/) on docs.rs.
- [`turul-llm-core`](https://crates.io/crates/turul-llm-core) — the trait this crate implements.
- [`turul-llm-openai`](https://crates.io/crates/turul-llm-openai) — sibling adapter for OpenAI-compatible APIs.

## Licensing

Dual-licensed under Apache-2.0 OR MIT.

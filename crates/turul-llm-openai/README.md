# turul-llm-openai

OpenAI-compatible adapter for the
[`turul-llm-core::LlmClient`](https://docs.rs/turul-llm-core/latest/turul_llm_core/client/trait.LlmClient.html)
trait. Targets `/chat/completions` with structured output via
`response_format = { type: "json_schema", json_schema: { ..., strict: true } }`.

The `base_url` is configurable, so this adapter also works against
OpenAI-compatible gateways — Together.ai, vLLM, Groq, local proxies,
etc. The default points at `https://api.openai.com/v1`.

## Quick start

```toml
[dependencies]
turul-llm-core = "0.1"
turul-llm-openai = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

```rust
use std::sync::Arc;
use serde_json::json;
use turul_llm_core::{CompletionRequest, ExecutionHints, LlmClient};
use turul_llm_openai::OpenAiClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let api_key = std::env::var("OPENAI_API_KEY")?;
    let client: Arc<dyn LlmClient> = Arc::new(OpenAiClient::new(api_key, "gpt-4o-mini"));

    let request = CompletionRequest::new(
        "Produce a JSON object with a single field `greeting` warmly greeting Ada.",
    )
    .with_output_schema(json!({
        "type": "object",
        "properties": { "greeting": { "type": "string" } },
        "required": ["greeting"],
        "additionalProperties": false
    }))
    .with_execution_hints(
        ExecutionHints::new().with_max_tokens(64).with_temperature(0.2),
    );

    let response = client.complete(request).await?;
    println!("{}", serde_json::to_string_pretty(&response.parsed_output)?);
    Ok(())
}
```

To target a non-OpenAI gateway, use
[`OpenAiClient::with_base_url`](https://docs.rs/turul-llm-openai/latest/turul_llm_openai/struct.OpenAiClient.html#method.with_base_url):

```rust
# use turul_llm_openai::OpenAiClient;
let client = OpenAiClient::with_base_url(
    "https://api.together.xyz/v1",
    std::env::var("TOGETHER_API_KEY").unwrap(),
    "meta-llama/Llama-3-8b-chat-hf",
);
```

## How the request maps to OpenAI

The adapter POSTs to `{base_url}/chat/completions` with:

- `Authorization: Bearer <api_key>` header.
- `model` — the value passed to the constructor.
- A single `user` message carrying [`CompletionRequest::rendered_prompt`](https://docs.rs/turul-llm-core/latest/turul_llm_core/request/struct.CompletionRequest.html).
- `response_format = { type: "json_schema", json_schema: { name, schema, strict: true } }`
  when `output_schema` is `Some(_)`. `strict: true` holds the model to
  the schema and surfaces a
  [`LlmError::SchemaViolation`](https://docs.rs/turul-llm-core/latest/turul_llm_core/error/enum.LlmError.html#variant.SchemaViolation)
  if the returned content still fails to parse.
- `max_tokens` / `temperature` / `top_p` derived from
  `ExecutionHints`, only emitted when the hint is `Some(_)`.

The OpenAI envelope is decoded via `choices[0].message.content`,
parsed as JSON.

## Tests are hermetic

The crate's own test suite uses
[`wiremock`](https://crates.io/crates/wiremock) to pin the request body
shape and the response decoding. `cargo test` requires no live
OpenAI API key and is not exercised against the real OpenAI API by
default.

## See also

- [Workspace repository](https://github.com/aussierobots/turul-llm) — trait, sibling adapters, runnable example.
- [API documentation](https://docs.rs/turul-llm-openai/latest/turul_llm_openai/) on docs.rs.
- [`turul-llm-core`](https://crates.io/crates/turul-llm-core) — the trait this crate implements.
- [`turul-llm-ollama`](https://crates.io/crates/turul-llm-ollama) — sibling adapter for local Ollama models.

## Licensing

Dual-licensed under Apache-2.0 OR MIT.

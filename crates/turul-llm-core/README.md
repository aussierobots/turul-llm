# turul-llm-core

The provider-neutral contract for calling an LLM with structured-output
JSON Schema and getting parsed JSON back. One async method, zero
transport dependencies.

This crate is the **trait + types only**. Concrete provider adapters
live in sibling crates so this crate stays cheap to depend on for code
that doesn't know yet which provider it will run against.

| Adapter crate | Provider |
|---|---|
| [`turul-llm-ollama`](https://crates.io/crates/turul-llm-ollama) | Ollama (`/api/chat`) |
| [`turul-llm-openai`](https://crates.io/crates/turul-llm-openai) | OpenAI + OpenAI-compatible gateways (`/chat/completions`) |

## Quick start

```toml
[dependencies]
turul-llm-core = "0.1"
```

```rust
use std::sync::Arc;
use serde_json::json;
use turul_llm_core::{CompletionRequest, ExecutionHints, LlmClient};

async fn greet(client: Arc<dyn LlmClient>) -> anyhow::Result<String> {
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
    Ok(response.parsed_output["greeting"].as_str().unwrap_or_default().to_string())
}
```

The `Arc<dyn LlmClient>` parameter accepts any adapter — swap providers
without changing this function.

## What this crate is

- [`LlmClient`](https://docs.rs/turul-llm-core/latest/turul_llm_core/client/trait.LlmClient.html)
  — one async method, `Send + Sync`, object-safe so adapters compose as
  `Arc<dyn LlmClient>` or `&dyn LlmClient`.
- [`CompletionRequest`](https://docs.rs/turul-llm-core/latest/turul_llm_core/request/struct.CompletionRequest.html)
  — rendered prompt + optional JSON Schema 2020-12 `output_schema` +
  `ExecutionHints` (max tokens, temperature, top-p).
- [`CompletionResponse`](https://docs.rs/turul-llm-core/latest/turul_llm_core/response/struct.CompletionResponse.html)
  — `parsed_output: serde_json::Value`.
- [`LlmError`](https://docs.rs/turul-llm-core/latest/turul_llm_core/error/enum.LlmError.html)
  — `#[non_exhaustive]` taxonomy: `Transport` / `Provider` /
  `SchemaViolation` / `Timeout` / `Other`.

All public structs and the error enum are `#[non_exhaustive]`, so the
contract can grow additively without breaking adopters.

## What this crate is not

- Not an HTTP client. The crate has no `reqwest` / `hyper` / TLS surface.
- Not a streaming layer. The trait is single-shot by design.
- Not a retry / cost / observability layer. Wrap a concrete adapter in
  your own type that also implements `LlmClient` to add those concerns
  without changing the trait.
- Not a JSON Schema validator. Adapters surface
  [`LlmError::SchemaViolation`](https://docs.rs/turul-llm-core/latest/turul_llm_core/error/enum.LlmError.html#variant.SchemaViolation)
  when the provider's structured-output API rejects the payload;
  callers can run their preferred validator on `parsed_output` after
  `complete` returns.

## See also

- [Workspace repository](https://github.com/aussierobots/turul-llm) — full source, ADRs, examples.
- [API documentation](https://docs.rs/turul-llm-core/latest/turul_llm_core/) on docs.rs.
- [CHANGELOG](https://github.com/aussierobots/turul-llm/blob/main/CHANGELOG.md).

## Licensing

Dual-licensed under Apache-2.0 OR MIT.

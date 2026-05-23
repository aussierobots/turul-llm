# Changelog

All notable changes to the `turul-llm` workspace are documented here.
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Format inspired by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.0] — 2026-05-23

Initial workspace scaffold. All crates are `publish = false` for now —
this repo's first crates.io release is gated on real adopter
load-testing (mirrors the discipline applied to `turul-a2a-patterns`
in the sibling `turul-a2a` workspace).

### Added — `turul-llm-core`

- Provider-neutral `LlmClient` trait:
  ```rust
  #[async_trait]
  pub trait LlmClient: Send + Sync {
      async fn complete(&self, request: CompletionRequest)
          -> Result<CompletionResponse, LlmError>;
  }
  ```
- Object-safe (`&dyn LlmClient` works); `Send + Sync` bounds; static
  compile-time assertions in `client::tests`.
- `CompletionRequest { rendered_prompt: String, output_schema: Option<Value>, execution_hints: ExecutionHints }` with `#[non_exhaustive]` for additive evolution. Builder methods (`with_output_schema`, `with_hints`).
- `ExecutionHints { max_tokens: Option<u32>, temperature: Option<f32>, top_p: Option<f32> }` — provider-neutral execution knobs only; provider-specific options live on adapter constructors, not on the request.
- `CompletionResponse { parsed_output: serde_json::Value }` — the validated structured JSON the model returned.
- `LlmError` enum (`#[non_exhaustive]`): `Transport(String)`, `Provider(String)`, `SchemaViolation(String)`, `Timeout`, `Other(String)`. `thiserror` derived.
- No HTTP / runtime dependencies — `turul-llm-core` is the contract surface; adapters live in sibling crates.
- 4 tests covering object-safety, `Send + Sync`, error variants, builder shape.

### Added — `turul-llm-ollama` adapter

- `OllamaClient { base_url, model, http }` implementing `LlmClient`.
- POSTs to `/api/chat` with `model`, `stream=false`, single user message carrying the rendered prompt, `format` field carrying the output schema (Ollama's structured-output mechanism), `options.{num_predict, temperature, top_p}` derived from `ExecutionHints`.
- Decodes the Ollama envelope by parsing `message.content` as JSON.
- 5 wiremock-backed tests pinning request body shape + response decoding. Live mode opt-in via `OLLAMA_BASE_URL` / `RUN_OLLAMA_SMOKE=1` env vars (hermetic CI by default).

### Added — `turul-llm-openai` adapter

- `OpenAiClient { base_url, api_key, model, http }` implementing `LlmClient`.
- POSTs to `/v1/chat/completions` with `Authorization: Bearer <key>`, top-level `model` / `max_tokens` / `temperature` / `top_p`, `response_format = { type: "json_schema", json_schema: { name, schema, strict: true } }`.
- Decodes via `choices[0].message.content` parsed as JSON.
- 4 wiremock-backed tests; live mode would require `OPENAI_API_KEY` and is NOT exercised by default CI.
- **Purpose:** trait-shape validation against a second provider. The trait fit both Ollama and OpenAI 1:1 without revision (rendered prompt → user message; schema → provider's structured-output slot; hints → provider's top-level knobs). Documented in ADR-001.

### Added — `examples/greet-ollama` runnable example

- Takes a name + style, renders a prompt, calls Ollama via the adapter, prints the structured greeting.
- Offline-stub by default; live mode opt-in via the same env vars as the adapter.

### Added — `docs/adr/ADR-001-llmclient-trait-shape.md`

- Records the trait-shape decision (Accepted).
- Documents the two-provider validation strategy (Ollama as primary; OpenAI as second-provider shape-check via wiremock — no live API key required).
- Captures the seed material from `turul-a2a`'s ADR-023 §4 + §7 that motivated this repo's existence.

### Internal

- Workspace dep discipline: all deps via `[workspace.dependencies]` + `{ workspace = true }`. Mirrors the `turul-a2a` workspace convention.
- All crates `publish = false`. First crates.io release pending adopter load-testing (publish gates to be documented when ready).
- `.gitignore` extended for Rust `target/`, `.env`, IDE conventions.

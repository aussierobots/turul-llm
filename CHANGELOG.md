# Changelog

All notable changes to the `turul-llm` workspace are documented here.
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Format inspired by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.1] — 2026-05-23

Patch release adding per-crate README files. These become the
crates.io landing page and the leading content on docs.rs for each
crate. No code changes; the published library API is identical to
0.1.0.

### Added

- `crates/turul-llm-core/README.md` — what the trait is, what it isn't,
  a minimal usage snippet showing `Arc<dyn LlmClient>`.
- `crates/turul-llm-ollama/README.md` — request-to-Ollama mapping,
  hermetic-tests note, live-mode env contract.
- `crates/turul-llm-openai/README.md` — request-to-OpenAI mapping,
  base-URL override for OpenAI-compatible gateways, hermetic-tests
  note.

### Internal

- Workspace version bumped from 0.1.0 to 0.1.1; intra-workspace
  `version` entries in root `Cargo.toml` updated in lockstep per
  CLAUDE.md §Release & Publish step 1.
- `0.1.0` remains published on crates.io (immutable per registry
  policy) and is **not yanked** — `0.1.1` is a documentation-only
  superset and cargo's resolver picks `0.1.1` for any `^0.1`
  consumer.

## [0.1.0] — 2026-05-23

Initial workspace scaffold. The three library crates
(`turul-llm-core`, `turul-llm-ollama`, `turul-llm-openai`) are
mechanically ready for crates.io: `publish = true`, intra-workspace
deps carry both `path` and `version`, and the full pre-publish gate
(test / clippy / fmt / doc / `cargo package`) is green. No `cargo
publish` has been invoked yet — authorization for the first
crates.io upload still requires the project-policy gates from
[`README.md`](README.md) §Roadmap (second non-toy adopter, third-provider
validation, follow-up ADRs on streaming / retries / observability),
mirroring the discipline applied to `turul-a2a-patterns` in the
sibling `turul-a2a` workspace. `examples/greet-ollama` remains
`publish = false` — examples don't ship to crates.io.

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

### Fixed

- `turul-llm-core` rustdoc: qualified intra-doc links to
  `CompletionResponse::parsed_output` in `request.rs` so they resolve;
  dropped a redundant explicit link target on `Value::String` in
  `response.rs`. `cargo doc --no-deps --workspace` is now
  warning-free.

### Internal

- Workspace dep discipline: all deps via `[workspace.dependencies]` + `{ workspace = true }`. Mirrors the `turul-a2a` workspace convention.
- Intra-workspace deps in root `Cargo.toml` now carry both `path` and `version = "0.1.0"` so the published manifest resolves siblings by version while local development still uses the workspace path.
- The three library crates carry `publish = true`; `examples/greet-ollama` stays `publish = false`. `cargo publish` invocations remain individually gated on maintainer authorization.
- `.gitignore` extended for Rust `target/`, `.env`, IDE conventions.

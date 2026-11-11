# Changelog

All notable changes to the `turul-llm` workspace are documented here.
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Format inspired by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- Rewrite the crate-level docs of `turul-llm-core`, `turul-llm-ollama` and `turul-llm-openai` as docs.rs front pages with a compiling example; version 0.1.6 (patch bump of all three crates; no API change).
- Point package homepages and project links to Aussie Robots; add explicit API documentation links and review discovery metadata.


## [0.1.5] — 2026-11-07

Patch release adding opt-in bounded generation (additive; existing calls
are unchanged). Design: `docs/adr/ADR-003-bounded-generation.md`.

### Added

- `ContextBudget` (`NonZeroU32` context window and output cap) on
  `CompletionRequest::with_budget`; `CompletionRequest::max_output_tokens`
  resolves the effective cap. Ollama maps it to `num_ctx`/`num_predict`
  (overriding `with_num_ctx`), OpenAI to `max_tokens`.
- `CompletionResponse::usage` (`Usage`), `stop_reason` (`StopReason`) and
  `token_ids`, filled by both adapters; absent provider values stay `None`.
- `TokenCounter` (async, receives the whole request) and `GuardedClient`
  with a required `PromptCountPolicy`: refuses before sending when the
  prompt plus output cap exceeds the window, and returns an error unless
  generation ended with a clean stop.
- `LlmError` variants `InvalidRequest`, `ContextOverflow`,
  `IncompleteGeneration`, `UsageMissing`, `UsageMismatch`,
  `ModelMismatch`.
- `OllamaClient::with_model_digest`: checks the weights blob digest via
  `/api/show` before each call (models without a local weights blob
  cannot be pinned).
- `OllamaClient::with_generate_endpoint`: templated `/api/generate`, which
  returns token ids (`context`) exposed verbatim as `token_ids`.
- `ChatResponseShape::with_usage_pointers` / `with_token_ids_pointer`.

## [0.1.4] — 2026-11-07

Patch release adding a thinking switch to the Ollama adapter.

### Added

- `OllamaClient::with_think(bool)` emitting the top-level `think`
  field; omitted unless set.
- `docs/adr/ADR-002-shared-chat-decode-and-send.md` recording the shared
  decode/send path introduced in 0.1.3 (`ChatResponseShape`, the `http`
  feature, the reqwest semver rule); root README notes the `http` feature.

## [0.1.3] — 2026-11-07

Patch release removing duplicated response handling between the
adapters.

### Added

- `turul_llm_core::ChatResponseShape` (`#[non_exhaustive]`, JSON-pointer
  locators) with `decode`: the single definition of status handling,
  envelope parsing and the structured-output contract.
- `turul_llm_core::http::send_chat` behind the default-off `http`
  feature (optional `reqwest`): the single send path with timeout and
  transport-error mapping. The adapters enable the feature. The
  module's API follows reqwest's major version (documented in the module
  and README). CI now also checks core without the feature.
- `turul_llm_ollama::DEFAULT_MODEL`, used by `OllamaClient::default` and
  `examples/greet-ollama`.

### Changed

- Both adapters send and decode through `send_chat` and `ChatResponseShape`. Both
  report the provider's finish/done reason in the `SchemaViolation`
  message, labelled `finish_reason=`; Ollama's `done_reason=` label is
  renamed. Non-2xx messages print the numeric status.

## [0.1.2] — 2026-11-07

Patch release: gemma4 structured output was truncated at Ollama's
default context window, which `max_tokens` cannot raise.

### Added

- `OllamaClient::with_num_ctx(u32)` emitting `options.num_ctx`.
- Context-window section in the `turul-llm-ollama` README.

### Changed

- MSRV raised from 1.85 to 1.94; dependencies refreshed within that MSRV.
- Removed unused workspace dependencies `futures` and `url`.
- Added `.github/workflows/ci.yml` (fmt, clippy `-D warnings`, tests, warning-free docs on stable; `cargo check` on the 1.94 MSRV), matching the sibling turul workspaces.
- Added release (fat LTO, one codegen unit) and trimmed dev/test debuginfo profiles, matching the sibling turul workspaces.
- Removed the unused `turul-llm-openai` workspace entry and the redundant `homepage` field.
- Docs: README roadmap and AGENTS.md release flow now reflect the published crates and match CLAUDE.md; stale env-var, API-name and rustdoc claims corrected.
- `turul-llm-ollama`: the `SchemaViolation` message for unparseable
  structured output now includes Ollama's `done_reason`.

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
- `CompletionRequest { rendered_prompt: String, output_schema: Option<Value>, execution_hints: ExecutionHints }` with `#[non_exhaustive]` for additive evolution. Builder methods (`with_output_schema`, `with_execution_hints`).
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

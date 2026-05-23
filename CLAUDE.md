# CLAUDE.md

1. Don't assume. Don't hide confusion. Surface tradeoffs.
2. Minimum code that solves the problem. Nothing speculative.
3. Touch only what you must. Clean up only your own mess.
4. Define success criteria. Loop until verified.

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`turul-llm` is a Rust workspace hosting a **provider-neutral LLM client abstraction** plus per-provider adapters. Licensed under MIT OR Apache-2.0.

**Sibling to `turul-a2a`** (separate repo at `https://github.com/aussierobots/turul-a2a`). The cross-repo split is deliberate — see `docs/adr/ADR-001-llmclient-trait-shape.md`. Short version:

- `turul-a2a` is the A2A protocol implementation. Stays provider-neutral.
- `turul-llm` is the LLM-client abstraction. Different cadence (provider APIs churn quarterly; A2A spec churns annually), different audience (any Rust project that calls LLMs, not just A2A agents), different dependency surface (provider SDKs, HTTP clients, auth).

**Current release:** see `CHANGELOG.md` for the version-by-version contract.

## Build & Development Commands

```bash
cargo build --workspace                   # Build all crates
cargo check --workspace                   # Type-check
cargo test --workspace                    # Run all tests (hermetic; live mode opt-in)
cargo clippy --workspace --all-targets -- -D warnings  # Lint (deny warnings)
cargo fmt --all -- --check                # Format check

# Per-crate tests
cargo test -p turul-llm-core              # Trait + types + errors
cargo test -p turul-llm-ollama            # Ollama adapter (wiremock-backed)
cargo test -p turul-llm-openai            # OpenAI adapter (wiremock-backed)

# Run the example (offline by default; live mode opt-in)
cargo run -p greet-ollama -- Ada formal
OLLAMA_BASE_URL=http://localhost:11434 cargo run -p greet-ollama -- Ada formal
```

**All crate dependencies MUST use `workspace = true`** — versions are managed in root `Cargo.toml` `[workspace.dependencies]`. This includes dev-dependencies. Never put a version number in a crate's own `Cargo.toml` — add the dependency to the workspace root first, then reference it with `{ workspace = true }` in the crate.

## Git Conventions

- Commit messages: succinct, no Co-Authored-By attribution.
- Do not publish or push to remotes unless explicitly asked.
- Version bumps follow SemVer. Per-release classification lives in `CHANGELOG.md`.

## Release & Publish (crates.io)

The workspace ships **three crates**, all currently `publish = false`. First crates.io release is gated on real adopter load-testing (mirrors the discipline applied to `turul-a2a-patterns` in the sibling repo).

When the publish gate is reached, the sequence is the standard turul-style release flow:

1. Bump `[workspace.package].version` in root `Cargo.toml`. Crates inherit via `version.workspace = true`. Intra-workspace deps that carry an explicit `version` field need updating in lockstep.
2. Write the `CHANGELOG.md` entry.
3. Run pre-publish gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo doc --no-deps --workspace`, `cargo package --no-verify --allow-dirty` per crate.
4. Commit release-prep changes (`cargo publish` refuses a dirty tree).
5. After explicit authorization: publish in dependency order — `turul-llm-core` first, then `turul-llm-ollama` and `turul-llm-openai` (any order; neither depends on the other). Each `cargo publish` invocation needs its own authorization.

## Architecture

### Crate Structure

- `turul-llm-core` — provider-neutral `LlmClient` trait + `CompletionRequest` / `CompletionResponse` / `ExecutionHints` types + `LlmError` taxonomy. No HTTP / runtime deps. The contract surface.
- `turul-llm-ollama` — Ollama `/api/chat` adapter. Structured output via the `format` field. Live mode opt-in via env (`OLLAMA_BASE_URL` / `RUN_OLLAMA_SMOKE=1`); hermetic CI by default.
- `turul-llm-openai` — OpenAI-compatible adapter via `response_format = { type: "json_schema", ... }`. Live mode requires `OPENAI_API_KEY` and is not exercised by default CI.
- `examples/greet-ollama` — runnable example demonstrating the trait + Ollama adapter end-to-end.

### Trait Shape

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, request: CompletionRequest)
        -> Result<CompletionResponse, LlmError>;
}
```

- `CompletionRequest` carries `rendered_prompt: String`, `output_schema: Option<Value>` (JSON Schema 2020-12 for structured output), `execution_hints: ExecutionHints`.
- `CompletionResponse` carries `parsed_output: Value` (the validated structured JSON).
- `LlmError` is `#[non_exhaustive]` — adapters classify errors as `Transport` / `Provider` / `SchemaViolation` / `Timeout` / `Other`.

The trait shape was validated against both Ollama and OpenAI without revision. The single `rendered_prompt` field intentionally collapses the system/user-prompt distinction; if a future provider or adopter workflow needs structured prompts, the `#[non_exhaustive]` request struct makes additive evolution safe.

### Cross-Repo Boundary (with `turul-a2a`)

- `turul-llm` does NOT depend on `turul-a2a`. The other direction is also forbidden by `turul-a2a`'s own discipline.
- `turul-a2a` example agents MAY consume `turul-llm` crates via local path dep once this repo ships its first stable releases. Until then, provider calls in `turul-a2a` examples are inline (e.g. the manifest-based Ollama example in `turul-a2a` carries its own Ollama call).
- Future ADRs about LLM-client design live in **this repo's** `docs/adr/`, not in `turul-a2a`.

### Architecture Decision Records

Long-form rationale lives under `docs/adr/`. ADR refs are durable internal anchors **for ADRs cross-referencing each other** — keep them out of source code (see "Comment and Docstring Style" below).

For non-trivial architecture changes, the ADR should be Accepted before implementation starts. The acceptance is its own commit, separate from the implementation commit, so `git log` shows the gate.

### Comment and Docstring Style

**Comments are human-facing documentation.** They must add value to a reader who has the code in front of them and no access to planning history, ADR section numbering, or internal review threads. The bar: a reader six months from now, with no project context, can act on the comment.

**Do not reference in code (comments, docstrings, rustdoc):**

- ADR numbers or sections (e.g. `ADR-001 section 2`).
- Phase / slice / wave / step labels.
- Issue / PR / task numbers.
- Internal review history.

These rot. They mean something to whoever wrote them this week and **nothing** to a reader later.

**Do write:**

- The invariant: "Trait must stay object-safe — adopters compose adapters as `&dyn LlmClient`."
- The contract: "Live mode is opt-in via `OLLAMA_BASE_URL`; default test path uses wiremock to keep CI hermetic."
- The "why" in timeless terms.
- Upstream provider citations (Ollama / OpenAI API docs with stable URLs) when the comment would be incomplete without them. Sparingly.

**Where ADR / phase / planning references DO belong:**

- Commit messages.
- `CHANGELOG.md` entries.
- ADR cross-references within other ADRs.
- `README.md` files (explicit human-facing documentation; ADR links here are useful navigation).

### Test Discipline

Tests use `wiremock` to pin the request shape each adapter emits and the response shape each provider returns. **Default test path is hermetic** — `cargo test --workspace` never requires a live Ollama or live OpenAI. Live mode is opt-in via per-adapter env vars.

When adding a new provider adapter:

1. Mirror the wiremock pattern from `turul-llm-ollama::tests` or `turul-llm-openai::tests`.
2. Pin: request URL, request body shape (headers + JSON), response decoding.
3. Document the live-mode env var convention in the adapter's `lib.rs` rustdoc.

### Adopter Surface

The `LlmClient` trait is the only public surface adopters depend on. Adapter types (`OllamaClient`, `OpenAiClient`) are configuration containers; adopters typically:

```rust
let client: Arc<dyn LlmClient> = Arc::new(OllamaClient::new("http://localhost:11434", "llama3.1"));
let resp = client.complete(req).await?;
```

Provider-specific options live on adapter constructors (`OllamaClient::with_options(...)`, `OpenAiClient::with_organization(...)` etc.), not on `CompletionRequest`.

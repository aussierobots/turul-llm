# AGENTS.md

Guidance for AI coding agents (Codex, Copilot CLI, Gemini CLI, others) working in this repository. Claude Code reads `CLAUDE.md` for the same content; both files are kept in lockstep.

## Working Rules

1. Don't assume. Don't hide confusion. Surface tradeoffs.
2. Minimum code that solves the problem. Nothing speculative.
3. Touch only what you must. Clean up only your own mess.
4. Define success criteria. Loop until verified.

## Project Overview

`turul-llm` is a Rust workspace hosting a **provider-neutral LLM client abstraction** plus per-provider adapters. Licensed under MIT OR Apache-2.0.

**Sibling to `turul-a2a`** (separate repo). Cross-repo split rationale lives in `docs/adr/ADR-001-llmclient-trait-shape.md`:

- `turul-a2a` is the A2A protocol implementation. Stays provider-neutral.
- `turul-llm` is the LLM-client abstraction. Different cadence, audience, and dependency surface.

See `CHANGELOG.md` for current release.

## Build & Development Commands

```bash
cargo build --workspace                   # Build
cargo check --workspace                   # Type-check
cargo test --workspace                    # Hermetic test suite (live mode opt-in)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

cargo test -p turul-llm-core              # Per-crate
cargo test -p turul-llm-ollama
cargo test -p turul-llm-openai

cargo run -p greet-ollama -- Ada formal
OLLAMA_BASE_URL=http://localhost:11434 cargo run -p greet-ollama -- Ada formal
```

## Workspace Dependency Discipline

All crate dependencies MUST go through `[workspace.dependencies]` in the root `Cargo.toml` and be consumed with `{ workspace = true }` in each member crate. No version strings inside member `Cargo.toml` files. This includes dev-dependencies.

When adding a new external dep:

1. Add the version to `[workspace.dependencies]` in the root `Cargo.toml`.
2. Reference it as `dep = { workspace = true }` in the consuming crate.
3. If features are crate-specific, use `dep = { workspace = true, features = ["x"] }`.

## Git Conventions

- Commit messages: succinct, factual, no Co-Authored-By attribution.
- Do not publish or push to remotes unless explicitly asked.
- Version bumps follow SemVer. Patch = compatible runtime; minor = contract change; major = breaking architecture.

## Release & Publish (crates.io)

All three crates ship with `publish = false` until a publish gate is reached (mirrors `turul-a2a-patterns` in the sibling repo).

When the gate is reached, the sequence is the standard turul-style release flow:

1. Bump `[workspace.package].version` in root `Cargo.toml`.
2. Update intra-workspace deps that carry an explicit `version` field.
3. Write `CHANGELOG.md` entry.
4. Pre-publish gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo doc --no-deps --workspace`, `cargo package --no-verify --allow-dirty` per crate.
5. Commit release-prep changes.
6. After explicit authorization: publish in dependency order — `turul-llm-core` first, then `turul-llm-ollama` and `turul-llm-openai` (no inter-dependency, any order).

Each `cargo publish` invocation needs its own authorization.

## Architecture

### Crate Structure

- `turul-llm-core` — `LlmClient` trait + request/response/error types. No HTTP or runtime deps.
- `turul-llm-ollama` — Ollama `/api/chat` adapter. Structured output via `format` field.
- `turul-llm-openai` — OpenAI-compatible adapter via `response_format = { type: "json_schema", ... }`.
- `examples/greet-ollama` — runnable end-to-end example.

### Trait Shape

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, request: CompletionRequest)
        -> Result<CompletionResponse, LlmError>;
}
```

The trait stays object-safe (`&dyn LlmClient` / `Arc<dyn LlmClient>` work). `CompletionRequest` and `LlmError` are `#[non_exhaustive]` for additive evolution. Provider-specific options live on adapter constructors, not on the request.

### Cross-Repo Boundary (with `turul-a2a`)

- `turul-llm` does NOT depend on `turul-a2a`. The other direction is also forbidden by `turul-a2a`'s own discipline.
- `turul-a2a` example agents MAY consume `turul-llm` via local path dep once this repo ships its first stable release.
- Future ADRs about LLM-client design live in **this repo's** `docs/adr/`, not in `turul-a2a`.

### ADRs

Long-form rationale lives under `docs/adr/`. For non-trivial architecture changes, the ADR should be Accepted before implementation starts. The acceptance is its own commit.

### Comment and Docstring Style

Comments serve readers six months from now with no project context.

**Do not reference in code (comments, docstrings, rustdoc):**

- ADR numbers or section refs.
- Phase / slice / wave / step labels.
- Issue / PR / task numbers.
- Internal review history.

**Do write:**

- The invariant.
- The contract.
- The "why" in timeless terms.
- Upstream provider citations (stable URLs) when essential.

**ADR / planning refs DO belong in:** commit messages, `CHANGELOG.md`, ADR cross-refs, `README.md`.

### Test Discipline

`wiremock` pins request/response shape for each adapter. Default `cargo test --workspace` is hermetic — no live Ollama or live OpenAI required. Live mode is opt-in via per-adapter env vars.

New adapters: mirror the wiremock pattern, pin URL + body + decoding, document the live-mode env var convention in adapter rustdoc.

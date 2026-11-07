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
cargo test --workspace                    # Hermetic test suite
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

The three library crates publish to crates.io; `examples/greet-ollama` stays `publish = false`. For 0.2+ ships, re-confirm the project gates in `README.md` §Roadmap before bumping.

1. Bump `[workspace.package].version`. Intra-workspace deps in `[workspace.dependencies]` carry both `path` and `version` — keep in lockstep.
2. Update `CHANGELOG.md`.
3. Pre-publish gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo doc --no-deps --workspace` (warning-free), `cargo package --no-verify --allow-dirty -p turul-llm-core`. Adapter packaging is deferred to `cargo publish --verify` (registry chicken-and-egg).
4. Commit release-prep, push to `origin/main`.
5. Tag and push **before** publish: `git tag -a vX.Y.Z <commit> -m "..."`, `git push origin vX.Y.Z`. Annotated tags only.
6. After one authorization, publish the batch in dep order: `-p turul-llm-core` first, then `-p turul-llm-ollama` and `-p turul-llm-openai`. One authorization covers all three.

If a publish fails: crates.io is immutable per-crate, so prior successful uploads stay — fix and rerun only the failed ones. If a tag points at the wrong commit, `git push --delete origin vX.Y.Z`, re-tag, push.

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

`wiremock` pins request/response shape for each adapter. Default `cargo test --workspace` is hermetic — no live Ollama or live OpenAI required. Live mode is opt-in in `examples/greet-ollama` via env vars; the adapters never read env vars.

New adapters: mirror the wiremock pattern, pin URL + body + decoding, adapters take configuration through constructors and never read env vars.

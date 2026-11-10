# turul-llm

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/turul/logo-dark.png">
  <img src="docs/assets/turul/logo-light.png" alt="Turul LLM" width="600">
</picture>


A provider-neutral LLM client trait and adapters for Rust. Sibling
workspace to [`turul-a2a`](https://github.com/aussierobots/turul-a2a) —
they share an author and a design vocabulary, but they ship on
independent cadences because LLM provider APIs churn faster than the A2A
spec.

This repository is the home of:

| Crate | Role |
|---|---|
| `turul-llm-core` | Provider-neutral `LlmClient` trait + request/response types + error taxonomy. Zero provider deps; an optional, default-off `http` feature adds the send path the adapters share. |
| `turul-llm-ollama` | Ollama adapter targeting `/api/chat` (or `/api/generate`) with structured-output `format` field. |
| `turul-llm-openai` | OpenAI-compatible adapter targeting `/chat/completions` with `response_format = json_schema`. |
| `examples/greet-ollama` | Runnable example: offline stub by default, live Ollama via env vars. |

## What this repo is

A small, focused abstraction over "send a rendered prompt + optional
JSON Schema → get a parsed structured output". The trait is one method.
The crate has no SDK pin, no retry policy, no streaming layer, no token
budgeter. Adapters live in sibling crates so the trait stays cheap to
depend on.

The shape of the trait is documented in [docs/adr/ADR-001-llmclient-trait-shape.md](docs/adr/ADR-001-llmclient-trait-shape.md); the response decode and send path the adapters share is in [docs/adr/ADR-002-shared-chat-decode-and-send.md](docs/adr/ADR-002-shared-chat-decode-and-send.md).

## What this repo is not

- **Not an A2A implementation.** A2A protocol, dispatch, transports,
  storage, and the agent runtime all live in
  [`turul-a2a`](https://github.com/aussierobots/turul-a2a). This repo
  knows nothing about agents.
- **Not a competitor to `ollama-rs`, `async-openai`, or Anthropic's
  SDK.** Those crates implement provider transports; the adapters here
  define a shared shape so adopter code can swap them at runtime.
- **Not a retry / observability / cost-tracking layer.** Those are
  cross-cutting concerns the trait deliberately defers — a wrapper
  client that decorates an inner `LlmClient` can add them without
  changing the trait surface.

## Run the example

Offline stub — no network, no Ollama running:

```bash
cargo run -p greet-ollama
cargo run -p greet-ollama -- Ada formal
```

Live Ollama — requires a reachable Ollama server with the chosen model
pulled:

```bash
OLLAMA_BASE_URL=http://localhost:11434 \
OLLAMA_MODEL=llama3.1 \
cargo run -p greet-ollama -- Ada formal

# Or use the canonical opt-in flag that defaults to localhost:11434
RUN_OLLAMA_SMOKE=1 cargo run -p greet-ollama -- Ada formal
```

The example prints the rendered prompt and the parsed structured JSON
output.

## Test

The full suite is hermetic — no live providers required:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Adapter tests use `wiremock` to stub the provider HTTP surface; the core
crate's tests are pure unit tests.

## Workspace discipline

All crate dependencies (internal and external) flow through the
workspace root: declare in `[workspace.dependencies]`, then reference
with `{ workspace = true }` in each crate. Version drift across crates
is a defect — fix at the root.

## Roadmap

The three library crates (`turul-llm-core`, `turul-llm-ollama`,
`turul-llm-openai`) are published on crates.io. The trait shape has been
validated against two providers (Ollama + OpenAI) under `wiremock`.
Before a 0.2 release, the following should land:

1. A second non-toy adopter beyond `examples/greet-ollama` exercises the
   trait end-to-end (live, not stubbed).
2. The trait holds up against a third provider with a materially
   different request/response shape (Anthropic content blocks is the
   obvious candidate).
3. Decisions on streaming, retries, and observability are landed as
   follow-up ADRs in this repo's `docs/adr/`.

The trait may evolve until then. Per-release changes are in
[`CHANGELOG.md`](CHANGELOG.md).

## Licensing

Dual-licensed under Apache-2.0 OR MIT, matching the wider `turul-*`
ecosystem.

## Brand artwork

Turul brand artwork is © 2026 Toast Technology Pty Ltd, all rights reserved,
and is excluded from the software licence. See the [artwork notice](docs/assets/turul/ARTWORK-NOTICE.md) and [master assets](https://github.com/aussierobots/turul-brand).

[Project icons and artwork](docs/assets/turul/README.md).

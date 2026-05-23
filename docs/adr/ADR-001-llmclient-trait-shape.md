# ADR-001: LlmClient trait shape

- **Status:** Accepted
- **Date:** 2026-05-23
- **Depends on:** none (first ADR in this repo)

## Context

This is `turul-llm`'s first ADR. The repo exists because
[`turul-a2a`'s ADR-023](https://github.com/aussierobots/turul-a2a/blob/main/docs/adr/ADR-023-llmclient-abstraction.md)
decided that a provider-neutral LLM client abstraction should live
**outside** the A2A workspace — different cadence, different audience,
different dependency surface. That ADR sketched a possible trait
shape; this ADR locks in the shape that ships in this workspace.

The seed shape from ADR-023 §4 was a three-argument signature:

```rust
fn complete(&self, prompt: String, output_schema: Value, provider_config: Value)
    -> LlmFuture<'a, Result<Value, LlmError>>;
```

Two things changed in the move into this repo:

1. `provider_config` is no longer part of the trait. The opaque
   per-provider config block belongs on the **adapter constructor**
   (model name, endpoint, API key), not on the per-call signature.
   Callers that need to switch providers per-call can hold
   `Arc<dyn LlmClient>` and route based on configuration.
2. The signature is normal `async fn` via `#[async_trait]`. The
   hand-rolled `LlmFuture<'a, T>` alias from ADR-023 §4 was motivated
   by AFIT not-yet-supporting the `Send` bound; with `async-trait` the
   ergonomics are simpler and the trait stays object-safe.

## Decision

The trait surface is:

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, request: CompletionRequest)
        -> Result<CompletionResponse, LlmError>;
}
```

With supporting types:

- `CompletionRequest { rendered_prompt: String, output_schema: Option<Value>, execution_hints: ExecutionHints }`
- `CompletionResponse { parsed_output: Value }`
- `ExecutionHints { max_tokens: Option<u32>, temperature: Option<f32>, top_p: Option<f32> }`
- `LlmError` — `#[non_exhaustive]`, variants: `Transport`, `Provider`,
  `SchemaViolation`, `Timeout`, `Other`.

All public types are `#[non_exhaustive]` so additive fields don't break
matchers. The trait is `Send + Sync` and object-safe; a compile-time
assertion lives in `turul-llm-core::client::tests`.

### Why a request struct, not a wider method signature

A single `CompletionRequest` argument keeps the trait one method and
makes additive fields (e.g. a future `tool_definitions`, `stop_tokens`,
`seed`) zero-break-change for implementers — they ignore unknown
fields, the type stays `#[non_exhaustive]`, callers opt in. The
alternative (positional arguments) would force every adapter signature
change to ripple through the workspace.

### Why `output_schema` is `Option<Value>`

Both supported providers treat structured output as opt-in. When
`None`, the adapter falls back to free-form completion and wraps the
returned text as `Value::String` so `parsed_output` is always
populated. This keeps the return type stable across "I want JSON" and
"I want raw text" callers.

### Why no system-prompt distinction in v1

The seed shape carried `rendered_prompt: String` and left
system-vs-user role assignment to the adapter. We kept that. Both
providers we validated accept a single user message and produce the
expected behaviour; neither required a system prompt to make the
structured-output contract work. If a future provider (or adopter
workflow) needs a structured prompt input, the path forward is a
follow-up ADR that introduces a richer `PromptInput` type — the
`#[non_exhaustive]` request struct makes that additive.

## Two-provider validation strategy

The trait shape is locked in only after it survives a second provider
with a materially different structured-output contract.

- **Primary provider: Ollama.** Adapter targets `/api/chat`, passes
  the schema through as the `format` field, parses the JSON-encoded
  string returned in `message.content`. Tests use `wiremock` and pin
  the request body shape + response decoding.
- **Second provider: OpenAI-compatible.** Adapter targets
  `/chat/completions`, passes the schema through as
  `response_format = { type: "json_schema", json_schema: { name, schema, strict: true } }`,
  parses the JSON-encoded string returned in `choices[0].message.content`.
  Tests use `wiremock`.

The OpenAI adapter validated the trait shape cleanly:

- Request body translates 1:1 — rendered prompt as the user message,
  schema mapped to `response_format.json_schema.schema`, execution
  hints mapped to top-level `max_tokens` / `temperature` / `top_p`.
- Response decoding follows the same "extract content string → parse
  as JSON" pattern. The path differs (`choices[0].message.content` vs
  `message.content`) but the contract doesn't.
- No trait revision was needed.

Anthropic content blocks, tool-call schemas, and multi-modal inputs
remain open questions deferred to future ADRs; if they reveal a
genuine trait-shape mismatch (e.g. content blocks can't be flattened
to a string without information loss), a follow-up ADR amends the
trait additively.

## Structured-output / JSON-Schema discipline

Adapters must forward `output_schema` to the provider's
structured-output API when it's present, and they must not loosen what
the caller's schema declares. When the provider returns content that
fails to parse as JSON under a structured-output request, the adapter
returns `LlmError::SchemaViolation` — callers can recognise that as a
distinct failure mode from "transport broken" or "provider returned
500". JSON Schema validation of the parsed value remains the caller's
responsibility; the trait does not pick a validator library.

## Out of scope for v1

These are intentionally deferred. Each gets its own ADR if and when an
adopter reports a real need.

- Streaming token output. Would require a second method
  (`stream_complete`) or a different return type; both would weaken
  object safety. Defer until at least one adopter has a streaming use
  case that can't be served by chunked manifest fields or polling.
- Retry policy. Belongs in a wrapper client
  (`RetryingLlmClient<C: LlmClient>`), not on the trait.
- Token-count / cost-attribution metrics. Would add an optional
  `LlmUsage` field to `CompletionResponse`. Defer — adopters can
  attach telemetry in a wrapper today.
- Tool-call / function-call schemas. Provider contracts vary widely
  (OpenAI tool_calls, Anthropic content blocks). Locking this into v1
  before a third provider exists risks shaping the trait around two
  patterns and breaking on the third.

## Reopening / scope changes

Future amendments to the trait shape land as new ADRs in this repo
(ADR-002, ADR-003, ...). This ADR stays Accepted as the historical
record of the v1 shape.

If a future provider integration reveals that the single
`rendered_prompt: String` field is insufficient (the most likely
trigger is a multimodal input or a system/user split that can't be
expressed in plain text), the path forward is a new ADR that
introduces a richer `PromptInput` type and a corresponding additive
field on `CompletionRequest`. The trait remains backward-compatible
because the existing `rendered_prompt` field stays valid.

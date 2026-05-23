# greet-ollama

A tiny, runnable example that exercises the `LlmClient` trait end-to-end
against Ollama, and degrades gracefully to an offline stub when no
Ollama server is reachable.

This example lives in the [`turul-llm`](../../README.md) workspace and
exists to answer one question for adopters:

> What does the smallest useful call through `turul-llm-core` +
> `turul-llm-ollama` actually look like?

The answer is in [`src/main.rs`](src/main.rs) — about 100 lines, no
hidden framework. Read it alongside this README.

## What it does

1. Renders a one-shot prompt asking the model to greet someone by name
   in a given style.
2. Attaches a JSON Schema (`{ greeting: string }`) so the response is
   constrained to structured output.
3. Sends the request through an `Arc<dyn LlmClient>` — either the real
   `OllamaClient` or an in-process `OfflineStub`, chosen at runtime.
4. Pretty-prints the parsed JSON response.

The deliberate point: the calling code (`main`) doesn't know or care
which client it has. Swapping providers is a constructor change.

## Run it

### Offline (default — no network, no Ollama needed)

```bash
cargo run -p greet-ollama
cargo run -p greet-ollama -- Ada formal
```

The offline stub returns a deterministic payload that satisfies the
output schema. Use this to verify wiring, inspect the request shape,
or run the example in CI without a live model.

Sample output:

```
Mode: offline-stub
Prompt: Produce a JSON object with a single field `greeting` that warmly greets a person named Ada. The style hint is `formal`. Respond with JSON only — no prose.
Response: {
  "greeting": "Hi, Ada! (offline stub)"
}
```

### Live Ollama (opt-in)

Requires a reachable Ollama server with the chosen model already
pulled (`ollama pull llama3.1`).

```bash
# Explicit base URL
OLLAMA_BASE_URL=http://localhost:11434 \
cargo run -p greet-ollama -- Ada formal

# Or the canonical opt-in flag (defaults base URL to localhost:11434)
RUN_OLLAMA_SMOKE=1 cargo run -p greet-ollama -- Ada formal

# Pick a different model
OLLAMA_BASE_URL=http://localhost:11434 \
OLLAMA_MODEL=qwen2.5 \
cargo run -p greet-ollama -- Grace casual
```

The example POSTs to `/api/chat` and decodes the JSON the model
returns under the schema.

## Arguments

| Position | Name    | Default    | Meaning                                  |
|----------|---------|------------|------------------------------------------|
| 1        | `name`  | `Ada`      | Person to greet — interpolated into the prompt. |
| 2        | `style` | `casual`   | Style hint passed to the model.          |

## Environment variables

| Variable           | Effect                                                                                  |
|--------------------|-----------------------------------------------------------------------------------------|
| `OLLAMA_BASE_URL`  | If set and non-empty, switches the example into live mode targeting this URL.           |
| `RUN_OLLAMA_SMOKE` | If `1`, switches into live mode against `http://localhost:11434`.                       |
| `OLLAMA_MODEL`     | Model name to request. Defaults to `llama3.1`.                                          |
| `RUST_LOG`         | Standard `tracing-subscriber` filter, e.g. `RUST_LOG=turul_llm_ollama=debug`.            |

The live-mode env convention matches the
[`turul-llm-ollama`](../../crates/turul-llm-ollama) adapter's own test
gate — learn it once, use it everywhere.

## What to look at in the source

- **`render_prompt`** — string templating, nothing more. The trait
  takes a `rendered_prompt: String`; prompt composition is the
  caller's job.
- **`output_schema`** — a JSON Schema 2020-12 object. The Ollama
  adapter forwards this to the `format` field; the OpenAI adapter
  would forward it as `response_format = json_schema`. The schema is
  the portable contract.
- **`OfflineStub`** — about ten lines. Implementing `LlmClient` for
  a test double is intentionally cheap; the trait is one method.
- **`main`** — selects a client behind `Box<dyn LlmClient>` and
  calls `.complete(...)`. This is the full adopter surface.

## When this example stops being enough

This example covers the happy path for a single, one-shot, structured
request. It does **not** cover:

- Streaming (the trait is non-streaming by design — see
  [ADR-001](../../docs/adr/ADR-001-llmclient-trait-shape.md)).
- Retries, timeouts beyond `ExecutionHints`, or fallbacks between
  providers — compose a wrapper `LlmClient` that decorates an inner
  one.
- Cost / token accounting — same pattern.

When adopters need any of those, they wrap a concrete adapter in their
own type that also implements `LlmClient`. Nothing in the trait blocks
that.

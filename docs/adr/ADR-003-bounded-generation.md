# ADR-003: Bounded generation — budgets, usage accounting, guarded client, model pinning

- **Status:** Accepted
- **Date:** 2026-11-07
- **Depends on:** ADR-001, ADR-002

## Context

Callers that must never act on a silently truncated completion, and must
be able to show what the model actually received, cannot be served by the
current contract:

- Nothing refuses a request whose prompt plus output cannot fit the
  context window.
- `CompletionResponse` carries only `parsed_output`. Token counts and the
  stop reason are discarded; a `length` stop on free-form text is a
  success.
- Context size and output size are optional hints, and for Ollama the
  context size has no per-request path.
- Nothing ties a client to a specific set of model weights; a mutable tag
  can change underneath it.

Facts this ADR relies on (measured on Ollama 0.40.11 with
`OLLAMA_NUM_PARALLEL=8`):

- `/api/chat` and `/api/generate` return `prompt_eval_count`, `eval_count`
  and `done_reason`.
- A prompt longer than `num_ctx` is **not rejected**: the server truncates
  it, sets `done_reason: "length"` and reports the **truncated**
  `prompt_eval_count`. Refusing before sending is therefore the only sound
  guard.
- `prompt_eval_count` includes the model's chat template and any default
  system text, so it is larger than a tokenisation of the bare prompt
  (27 tokens for a two-word prompt on llama3.2; 12 on qwen3). A counter
  that sees only the bare prompt can never match it.
- `num_ctx` is the per-request window; it is not divided by
  `OLLAMA_NUM_PARALLEL`. A per-request `num_ctx` that differs from the
  loaded model's window makes Ollama reload the model.
- Thinking tokens count against both the output cap and the context window.
- A non-positive `num_predict` is treated as unset (unbounded).
- `/api/show` returns a `modelfile` whose first non-comment `FROM` line
  names the weights blob as `sha256-<hex>`. That digest differs from the
  `digest` in `/api/tags`.

## Decision

Everything below is additive and opt-in. Existing calls, signatures and
error behaviour are unchanged.

### Core (provider-neutral)

1. **`ContextBudget`** — `ContextBudget::new(context_tokens: NonZeroU32,
   max_output_tokens: NonZeroU32)`. Both values are mandatory by
   construction; zero cannot be expressed.
   `CompletionRequest::with_budget(budget)` attaches it.
   `CompletionRequest::max_output_tokens()` is the single place that
   resolves the effective output cap (budget first, then
   `ExecutionHints::max_tokens`); adapters call it.
   `context_tokens` means the per-request window the provider will
   actually give; both fields include thinking tokens.
2. **`TokenCounter`** — an `#[async_trait]` trait,
   `count(&self, request: &CompletionRequest) -> Result<u32, LlmError>`.
   It receives the whole request so the caller's implementation can apply
   the served model's template, system text and tokenizer. The library
   ships no tokenizer.
3. **Response accounting** — `CompletionResponse` gains
   `usage: Option<Usage>` (`prompt_tokens`, `output_tokens`, each
   `Option<u32>`) and `stop_reason: Option<StopReason>`
   (`Stop`, `Length`, `Other(String)`; `#[non_exhaustive]`).
   `CompletionResponse::new` is unchanged and leaves both `None`.
   `ChatResponseShape` grows through builder methods
   (`with_usage_pointers`, existing finish-reason locator); the stop-reason
   mapping (`stop` → `Stop`, `length` → `Length`, anything else →
   `Other`) is defined once in `decode`. Absent in the provider response
   means `None`, never a guessed value.
4. **`GuardedClient`** — `GuardedClient::new(inner, counter, policy)`
   wraps any `LlmClient` (and is itself one). `policy` is required, with
   no default: `PromptCountPolicy::Exact` or `PromptCountPolicy::Ignore`.
   `complete` then:
   - refuses a request without a `ContextBudget`
     (`LlmError::InvalidRequest`); sizing is explicit per call;
   - counts the prompt with the counter and refuses when
     `prompt_tokens + max_output_tokens > context_tokens`
     (`LlmError::ContextOverflow`) before any call is made;
   - after the call, returns `LlmError::IncompleteGeneration` unless
     `stop_reason` is `Stop` (`Length`, `Other` and absent all fail);
   - under `Exact`, returns `LlmError::UsageMissing` if the provider
     reported no prompt count and `LlmError::UsageMismatch` if it differs
     from the local count.
   The guard reads `context_tokens` only from the budget; it never queries
   the provider.
5. **New `LlmError` variants**, all struct-like and `#[non_exhaustive]`:
   `InvalidRequest`, `ContextOverflow`, `IncompleteGeneration`,
   `UsageMissing`, `UsageMismatch`, `ModelMismatch`.

### Adapters

6. **Both adapters** fill `usage` and `stop_reason` (Ollama:
   `/prompt_eval_count`, `/eval_count`, `/done_reason`; OpenAI:
   `/usage/prompt_tokens`, `/usage/completion_tokens`,
   `/choices/0/finish_reason`) and map the budget. OpenAI sends
   `max_tokens` from `max_output_tokens()` (it has no per-call window;
   `context_tokens` is used only by the guard). Ollama sends
   `options.num_ctx = context_tokens` and `num_predict =
   max_output_tokens()`; a request budget overrides
   `OllamaClient::with_num_ctx`, and a `num_ctx` that differs from the
   resident model's window reloads it.
7. **Model pinning (Ollama)** — `OllamaClient::with_model_digest(hex)`
   pins the client to a weights digest. Before each call the client
   `POST`s `/api/show` for the client's model, takes the `sha256-<hex>` of
   the first non-comment `FROM` line of `modelfile` (`ADAPTER` lines are
   ignored), and returns `LlmError::ModelMismatch` if it differs. Limits,
   stated in the API docs: it identifies the weights only, not the
   template, system text or parameters; and the model can change between
   the check and the call.

## Deferred

- **Token ids and `/api/generate`.** Ollama's `context` field is the only
  source of prompt token ids, it is only returned by `/api/generate`, it
  holds ids before truncation (so it cannot prove what was evaluated), and
  its BOS/EOS handling differs by model family. The count-level proof
  (`Exact` against a template-aware counter) is delivered now; a
  token-level proof needs a stable provider contract first.
- **Raw mode (`raw: true`).** Returns no `context` and moves template
  correctness onto every caller; it cannot provide the proof it would be
  used for.

## Consequences

- Strictness lives in one place (`GuardedClient`) and is opt-in; plain
  clients behave as before.
- Pinning costs one extra HTTP request per call; callers that do not pin
  pay nothing.
- `Exact` is only meaningful with a counter that models the served
  template; with a bare-prompt counter it fails by design, which is why
  the policy is an explicit argument.
- Version: all changes are additive (non-exhaustive types and enums), so
  the release is a patch bump.

## Alternatives rejected

- **Making `ExecutionHints::max_tokens` / `num_ctx` required**: breaks
  every existing caller. A type that cannot be built without both
  (`ContextBudget`) gives mandatory sizing to callers that opt in.
- **A synchronous `count(&str)` counter**: counts a string the server
  never tokenises, and cannot call a remote tokenizer.
- **Shipping tokenizers in this repo**: ties the contract crate to model
  families and weight files.
- **Resolving the served tag from the digest**: needs a scan of the model
  list and a second lookup per call; refusing on mismatch is sufficient.
- **Verifying identity from the local filesystem**: only works when the
  caller shares a host with the server; `/api/show` works over HTTP.
- **A default budget on the guard**: sizing must be explicit per call.

# ADR-002: Shared chat decode and send path in turul-llm-core

- **Status:** Accepted
- **Date:** 2026-11-07
- **Depends on:** ADR-001

## Context

The Ollama and OpenAI adapters each carried the same post-request logic:
send, map a timeout to `LlmError::Timeout` and other failures to
`LlmError::Transport`, read the body, map a non-2xx status to
`LlmError::Provider`, parse the envelope, extract the content string, and
either parse it as JSON (structured output) or wrap it as a string. Only
the request body, the auth header and the JSON pointers to the content
and finish reason differed. Two copies drift: one adapter reported the
provider's finish reason on truncated structured output and the other
did not.

## Decision

`turul-llm-core` owns one definition of that path.

- `ChatResponseShape` (`decode` module) holds the two JSON pointers
  (content, finish reason) and `decode(url, status, body, structured)`
  implements status handling, envelope parsing and the structured-output
  contract. It is `#[non_exhaustive]` so further locators can be added
  without a break. It has no HTTP dependency.
- `http::send_chat` (`http` module) takes the adapter's built
  `reqwest::RequestBuilder`, sends it, maps transport errors, reads the
  body and calls `decode`. It lives behind the default-off `http` feature,
  which enables an optional `reqwest` dependency. The adapters enable the
  feature; a crate that depends on core only for the trait gets no HTTP
  client.
- When structured output fails to parse, the `SchemaViolation` message
  carries the provider's finish reason as `finish_reason=<value>`
  (`unknown` if absent). A reason such as `length` identifies truncation
  rather than a model that ignored the schema.

Adapters keep everything provider-specific: URL, headers, request body,
the pointers passed to `ChatResponseShape`, and provider options on their
constructors (`OllamaClient::with_num_ctx`, `with_think`).

## Consequences

- **Semver:** `http::send_chat` exposes `reqwest::RequestBuilder`, so the
  `http` feature's public API follows reqwest's major version. A reqwest
  major bump is a breaking change of that feature, not of the trait,
  types or errors. This is stated in the module docs.
- Adopters of the trait are unaffected; the shared items are adapter
  plumbing and adopters do not need them.
- Error message text is human context, not a machine contract (ADR-001).
  The message wording changed with this ADR: non-2xx errors print the
  numeric status, and the truncation label is `finish_reason=`.
- A new chat-style adapter supplies its request and two pointers and gets
  the same error and structured-output behaviour.

## Alternatives rejected

- **A new `turul-llm-http` crate** owning the send path: a new published
  crate for about fifteen lines.
- **Decoder only, send path duplicated in each adapter:** leaves the
  transport classification defined twice.
- **A free function with positional arguments:** two untyped pointer
  strings would be frozen into the contract crate; the struct allows
  additive evolution.

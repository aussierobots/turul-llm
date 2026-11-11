//! Provider-neutral LLM client trait, request and response types, and error
//! taxonomy.
//!
//! [`LlmClient`] has one async method, [`complete`](LlmClient::complete):
//! it takes a fully rendered prompt plus an optional JSON Schema 2020-12
//! output schema in a [`CompletionRequest`] and returns a
//! [`CompletionResponse`] holding the parsed JSON. The crate ships no
//! provider SDK and, without the `http` feature, no HTTP client.
//!
//! Depend on this crate to write code that is generic over providers, or to
//! implement an adapter. To call a model, add an adapter such as
//! [`turul-llm-ollama`](https://docs.rs/turul-llm-ollama) or
//! [`turul-llm-openai`](https://docs.rs/turul-llm-openai).
//!
//! The caller renders the prompt and validates `parsed_output` against its
//! own schema; the trait does neither. Adapters pass the schema through to
//! the provider's structured-output API and must not loosen it.
//! `LlmClient` is object-safe, so `Arc<dyn LlmClient>` can switch providers
//! at runtime.
//!
//! # Example
//!
//! Code written against the trait works with any adapter:
//!
//! ```toml
//! [dependencies]
//! turul-llm-core = "0.1"
//! serde_json = "1"
//! ```
//!
//! ```rust
//! use serde_json::{Value, json};
//! use turul_llm_core::{CompletionRequest, ExecutionHints, LlmClient, LlmError};
//!
//! async fn greeting(client: &dyn LlmClient, name: &str) -> Result<Value, LlmError> {
//!     let schema = json!({
//!         "type": "object",
//!         "properties": { "greeting": { "type": "string" } },
//!         "required": ["greeting"]
//!     });
//!     let request = CompletionRequest::new(format!("Greet {name} as JSON."))
//!         .with_output_schema(schema)
//!         .with_execution_hints(ExecutionHints::new().with_temperature(0.0));
//!     Ok(client.complete(request).await?.parsed_output)
//! }
//! ```
//!
//! # Features
//!
//! | Feature | Default | Enables |
//! |---|---|---|
//! | `http` | no | `http::send_chat`, the shared send path the adapters use; pulls in `reqwest` |
//!
//! [`ChatResponseShape`] (chat-style response decoding) is always available.
//! [`GuardedClient`] wraps any client to refuse over-long prompts and
//! incomplete generations.
//!
//! # Where next
//!
//! - [`turul-llm-ollama`](https://docs.rs/turul-llm-ollama): Ollama adapter.
//! - [`turul-llm-openai`](https://docs.rs/turul-llm-openai): OpenAI-compatible adapter.
//! - [`greet-ollama`](https://github.com/aussierobots/turul-llm/tree/main/examples/greet-ollama):
//!   runnable end-to-end example.

pub mod client;
pub mod decode;
pub mod error;
pub mod guard;
#[cfg(feature = "http")]
pub mod http;
pub mod request;
pub mod response;

pub use client::LlmClient;
pub use decode::ChatResponseShape;
pub use error::LlmError;
pub use guard::{GuardedClient, PromptCountPolicy, TokenCounter};
pub use request::{CompletionRequest, ContextBudget, ExecutionHints};
pub use response::{CompletionResponse, StopReason, Usage};

//! Provider-neutral LLM client trait + request/response types + error
//! taxonomy.
//!
//! The crate intentionally carries no transport, no HTTP client, and no
//! provider SDKs. Concrete adapters live in sibling crates
//! (`turul-llm-ollama`, `turul-llm-openai`, etc.) so the trait stays
//! cheap to depend on for callers who do not need a specific provider.
//!
//! # The trait
//!
//! [`LlmClient`] exposes a single async operation: take a fully-rendered
//! prompt plus an optional JSON Schema 2020-12 document describing the
//! expected output shape, return a parsed JSON value that the caller can
//! validate / consume.
//!
//! Callers are expected to:
//!
//! - Render the prompt themselves (e.g. via a manifest template engine).
//!   The trait does not own the template surface — different callers
//!   render prompts differently.
//! - Supply the output schema when they need structured output. Adapters
//!   pass it through to the provider's structured-output API and MUST
//!   NOT loosen what the schema declares.
//! - Validate the returned `parsed_output` against their schema. The
//!   trait deliberately stops short of running validation itself so that
//!   adopters can pick their preferred JSON Schema validator.
//!
//! # Object safety
//!
//! `LlmClient` is object-safe: callers that want to swap providers at
//! runtime hold `Arc<dyn LlmClient>` and route based on configuration.
//! The compile-time assertion lives in this crate's tests.

pub mod client;
pub mod error;
pub mod request;
pub mod response;

pub use client::LlmClient;
pub use error::LlmError;
pub use request::{CompletionRequest, ExecutionHints};
pub use response::CompletionResponse;

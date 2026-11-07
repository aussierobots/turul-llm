//! Error taxonomy for LLM calls.
//!
//! Variants are intentionally coarse — adapters carry transport-specific
//! detail in the inner `String`, and the enum is `#[non_exhaustive]` so
//! future failure modes (rate limiting, content filter rejections,
//! context-window overflow) can be added without breaking callers that
//! match exhaustively today.

use thiserror::Error;

/// Errors returned by any [`LlmClient`](crate::client::LlmClient)
/// implementation.
///
/// Adapters should map provider-specific failures onto the closest
/// variant. The inner `String` payload is for human-readable context and
/// is not part of any machine-readable contract.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum LlmError {
    /// Network, TLS, DNS, or other transport-level failure reaching the
    /// provider. Wraps a human-readable description of the underlying
    /// error.
    #[error("LLM transport error: {0}")]
    Transport(String),

    /// The provider responded but the response was not usable: a 4xx /
    /// 5xx status, a parse failure on the provider envelope, or a
    /// content body that violates the provider's own response contract.
    #[error("LLM provider error: {0}")]
    Provider(String),

    /// The model returned content that did not satisfy the caller's
    /// [`output_schema`](crate::request::CompletionRequest::output_schema).
    /// Adapters surface this when the structured-output payload
    /// straight-up failed to deserialize as JSON or when the provider
    /// signalled a schema-validation failure on its end. Callers that
    /// run their own JSON Schema validator after [`complete`](crate::client::LlmClient::complete)
    /// returns may also construct this variant when validation fails.
    #[error("LLM response did not satisfy output schema: {0}")]
    SchemaViolation(String),

    /// The call exceeded an adapter-configured timeout. Adapters may
    /// surface this when an underlying transport raises a timeout-shaped
    /// error.
    #[error("LLM call timed out")]
    Timeout,

    /// Anything that does not fit the other variants. Adapters should
    /// prefer the more specific variants when applicable.
    #[error("LLM error: {0}")]
    Other(String),

    /// The request was refused before any call because it is not valid
    /// for the client handling it (for example a guarded client given a
    /// request without a budget).
    #[error("invalid LLM request: {reason}")]
    InvalidRequest {
        /// What is wrong with the request.
        reason: String,
    },

    /// The prompt plus the output cap does not fit the context window.
    /// Nothing was sent.
    #[error(
        "prompt ({prompt_tokens}) + max output ({max_output_tokens}) tokens exceed the context window ({context_tokens})"
    )]
    ContextOverflow {
        /// Tokens the caller's counter reported for the prompt.
        prompt_tokens: u32,
        /// The requested output cap.
        max_output_tokens: u32,
        /// The context window the request was sized against.
        context_tokens: u32,
    },

    /// Generation did not end on its own: the provider reported a stop
    /// reason other than `stop`, or none.
    #[error("generation did not complete (stop reason: {})", stop_reason.as_deref().unwrap_or("missing"))]
    IncompleteGeneration {
        /// The provider's reason, verbatim; `None` when it reported none.
        stop_reason: Option<String>,
    },

    /// The provider reported no prompt token count where one was
    /// required.
    #[error("provider reported no prompt token count")]
    UsageMissing,

    /// The provider's prompt token count differs from the caller's own
    /// count.
    #[error("provider counted {reported} prompt tokens, local count is {local}")]
    UsageMismatch {
        /// The caller's count.
        local: u32,
        /// The provider's count.
        reported: u32,
    },

    /// The model being served is not the one the client is pinned to.
    #[error("served model identity {actual} does not match pinned {expected}")]
    ModelMismatch {
        /// The pinned identity.
        expected: String,
        /// The identity the server reported.
        actual: String,
    },
}

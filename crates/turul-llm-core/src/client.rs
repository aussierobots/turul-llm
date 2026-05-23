//! The provider-neutral LLM client trait.

use async_trait::async_trait;

use crate::error::LlmError;
use crate::request::CompletionRequest;
use crate::response::CompletionResponse;

/// A provider-neutral interface for a single non-streaming LLM call.
///
/// Implementations live in adapter crates (`turul-llm-ollama`,
/// `turul-llm-openai`, ...). Callers can hold either a concrete
/// adapter or `Arc<dyn LlmClient>` to swap providers at runtime.
///
/// The trait is object-safe and `Send + Sync` so handlers that cross
/// `tokio::spawn` boundaries can carry a `&dyn LlmClient` reference.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a rendered prompt + optional output schema to the provider
    /// and return the parsed structured output on success.
    ///
    /// Adapters are expected to:
    ///
    /// - Forward
    ///   [`CompletionRequest::output_schema`](crate::request::CompletionRequest::output_schema)
    ///   to the provider's structured-output API when present.
    /// - Map transport / protocol / parsing failures onto the closest
    ///   [`LlmError`] variant.
    /// - NOT loosen what the caller's schema declares — if the
    ///   provider's structured-output API accepts only a subset of
    ///   JSON Schema, the adapter MUST surface a
    ///   [`LlmError::SchemaViolation`] rather than silently weakening
    ///   the constraint.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Compile-time assertion that the trait is object-safe.
    const _: fn() = || {
        fn assert_object_safe<T: ?Sized + LlmClient>() {}
        assert_object_safe::<dyn LlmClient>();
    };

    // Compile-time assertion that the trait is Send + Sync, matching
    // the bound on the trait itself. If the bound is ever relaxed this
    // fails to compile.
    const _: fn() = || {
        fn assert_send_sync<T: ?Sized + Send + Sync>() {}
        assert_send_sync::<dyn LlmClient>();
    };

    struct StubClient;

    #[async_trait]
    impl LlmClient for StubClient {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            // Echo back a deterministic structured payload so we can
            // confirm the trait wires together without a real provider.
            Ok(CompletionResponse::new(json!({
                "echoed_prompt": request.rendered_prompt,
            })))
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn trait_wires_through_dyn_dispatch() {
        let client: Box<dyn LlmClient> = Box::new(StubClient);
        let request = CompletionRequest::new("hello world");
        let response = client.complete(request).await.expect("stub never fails");
        assert_eq!(response.parsed_output["echoed_prompt"], "hello world");
    }

    #[test]
    fn error_variants_are_distinguishable() {
        // Coarse but real: ensure every variant's Display string is
        // non-empty so callers can log it.
        let cases = [
            LlmError::Transport("connect refused".into()),
            LlmError::Provider("HTTP 500".into()),
            LlmError::SchemaViolation("missing field `greeting`".into()),
            LlmError::Timeout,
            LlmError::Other("unknown".into()),
        ];
        for case in &cases {
            assert!(
                !format!("{case}").is_empty(),
                "Display impl returned empty string"
            );
        }
    }

    #[test]
    fn execution_hints_default_is_empty() {
        let hints = crate::request::ExecutionHints::default();
        assert!(hints.is_empty());
    }

    #[test]
    fn completion_request_builder_chains() {
        let request = CompletionRequest::new("p")
            .with_output_schema(json!({"type": "object"}))
            .with_execution_hints(
                crate::request::ExecutionHints::new()
                    .with_max_tokens(64)
                    .with_temperature(0.2),
            );
        assert_eq!(request.rendered_prompt, "p");
        assert!(request.output_schema.is_some());
        assert_eq!(request.execution_hints.max_tokens, Some(64));
    }
}

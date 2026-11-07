//! Opt-in strict generation: refuse requests that cannot fit, and refuse
//! responses that did not complete.
//!
//! [`GuardedClient`] wraps any [`LlmClient`]. It never lets a request
//! through that the caller's own [`TokenCounter`] says will overflow the
//! context window, and it never returns a response whose stop reason is
//! not a clean stop. Providers differ in how they treat an over-long
//! prompt (some truncate it silently), so refusing before sending is the
//! only check that holds everywhere.

use std::sync::Arc;

use async_trait::async_trait;

use crate::client::LlmClient;
use crate::error::LlmError;
use crate::request::CompletionRequest;
use crate::response::{CompletionResponse, StopReason};

/// Counts the prompt tokens a provider will evaluate for a request.
///
/// The count must be of what the provider actually tokenises, which
/// includes any chat template and default system text it applies, so the
/// implementation receives the whole request and is responsible for
/// modelling the served model's template and tokenizer. This crate ships
/// no tokenizer.
#[async_trait]
pub trait TokenCounter: Send + Sync {
    /// The number of prompt tokens the provider will evaluate for
    /// `request`.
    async fn count(&self, request: &CompletionRequest) -> Result<u32, LlmError>;
}

/// Whether the provider's reported prompt token count must equal the
/// local count. There is deliberately no default: the caller states
/// whether its counter models the served template exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PromptCountPolicy {
    /// The provider must report a prompt count and it must equal the
    /// local count.
    Exact,
    /// The provider's prompt count is not checked.
    Ignore,
}

/// An [`LlmClient`] that enforces explicit sizing and complete
/// generations around an inner client.
///
/// Requests must carry a [`ContextBudget`](crate::request::ContextBudget).
/// The window is read only from the budget; the guard never queries the
/// provider. Errors from the inner client pass through unchanged, so a
/// structured-output call whose text was cut off surfaces as the
/// adapter's `SchemaViolation` (it fails to parse before the stop reason
/// is checked), not as `IncompleteGeneration`; both are errors.
#[derive(Clone)]
pub struct GuardedClient {
    inner: Arc<dyn LlmClient>,
    counter: Arc<dyn TokenCounter>,
    policy: PromptCountPolicy,
}

impl GuardedClient {
    /// Wrap `inner`, counting prompts with `counter` and applying
    /// `policy` to the provider's reported prompt count.
    pub fn new(
        inner: Arc<dyn LlmClient>,
        counter: Arc<dyn TokenCounter>,
        policy: PromptCountPolicy,
    ) -> Self {
        Self {
            inner,
            counter,
            policy,
        }
    }
}

#[async_trait]
impl LlmClient for GuardedClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let budget = request.budget.ok_or_else(|| LlmError::InvalidRequest {
            reason: "a guarded client requires a ContextBudget on every request".into(),
        })?;

        let prompt_tokens = self.counter.count(&request).await?;
        let max_output_tokens = budget.max_output_tokens.get();
        let context_tokens = budget.context_tokens.get();
        if u64::from(prompt_tokens) + u64::from(max_output_tokens) > u64::from(context_tokens) {
            return Err(LlmError::ContextOverflow {
                prompt_tokens,
                max_output_tokens,
                context_tokens,
            });
        }

        let response = self.inner.complete(request).await?;

        if response.stop_reason != Some(StopReason::Stop) {
            return Err(LlmError::IncompleteGeneration {
                stop_reason: response
                    .stop_reason
                    .as_ref()
                    .map(|r| r.as_str().to_string()),
            });
        }

        if self.policy == PromptCountPolicy::Exact {
            match response.usage.and_then(|u| u.prompt_tokens) {
                None => return Err(LlmError::UsageMissing),
                Some(reported) if reported != prompt_tokens => {
                    return Err(LlmError::UsageMismatch {
                        local: prompt_tokens,
                        reported,
                    });
                }
                Some(_) => {}
            }
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::request::ContextBudget;
    use crate::response::Usage;
    use serde_json::Value;

    struct FixedCounter(u32);

    #[async_trait]
    impl TokenCounter for FixedCounter {
        async fn count(&self, _request: &CompletionRequest) -> Result<u32, LlmError> {
            Ok(self.0)
        }
    }

    struct Inner {
        calls: AtomicUsize,
        reply: CompletionResponse,
    }

    #[async_trait]
    impl LlmClient for Inner {
        async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.reply.clone())
        }
    }

    fn budget(context: u32, out: u32) -> ContextBudget {
        ContextBudget::new(
            NonZeroU32::new(context).unwrap(),
            NonZeroU32::new(out).unwrap(),
        )
    }

    fn reply(stop: Option<StopReason>, prompt_tokens: Option<u32>) -> CompletionResponse {
        let mut r = CompletionResponse::new(Value::Null);
        if let Some(s) = stop {
            r = r.with_stop_reason(s);
        }
        if prompt_tokens.is_some() {
            r = r.with_usage(Usage::new(prompt_tokens, Some(1)));
        }
        r
    }

    fn guarded(
        reply: CompletionResponse,
        counted: u32,
        policy: PromptCountPolicy,
    ) -> (GuardedClient, Arc<Inner>) {
        let inner = Arc::new(Inner {
            calls: AtomicUsize::new(0),
            reply,
        });
        let g = GuardedClient::new(inner.clone(), Arc::new(FixedCounter(counted)), policy);
        (g, inner)
    }

    fn req(context: u32, out: u32) -> CompletionRequest {
        CompletionRequest::new("hi").with_budget(budget(context, out))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_without_budget_is_invalid_and_not_sent() {
        let (g, inner) = guarded(
            reply(Some(StopReason::Stop), None),
            1,
            PromptCountPolicy::Ignore,
        );
        let err = g
            .complete(CompletionRequest::new("hi"))
            .await
            .expect_err("must fail");
        assert!(matches!(err, LlmError::InvalidRequest { .. }), "{err:?}");
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn overflow_is_refused_before_sending() {
        let (g, inner) = guarded(
            reply(Some(StopReason::Stop), None),
            90,
            PromptCountPolicy::Ignore,
        );
        let err = g.complete(req(100, 11)).await.expect_err("must fail");
        assert!(
            matches!(
                err,
                LlmError::ContextOverflow {
                    prompt_tokens: 90,
                    max_output_tokens: 11,
                    context_tokens: 100,
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn exact_fit_is_allowed() {
        let (g, inner) = guarded(
            reply(Some(StopReason::Stop), None),
            90,
            PromptCountPolicy::Ignore,
        );
        g.complete(req(100, 10)).await.expect("ok");
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn length_other_and_missing_stop_reasons_are_incomplete() {
        for stop in [
            Some(StopReason::Length),
            Some(StopReason::Other("content_filter".into())),
            None,
        ] {
            let (g, _) = guarded(reply(stop.clone(), None), 1, PromptCountPolicy::Ignore);
            let err = g.complete(req(100, 10)).await.expect_err("must fail");
            assert!(
                matches!(&err, LlmError::IncompleteGeneration { stop_reason }
                    if *stop_reason == stop.as_ref().map(|s| s.as_str().to_string())),
                "{err:?}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn exact_policy_requires_matching_prompt_count() {
        let (g, _) = guarded(
            reply(Some(StopReason::Stop), Some(5)),
            5,
            PromptCountPolicy::Exact,
        );
        g.complete(req(100, 10))
            .await
            .expect("matching count passes");

        let (g, _) = guarded(
            reply(Some(StopReason::Stop), Some(6)),
            5,
            PromptCountPolicy::Exact,
        );
        let err = g.complete(req(100, 10)).await.expect_err("must fail");
        assert!(
            matches!(
                err,
                LlmError::UsageMismatch {
                    local: 5,
                    reported: 6,
                    ..
                }
            ),
            "{err:?}"
        );

        let (g, _) = guarded(
            reply(Some(StopReason::Stop), None),
            5,
            PromptCountPolicy::Exact,
        );
        let err = g.complete(req(100, 10)).await.expect_err("must fail");
        assert!(matches!(err, LlmError::UsageMissing), "{err:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ignore_policy_skips_the_prompt_count_check() {
        let (g, _) = guarded(
            reply(Some(StopReason::Stop), Some(99)),
            5,
            PromptCountPolicy::Ignore,
        );
        g.complete(req(100, 10)).await.expect("ok");
    }
}

//! Request types passed to [`LlmClient::complete`](crate::client::LlmClient::complete).

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single LLM completion request.
///
/// The caller is responsible for rendering the prompt — the trait does
/// not own the template surface. `output_schema` is optional: leaving it
/// `None` means "free-form text" and the adapter will treat the response
/// as a raw string wrapped into [`crate::response::CompletionResponse::parsed_output`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CompletionRequest {
    /// Fully-rendered prompt string the model should consume as user
    /// input. The trait deliberately carries a single string here
    /// rather than a system/user split: providers that distinguish
    /// roles can split the rendered prompt at a marker. If a future
    /// provider needs role-tagged or multi-part input, the trait must
    /// grow a structured input type rather than overloading this
    /// field — `#[non_exhaustive]` on this struct keeps that path open.
    pub rendered_prompt: String,

    /// Optional JSON Schema 2020-12 document describing the expected
    /// output shape. When present, adapters pass it through to the
    /// provider's structured-output API (Ollama `format`, OpenAI
    /// `response_format.json_schema.schema`, etc.). When absent the
    /// model is unconstrained and the adapter wraps any returned text
    /// into [`crate::response::CompletionResponse::parsed_output`] as a JSON string.
    pub output_schema: Option<Value>,

    /// Generation hints. All fields are optional; adapters apply each
    /// hint only if the provider's API supports it.
    pub execution_hints: ExecutionHints,

    /// Optional explicit sizing. When present it takes precedence over
    /// [`ExecutionHints::max_tokens`] and, for providers with a
    /// per-request context window, sets that window.
    pub budget: Option<ContextBudget>,
}

/// Explicit per-request sizing: the context window the provider will
/// give the request and the cap on generated tokens.
///
/// Both values are mandatory by construction and cannot be zero. Both
/// count thinking tokens, which share the window with the prompt and
/// the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ContextBudget {
    /// Tokens available to the prompt, any thinking and the output
    /// together.
    pub context_tokens: NonZeroU32,
    /// Upper bound on generated tokens.
    pub max_output_tokens: NonZeroU32,
}

impl ContextBudget {
    /// Build a budget from a context window and an output cap.
    pub const fn new(context_tokens: NonZeroU32, max_output_tokens: NonZeroU32) -> Self {
        Self {
            context_tokens,
            max_output_tokens,
        }
    }
}

impl CompletionRequest {
    /// Build a request with just a rendered prompt; no schema, default
    /// hints. Use this for free-form text completion.
    pub fn new(rendered_prompt: impl Into<String>) -> Self {
        Self {
            rendered_prompt: rendered_prompt.into(),
            output_schema: None,
            execution_hints: ExecutionHints::default(),
            budget: None,
        }
    }

    /// Builder-style setter for [`output_schema`](Self::output_schema).
    pub fn with_output_schema(mut self, schema: Value) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Builder-style setter for [`execution_hints`](Self::execution_hints).
    pub fn with_execution_hints(mut self, hints: ExecutionHints) -> Self {
        self.execution_hints = hints;
        self
    }

    /// Builder-style setter for [`budget`](Self::budget).
    pub fn with_budget(mut self, budget: ContextBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// The effective cap on generated tokens: the budget's
    /// `max_output_tokens` if a budget is set, otherwise
    /// [`ExecutionHints::max_tokens`]. Adapters use this instead of
    /// reading either field directly.
    pub fn max_output_tokens(&self) -> Option<u32> {
        self.budget
            .map(|b| b.max_output_tokens.get())
            .or(self.execution_hints.max_tokens)
    }
}

/// Optional generation hints. Adapters apply each field only if the
/// provider's API supports it; unsupported fields are silently ignored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExecutionHints {
    /// Upper bound on tokens generated for the completion.
    pub max_tokens: Option<u32>,

    /// Sampling temperature. Conventional range is `0.0..=2.0`.
    pub temperature: Option<f32>,

    /// Nucleus sampling cutoff. Conventional range is `0.0..=1.0`.
    pub top_p: Option<f32>,
}

impl ExecutionHints {
    /// Construct an empty hints block — equivalent to
    /// [`ExecutionHints::default`] and the entry point for the builder
    /// methods.
    pub fn new() -> Self {
        Self {
            max_tokens: None,
            temperature: None,
            top_p: None,
        }
    }

    /// Builder-style setter for [`max_tokens`](Self::max_tokens).
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Builder-style setter for [`temperature`](Self::temperature).
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Builder-style setter for [`top_p`](Self::top_p).
    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// True iff every field is `None` — useful for adapters that want
    /// to skip emitting the generation-options block entirely.
    pub fn is_empty(&self) -> bool {
        self.max_tokens.is_none() && self.temperature.is_none() && self.top_p.is_none()
    }
}

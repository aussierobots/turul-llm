//! Request types passed to [`LlmClient::complete`](crate::client::LlmClient::complete).

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
}

impl CompletionRequest {
    /// Build a request with just a rendered prompt; no schema, default
    /// hints. Use this for free-form text completion.
    pub fn new(rendered_prompt: impl Into<String>) -> Self {
        Self {
            rendered_prompt: rendered_prompt.into(),
            output_schema: None,
            execution_hints: ExecutionHints::default(),
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
    /// [`ExecutionHints::default`] but available in `const` contexts
    /// and as a builder entry point.
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

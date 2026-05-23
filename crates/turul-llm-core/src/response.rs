//! Response type returned by [`LlmClient::complete`](crate::client::LlmClient::complete).

use serde_json::Value;

/// A single LLM completion response.
///
/// The structured payload is a [`serde_json::Value`] so callers can run
/// it through their own JSON Schema validator (matching whatever schema
/// they passed in the request). Adapters that called a free-form
/// completion endpoint wrap the returned text into
/// [`Value::String`](serde_json::Value::String) so the field is always
/// populated.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CompletionResponse {
    /// The parsed JSON value returned by the provider. When the caller
    /// supplied an [`output_schema`](crate::request::CompletionRequest::output_schema)
    /// this is the structured payload the model produced. When the
    /// schema was absent this is a JSON string carrying the raw text
    /// response.
    pub parsed_output: Value,
}

impl CompletionResponse {
    /// Construct a response from a parsed JSON value.
    pub fn new(parsed_output: Value) -> Self {
        Self { parsed_output }
    }
}

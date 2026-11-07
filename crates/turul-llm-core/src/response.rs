//! Response type returned by [`LlmClient::complete`](crate::client::LlmClient::complete).

use serde_json::Value;

/// A single LLM completion response.
///
/// The structured payload is a [`serde_json::Value`] so callers can run
/// it through their own JSON Schema validator (matching whatever schema
/// they passed in the request). Adapters that called a free-form
/// completion endpoint wrap the returned text into
/// [`Value::String`] so the field is always
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

    /// Token counts reported by the provider; `None` when the provider
    /// reported none.
    pub usage: Option<Usage>,

    /// Why generation ended, as reported by the provider; `None` when the
    /// provider reported none.
    pub stop_reason: Option<StopReason>,

    /// Token ids the provider returned for the request, verbatim and
    /// unsliced; `None` when the provider returned none. What the ids
    /// cover (prompt, generated text, special tokens) and where the
    /// prompt ends are provider- and model-specific, so interpreting
    /// them is the caller's job.
    pub token_ids: Option<Vec<u32>>,
}

impl CompletionResponse {
    /// Construct a response from a parsed JSON value, with no usage and
    /// no stop reason.
    pub fn new(parsed_output: Value) -> Self {
        Self {
            parsed_output,
            usage: None,
            stop_reason: None,
            token_ids: None,
        }
    }

    /// Builder-style setter for [`usage`](Self::usage).
    pub fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Builder-style setter for [`token_ids`](Self::token_ids).
    pub fn with_token_ids(mut self, token_ids: Vec<u32>) -> Self {
        self.token_ids = Some(token_ids);
        self
    }

    /// Builder-style setter for [`stop_reason`](Self::stop_reason).
    pub fn with_stop_reason(mut self, stop_reason: StopReason) -> Self {
        self.stop_reason = Some(stop_reason);
        self
    }
}

/// Token counts as reported by the provider. Each count is `None` when
/// the provider did not report it; nothing is estimated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Usage {
    /// Tokens the provider evaluated for the prompt (including any
    /// template or system text it added).
    pub prompt_tokens: Option<u32>,
    /// Tokens the provider generated (including thinking tokens).
    pub output_tokens: Option<u32>,
}

impl Usage {
    /// Build a usage record from the two counts.
    pub fn new(prompt_tokens: Option<u32>, output_tokens: Option<u32>) -> Self {
        Self {
            prompt_tokens,
            output_tokens,
        }
    }
}

/// Why generation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopReason {
    /// The model finished on its own (provider reason `stop`).
    Stop,
    /// Generation hit the output cap or filled the context window
    /// (provider reason `length`); the output may be cut off.
    Length,
    /// Any other provider-specific reason, verbatim.
    Other(String),
}

impl StopReason {
    /// Map a provider's reason string: `stop` and `length` are the shared
    /// vocabulary; anything else is kept verbatim.
    pub fn from_provider(reason: &str) -> Self {
        match reason {
            "stop" => Self::Stop,
            "length" => Self::Length,
            other => Self::Other(other.to_string()),
        }
    }

    /// The provider-style name of this reason.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::Other(s) => s,
        }
    }
}

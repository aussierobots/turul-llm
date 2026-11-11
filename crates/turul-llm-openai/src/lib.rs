//! OpenAI-compatible adapter for [`turul_llm_core::LlmClient`].
//!
//! [`OpenAiClient`] calls the chat completions API at
//! `{base_url}/chat/completions` with a Bearer token. Structured output uses
//! `response_format = { type: "json_schema", ... }` with `strict: true`, and
//! content that still fails to parse as JSON surfaces as
//! [`LlmError::SchemaViolation`]. The base URL defaults to
//! `https://api.openai.com/v1` and can point at any OpenAI-compatible gateway
//! via [`OpenAiClient::with_base_url`].
//!
//! Use this crate for OpenAI or an OpenAI-compatible server. The trait,
//! request and response types live in [`turul_llm_core`]; swap in
//! [`turul-llm-ollama`](https://docs.rs/turul-llm-ollama) without touching
//! calling code.
//!
//! # Example
//!
//! ```toml
//! [dependencies]
//! turul-llm-core = "0.1"
//! turul-llm-openai = "0.1"
//! tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
//! serde_json = "1"
//! ```
//!
//! ```rust,no_run
//! use serde_json::json;
//! use turul_llm_core::{CompletionRequest, LlmClient};
//! use turul_llm_openai::OpenAiClient;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let api_key = std::env::var("OPENAI_API_KEY")?;
//! let client = OpenAiClient::new(api_key, "gpt-4o-mini");
//! let request = CompletionRequest::new("Greet Ada as JSON.").with_output_schema(json!({
//!     "type": "object",
//!     "properties": { "greeting": { "type": "string" } },
//!     "required": ["greeting"]
//! }));
//! let response = client.complete(request).await?;
//! println!("{}", response.parsed_output);
//! # Ok(())
//! # }
//! ```
//!
//! # Where next
//!
//! - [`OpenAiClient::with_schema_name`] sets the `json_schema.name` sent to the provider.
//! - [`turul_llm_core::GuardedClient`] refuses over-long prompts and
//!   incomplete generations.
//! - [`greet-ollama`](https://github.com/aussierobots/turul-llm/tree/main/examples/greet-ollama):
//!   the same call shape end to end.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use turul_llm_core::{
    ChatResponseShape, CompletionRequest, CompletionResponse, LlmClient, LlmError, http::send_chat,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_SCHEMA_NAME: &str = "structured_output";

/// Client targeting an OpenAI-compatible `/chat/completions` endpoint.
#[derive(Debug, Clone)]
pub struct OpenAiClient {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::Client,
    schema_name: String,
}

impl OpenAiClient {
    /// Build a client with the default OpenAI base URL.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_base_url(DEFAULT_BASE_URL, api_key, model)
    }

    /// Build a client pointed at a specific OpenAI-compatible base URL
    /// (e.g. a local proxy or a non-OpenAI gateway). The base URL is
    /// normalised by trimming any trailing slash.
    pub fn with_base_url(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self::with_http_client(base_url, api_key, model, reqwest::Client::new())
    }

    /// Build a client with a pre-configured [`reqwest::Client`].
    pub fn with_http_client(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        let base = base_url.into();
        Self {
            base_url: base.trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            http,
            schema_name: DEFAULT_SCHEMA_NAME.to_string(),
        }
    }

    /// Override the `json_schema.name` field sent in the request body.
    /// OpenAI requires a non-empty identifier; some gateways are
    /// stricter than others about what they accept.
    pub fn with_schema_name(mut self, name: impl Into<String>) -> Self {
        self.schema_name = name.into();
        self
    }

    /// The model identifier this client is bound to.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The base URL this client is bound to (without trailing slash).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponseFormat<'a> {
    JsonSchema { json_schema: JsonSchemaBlock<'a> },
}

#[derive(Serialize)]
struct JsonSchemaBlock<'a> {
    name: &'a str,
    schema: &'a Value,
    strict: bool,
}

fn apply_hints(req: &mut ChatRequest<'_>, request: &CompletionRequest) {
    req.max_tokens = request.max_output_tokens();
    req.temperature = request.execution_hints.temperature;
    req.top_p = request.execution_hints.top_p;
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let response_format =
            request
                .output_schema
                .as_ref()
                .map(|schema| ResponseFormat::JsonSchema {
                    json_schema: JsonSchemaBlock {
                        name: &self.schema_name,
                        schema,
                        strict: true,
                    },
                });

        let mut body = ChatRequest {
            model: &self.model,
            messages: vec![ChatMessage {
                role: "user",
                content: &request.rendered_prompt,
            }],
            response_format,
            max_tokens: None,
            temperature: None,
            top_p: None,
        };
        apply_hints(&mut body, &request);

        let url = self.endpoint();
        send_chat(
            self.http.post(&url).bearer_auth(&self.api_key).json(&body),
            &url,
            &ChatResponseShape::new("/choices/0/message/content", "/choices/0/finish_reason")
                .with_usage_pointers("/usage/prompt_tokens", "/usage/completion_tokens"),
            request.output_schema.is_some(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turul_llm_core::ExecutionHints;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn chat_envelope(content: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": "stop"
            }]
        }))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn structured_call_emits_response_format_json_schema() {
        let server = MockServer::start().await;
        let schema = json!({
            "type": "object",
            "properties": {"greeting": {"type": "string"}},
            "required": ["greeting"]
        });
        let expected_payload = json!({"greeting": "Hi, Ada!"});
        let content = serde_json::to_string(&expected_payload).unwrap();

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer test-key"))
            .and(body_partial_json(json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Greet Ada warmly."}],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "structured_output",
                        "schema": schema.clone(),
                        "strict": true
                    }
                },
                "max_tokens": 64,
                "temperature": 0.2
            })))
            .respond_with(chat_envelope(&content))
            .mount(&server)
            .await;

        let client = OpenAiClient::with_base_url(server.uri(), "test-key", "gpt-4o-mini");
        let request = CompletionRequest::new("Greet Ada warmly.")
            .with_output_schema(schema)
            .with_execution_hints(
                ExecutionHints::new()
                    .with_max_tokens(64)
                    .with_temperature(0.2),
            );
        let response = client.complete(request).await.expect("ok");
        assert_eq!(response.parsed_output, expected_payload);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn freeform_response_is_wrapped_as_json_string() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(chat_envelope("Hello there."))
            .mount(&server)
            .await;

        let client = OpenAiClient::with_base_url(server.uri(), "k", "m");
        let response = client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect("ok");
        assert_eq!(response.parsed_output, Value::String("Hello there.".into()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_error_maps_to_provider_variant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = OpenAiClient::with_base_url(server.uri(), "k", "m");
        let err = client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect_err("must fail");
        assert!(matches!(err, LlmError::Provider(_)), "got {err:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_structured_payload_maps_to_schema_violation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(chat_envelope("not json"))
            .mount(&server)
            .await;

        let client = OpenAiClient::with_base_url(server.uri(), "k", "m");
        let err = client
            .complete(CompletionRequest::new("hi").with_output_schema(json!({"type": "object"})))
            .await
            .expect_err("must fail");
        assert!(matches!(err, LlmError::SchemaViolation(_)), "got {err:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn budget_output_cap_overrides_the_max_tokens_hint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({"max_tokens": 512})))
            .respond_with(chat_envelope("ok"))
            .expect(1)
            .mount(&server)
            .await;

        let client = OpenAiClient::with_base_url(server.uri(), "k", "m");
        let request = CompletionRequest::new("hi")
            .with_execution_hints(ExecutionHints::new().with_max_tokens(7))
            .with_budget(turul_llm_core::ContextBudget::new(
                std::num::NonZeroU32::new(8192).unwrap(),
                std::num::NonZeroU32::new(512).unwrap(),
            ));
        client.complete(request).await.expect("ok");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn usage_and_stop_reason_are_decoded() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "length"
                }],
                "usage": {"prompt_tokens": 9, "completion_tokens": 5, "total_tokens": 14}
            })))
            .mount(&server)
            .await;

        let client = OpenAiClient::with_base_url(server.uri(), "k", "m");
        let out = client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect("ok");
        assert_eq!(out.stop_reason, Some(turul_llm_core::StopReason::Length));
        assert_eq!(
            out.usage,
            Some(turul_llm_core::Usage::new(Some(9), Some(5)))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn truncated_structured_payload_reports_finish_reason() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "{\"a\": \"cut"},
                    "finish_reason": "length"
                }]
            })))
            .mount(&server)
            .await;

        let client = OpenAiClient::with_base_url(server.uri(), "k", "m");
        let err = client
            .complete(CompletionRequest::new("hi").with_output_schema(json!({"type": "object"})))
            .await
            .expect_err("must fail");
        match err {
            LlmError::SchemaViolation(m) => assert!(m.contains("finish_reason=length"), "{m}"),
            other => panic!("got {other:?}"),
        }
    }
}

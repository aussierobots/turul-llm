//! OpenAI-compatible adapter for [`turul_llm_core::LlmClient`].
//!
//! Targets the chat completions API at `{base_url}/chat/completions`
//! with a Bearer-token `Authorization` header. The `base_url` is
//! configurable so this adapter also works against OpenAI-compatible
//! gateways (Together.ai, vLLM, Groq, etc.); the default points at
//! `https://api.openai.com/v1`.
//!
//! Structured output uses `response_format = { type: "json_schema",
//! json_schema: { name, schema, strict } }` — the official structured
//! output path. The adapter sets `strict: true` so the model is held to
//! the schema and surfaces a [`LlmError::SchemaViolation`] when the
//! returned content still fails to parse as JSON.
//!
//! Tests are `wiremock`-backed; live calls would require
//! `OPENAI_API_KEY` and are out of scope for the default workflow.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use turul_llm_core::{CompletionRequest, CompletionResponse, ExecutionHints, LlmClient, LlmError};

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

fn apply_hints(req: &mut ChatRequest<'_>, hints: &ExecutionHints) {
    req.max_tokens = hints.max_tokens;
    req.temperature = hints.temperature;
    req.top_p = hints.top_p;
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
        apply_hints(&mut body, &request.execution_hints);

        let url = self.endpoint();
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LlmError::Timeout
                } else {
                    LlmError::Transport(format!("POST {url} failed: {e}"))
                }
            })?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| LlmError::Transport(format!("read body from {url} failed: {e}")))?;

        if !status.is_success() {
            return Err(LlmError::Provider(format!(
                "{url} returned HTTP {status}: {text}"
            )));
        }

        let envelope: Value = serde_json::from_str(&text)
            .map_err(|e| LlmError::Provider(format!("envelope parse failed: {e}: {text}")))?;
        let content = envelope
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                LlmError::Provider(format!(
                    "response missing /choices/0/message/content: {envelope}"
                ))
            })?;

        let parsed = if request.output_schema.is_some() {
            serde_json::from_str::<Value>(content).map_err(|e| {
                LlmError::SchemaViolation(format!(
                    "structured-output payload was not valid JSON: {e}: {content}"
                ))
            })?
        } else {
            Value::String(content.to_string())
        };

        Ok(CompletionResponse::new(parsed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
}

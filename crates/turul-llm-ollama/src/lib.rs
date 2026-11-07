//! Ollama adapter for [`turul_llm_core::LlmClient`].
//!
//! Wraps Ollama's `/api/chat` endpoint. The caller supplies a rendered
//! prompt + optional JSON Schema; this crate forwards the schema as
//! Ollama's `format` field (structured output) and parses the
//! `message.content` string as JSON before returning.
//!
//! Live integration is opt-in via env vars (see `examples/greet-ollama`);
//! the crate's own tests use `wiremock` so they remain hermetic.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use turul_llm_core::{
    ChatResponseShape, CompletionRequest, CompletionResponse, ExecutionHints, LlmClient, LlmError,
    http::send_chat,
};

/// Model identifier used by [`OllamaClient::default`] and by callers that
/// need the same fallback.
pub const DEFAULT_MODEL: &str = "llama3.1";

/// Client targeting an Ollama `/api/chat` endpoint.
#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
    num_ctx: Option<u32>,
    think: Option<bool>,
}

impl OllamaClient {
    /// Build a client pointed at `base_url` (e.g. `http://localhost:11434`).
    /// The base URL is normalised by trimming any trailing slash so the
    /// caller can pass either form.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_http_client(base_url, model, reqwest::Client::new())
    }

    /// Build a client with a pre-configured [`reqwest::Client`] — useful
    /// for tests that need a tuned timeout or for sharing a single
    /// connection pool across adapters.
    pub fn with_http_client(
        base_url: impl Into<String>,
        model: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        let base = base_url.into();
        let normalised = base.trim_end_matches('/').to_string();
        Self {
            base_url: normalised,
            model: model.into(),
            http,
            num_ctx: None,
            think: None,
        }
    }

    /// Set the context window (`options.num_ctx`) requested for every call.
    ///
    /// Ollama's default window depends on the host (4096 tokens on
    /// smaller-VRAM machines) and is shared by the prompt, any thinking
    /// tokens and the output; generation stops with `done_reason: "length"`
    /// when it fills, regardless of `max_tokens`. A value that differs from
    /// the window of the already-loaded model makes Ollama reload it.
    pub fn with_num_ctx(mut self, num_ctx: u32) -> Self {
        self.num_ctx = Some(num_ctx);
        self
    }

    /// Enable or disable the model's thinking phase (`think`) for every
    /// call. Thinking tokens share the context window with the prompt and
    /// the output, so turning it off leaves more room for structured
    /// output on models that think by default. Left unset, the model's
    /// own default applies, and models without thinking support reject
    /// an explicit value.
    pub fn with_think(mut self, think: bool) -> Self {
        self.think = Some(think);
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

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url)
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new("http://localhost:11434", DEFAULT_MODEL)
    }
}

/// Request body emitted to `/api/chat`. Kept as a typed struct so the
/// wire shape is reviewable in one place; serialised as the JSON Ollama
/// expects.
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    messages: Vec<ChatMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize, Default)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

impl OllamaOptions {
    fn new(hints: &ExecutionHints, num_ctx: Option<u32>) -> Option<Self> {
        if hints.is_empty() && num_ctx.is_none() {
            return None;
        }
        Some(Self {
            num_ctx,
            num_predict: hints.max_tokens,
            temperature: hints.temperature,
            top_p: hints.top_p,
        })
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let body = ChatRequest {
            model: &self.model,
            stream: false,
            think: self.think,
            messages: vec![ChatMessage {
                role: "user",
                content: &request.rendered_prompt,
            }],
            format: request.output_schema.as_ref(),
            options: OllamaOptions::new(&request.execution_hints, self.num_ctx),
        };

        let url = self.chat_url();
        send_chat(
            self.http.post(&url).json(&body),
            &url,
            &ChatResponseShape::new("/message/content", "/done_reason"),
            request.output_schema.is_some(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn structured_response(payload: &Value) -> ResponseTemplate {
        // Ollama returns the structured payload as a JSON-encoded
        // string inside `message.content`.
        let content = serde_json::to_string(payload).unwrap();
        ResponseTemplate::new(200).set_body_json(json!({
            "model": "test-model",
            "message": {"role": "assistant", "content": content},
            "done": true,
        }))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn structured_call_pins_request_shape_and_decodes_response() {
        // Asserts the request body shape (model, stream=false,
        // single user message with the rendered prompt, format field
        // carrying the schema, generation options derived from hints)
        // and decodes the Ollama-style envelope (message.content as a
        // JSON-encoded string) back to a serde_json::Value.
        let server = MockServer::start().await;
        let schema = json!({
            "type": "object",
            "properties": {"greeting": {"type": "string"}},
            "required": ["greeting"]
        });
        let expected_payload = json!({"greeting": "Hi, Ada!"});

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({
                "model": "test-model",
                "stream": false,
                "format": schema.clone(),
                "messages": [{"role": "user", "content": "Greet Ada warmly."}],
                "options": {"num_predict": 64, "temperature": 0.2},
            })))
            .respond_with(structured_response(&expected_payload))
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model");
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
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "test-model",
                "message": {"role": "assistant", "content": "Hello there."},
                "done": true,
            })))
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model");
        let response = client
            .complete(CompletionRequest::new("Say hi."))
            .await
            .expect("ok");
        assert_eq!(response.parsed_output, Value::String("Hello there.".into()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_error_maps_to_provider_variant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model");
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
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "test-model",
                "message": {"role": "assistant", "content": "not json at all"},
                "done": true,
            })))
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model");
        let err = client
            .complete(CompletionRequest::new("hi").with_output_schema(json!({"type": "object"})))
            .await
            .expect_err("must fail");
        assert!(matches!(err, LlmError::SchemaViolation(_)), "got {err:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn num_ctx_reaches_request_options() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({"options": {"num_ctx": 16384}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role": "assistant", "content": "ok"},
                "done": true,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model").with_num_ctx(16384);
        client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect("ok");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn think_reaches_request_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({"think": false})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role": "assistant", "content": "ok"},
                "done": true,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model").with_think(false);
        client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect("ok");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn think_is_omitted_unless_set() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role": "assistant", "content": "ok"},
                "done": true,
            })))
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model");
        client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect("ok");
        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert!(body.get("think").is_none(), "{body}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn truncated_structured_payload_reports_finish_reason() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role": "assistant", "content": "{\"a\": \"cut"},
                "done": true,
                "done_reason": "length",
            })))
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model");
        let err = client
            .complete(CompletionRequest::new("hi").with_output_schema(json!({"type": "object"})))
            .await
            .expect_err("must fail");
        match err {
            LlmError::SchemaViolation(m) => assert!(m.contains("finish_reason=length"), "{m}"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn base_url_trims_trailing_slash() {
        let c = OllamaClient::new("http://example.invalid:11434/", "m");
        assert_eq!(c.base_url(), "http://example.invalid:11434");
        assert_eq!(c.chat_url(), "http://example.invalid:11434/api/chat");
    }
}

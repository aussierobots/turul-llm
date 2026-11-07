//! Ollama adapter for [`turul_llm_core::LlmClient`].
//!
//! Wraps Ollama's `/api/chat` endpoint (or the templated `/api/generate`
//! endpoint, see [`OllamaClient::with_generate_endpoint`]). The caller supplies a rendered
//! prompt + optional JSON Schema; this crate forwards the schema as
//! Ollama's `format` field (structured output) and parses the
//! `message.content` string as JSON before returning.
//!
//! Live integration is opt-in via env vars (see `examples/greet-ollama`);
//! the crate's own tests use `wiremock` so they remain hermetic.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use turul_llm_core::{
    ChatResponseShape, CompletionRequest, CompletionResponse, LlmClient, LlmError, http::send_chat,
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
    generate: bool,
    model_digest: Option<String>,
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
            generate: false,
            model_digest: None,
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

    /// Use the templated `/api/generate` endpoint instead of `/api/chat`.
    ///
    /// It is the only Ollama endpoint that returns token ids, which then
    /// appear verbatim and unsliced in
    /// [`CompletionResponse::token_ids`](turul_llm_core::CompletionResponse::token_ids).
    /// The prompt is still passed through the model's template; raw mode
    /// is not offered because Ollama returns no token ids for it.
    pub fn with_generate_endpoint(mut self) -> Self {
        self.generate = true;
        self
    }

    /// Pin the client to a model's weights digest (hex, optionally
    /// prefixed `sha256:` or `sha256-`). Before each call the client asks
    /// `/api/show` for the model and returns
    /// [`LlmError::ModelMismatch`] if the weights blob named by the
    /// Modelfile's first `FROM` line has a different digest.
    ///
    /// The digest identifies the weights only, not the template, system
    /// text or parameters, and the model can change between the check and
    /// the call. It is the blob digest, not the manifest digest shown by
    /// `/api/tags`. Models without a local weights blob (for example
    /// remotely hosted ones) have no digest to check, so every call on a
    /// pinned client for such a model fails.
    pub fn with_model_digest(mut self, digest: impl AsRef<str>) -> Self {
        let digest = digest.as_ref().trim().to_ascii_lowercase();
        let digest = digest
            .strip_prefix("sha256:")
            .or_else(|| digest.strip_prefix("sha256-"))
            .unwrap_or(&digest);
        self.model_digest = Some(digest.to_string());
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

    fn endpoint_url(&self) -> String {
        let path = if self.generate { "generate" } else { "chat" };
        format!("{}/api/{path}", self.base_url)
    }

    async fn verify_model_digest(&self, expected: &str) -> Result<(), LlmError> {
        let url = format!("{}/api/show", self.base_url);
        let shown = send_chat(
            self.http.post(&url).json(&json!({"model": self.model})),
            &url,
            &ChatResponseShape::new("/modelfile", "/done_reason"),
            false,
        )
        .await?;
        let modelfile = shown.parsed_output.as_str().unwrap_or_default();
        let actual = weights_digest(modelfile).ok_or_else(|| {
            LlmError::Provider(format!("{url} modelfile names no weights blob digest"))
        })?;
        if actual != expected {
            return Err(LlmError::ModelMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
        Ok(())
    }
}

/// The weights blob digest named by a Modelfile's first non-comment
/// `FROM` line (`.../blobs/sha256-<hex>`); `ADAPTER` and other lines are
/// ignored.
fn weights_digest(modelfile: &str) -> Option<String> {
    let from = modelfile.lines().map(str::trim).find(|line| {
        !line.starts_with('#')
            && line
                .get(..5)
                .is_some_and(|p| p.eq_ignore_ascii_case("FROM "))
    })?;
    let hex: String = from
        .rsplit_once("sha256-")?
        .1
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    (!hex.is_empty()).then(|| hex.to_ascii_lowercase())
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

/// Request body emitted to `/api/generate` (templated, never raw).
#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    prompt: &'a str,
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
    /// A request budget sets both the window and the output cap and takes
    /// precedence over the client's `num_ctx` and the `max_tokens` hint.
    fn new(request: &CompletionRequest, client_num_ctx: Option<u32>) -> Option<Self> {
        let hints = &request.execution_hints;
        let options = Self {
            num_ctx: request
                .budget
                .map(|b| b.context_tokens.get())
                .or(client_num_ctx),
            num_predict: request.max_output_tokens(),
            temperature: hints.temperature,
            top_p: hints.top_p,
        };
        let any = options.num_ctx.is_some()
            || options.num_predict.is_some()
            || options.temperature.is_some()
            || options.top_p.is_some();
        any.then_some(options)
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        if let Some(expected) = &self.model_digest {
            self.verify_model_digest(expected).await?;
        }

        let options = OllamaOptions::new(&request, self.num_ctx);
        let url = self.endpoint_url();
        let structured = request.output_schema.is_some();
        let (builder, shape) = if self.generate {
            let body = GenerateRequest {
                model: &self.model,
                stream: false,
                think: self.think,
                prompt: &request.rendered_prompt,
                format: request.output_schema.as_ref(),
                options,
            };
            (
                self.http.post(&url).json(&body),
                ChatResponseShape::new("/response", "/done_reason")
                    .with_token_ids_pointer("/context"),
            )
        } else {
            let body = ChatRequest {
                model: &self.model,
                stream: false,
                think: self.think,
                messages: vec![ChatMessage {
                    role: "user",
                    content: &request.rendered_prompt,
                }],
                format: request.output_schema.as_ref(),
                options,
            };
            (
                self.http.post(&url).json(&body),
                ChatResponseShape::new("/message/content", "/done_reason"),
            )
        };

        send_chat(
            builder,
            &url,
            &shape.with_usage_pointers("/prompt_eval_count", "/eval_count"),
            structured,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turul_llm_core::ExecutionHints;
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

    fn nz(n: u32) -> std::num::NonZeroU32 {
        std::num::NonZeroU32::new(n).unwrap()
    }

    fn ok_envelope(extra: Value) -> ResponseTemplate {
        let mut body = json!({
            "message": {"role": "assistant", "content": "ok"},
            "done": true,
        });
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        ResponseTemplate::new(200).set_body_json(body)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn budget_sets_window_and_output_cap_and_overrides_client_num_ctx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(
                json!({"options": {"num_ctx": 8192, "num_predict": 512}}),
            ))
            .respond_with(ok_envelope(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model").with_num_ctx(1000);
        let request = CompletionRequest::new("hi")
            .with_execution_hints(ExecutionHints::new().with_max_tokens(7))
            .with_budget(turul_llm_core::ContextBudget::new(nz(8192), nz(512)));
        client.complete(request).await.expect("ok");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn usage_and_stop_reason_are_decoded() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ok_envelope(json!({
                "done_reason": "stop",
                "prompt_eval_count": 27,
                "eval_count": 4
            })))
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model");
        let out = client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect("ok");
        assert_eq!(out.stop_reason, Some(turul_llm_core::StopReason::Stop));
        assert_eq!(
            out.usage,
            Some(turul_llm_core::Usage::new(Some(27), Some(4)))
        );
        assert_eq!(out.token_ids, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generate_endpoint_posts_prompt_and_returns_token_ids() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/generate"))
            .and(body_partial_json(
                json!({"model": "test-model", "stream": false, "prompt": "hi"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "response": "ok",
                "done": true,
                "done_reason": "stop",
                "prompt_eval_count": 2,
                "eval_count": 1,
                "context": [11, 12, 13]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model").with_generate_endpoint();
        let out = client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect("ok");
        assert_eq!(out.parsed_output, Value::String("ok".into()));
        assert_eq!(out.token_ids, Some(vec![11, 12, 13]));
        assert_eq!(
            out.usage,
            Some(turul_llm_core::Usage::new(Some(2), Some(1)))
        );
        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert!(
            body.get("raw").is_none(),
            "raw mode must not be offered: {body}"
        );
        assert!(body.get("messages").is_none(), "{body}");
    }

    const WEIGHTS: &str = "dde5aa3fc5ffc17176b5e8bdc82f587b24b2678c6c66101bf7da77af9f7ccdff";

    fn show_response(modelfile: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"modelfile": modelfile}))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn matching_model_digest_allows_the_call() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/show"))
            .and(body_partial_json(json!({"model": "test-model"})))
            .respond_with(show_response(&format!(
                "# Modelfile\n# FROM llama3.2:latest\nFROM /models/blobs/sha256-{WEIGHTS}\nTEMPLATE x"
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ok_envelope(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model")
            .with_model_digest(format!("sha256:{}", WEIGHTS.to_uppercase()));
        client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect("ok");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mismatching_model_digest_refuses_before_generating() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/show"))
            .respond_with(show_response("FROM /models/blobs/sha256-abcdef0123"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ok_envelope(json!({})))
            .expect(0)
            .mount(&server)
            .await;

        let client = OllamaClient::new(server.uri(), "test-model").with_model_digest(WEIGHTS);
        let err = client
            .complete(CompletionRequest::new("hi"))
            .await
            .expect_err("must fail");
        assert!(
            matches!(&err, LlmError::ModelMismatch { expected, actual, .. }
                if expected == WEIGHTS && actual == "abcdef0123"),
            "{err:?}"
        );
    }

    #[test]
    fn weights_digest_reads_the_first_non_comment_from_line() {
        assert_eq!(
            weights_digest(
                "# FROM x\nADAPTER /a/sha256-aa\nfrom /b/sha256-ABC123\nFROM /c/sha256-ff"
            ),
            Some("abc123".to_string())
        );
        assert_eq!(weights_digest("FROM llama3.2:latest"), None);
        assert_eq!(weights_digest("TEMPLATE x"), None);
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
        assert_eq!(c.endpoint_url(), "http://example.invalid:11434/api/chat");
    }
}

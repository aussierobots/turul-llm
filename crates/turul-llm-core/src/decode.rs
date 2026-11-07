//! Shared decoding of a chat-style provider response.
//!
//! Adapters own the request side (URL, headers, body shape) and hand the
//! raw HTTP result to [`ChatResponseShape::decode`], so status handling,
//! envelope parsing and the structured-output contract have a single
//! definition. Sending is `http::send_chat` (feature `http`).

use serde_json::Value;

use crate::error::LlmError;
use crate::response::CompletionResponse;

/// Where a chat-style provider puts the fields the decoder needs.
///
/// Both locators are RFC 6901 JSON pointers into the response envelope.
/// The struct is `#[non_exhaustive]` so further locators (for example
/// token usage) can be added without breaking adapters.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct ChatResponseShape<'a> {
    /// Pointer to the assistant message text.
    pub content_pointer: &'a str,
    /// Pointer to the provider's finish/done reason; optional in the
    /// response and used for diagnostics only.
    pub finish_reason_pointer: &'a str,
}

impl<'a> ChatResponseShape<'a> {
    /// Build a shape from the two JSON pointers.
    pub const fn new(content_pointer: &'a str, finish_reason_pointer: &'a str) -> Self {
        Self {
            content_pointer,
            finish_reason_pointer,
        }
    }

    /// Decode an HTTP response body from a chat-style provider.
    ///
    /// - A non-2xx `status` becomes [`LlmError::Provider`].
    /// - A missing content string is [`LlmError::Provider`].
    /// - When `structured` is true the content must parse as JSON,
    ///   otherwise [`LlmError::SchemaViolation`] is returned with the
    ///   finish reason in the message (a reason such as `length` points
    ///   at truncation, not at a model that ignored the schema). When
    ///   false, the raw text is wrapped as a JSON string so
    ///   `parsed_output` is always populated.
    pub fn decode(
        &self,
        url: &str,
        status: u16,
        body: &str,
        structured: bool,
    ) -> Result<CompletionResponse, LlmError> {
        if !(200..300).contains(&status) {
            return Err(LlmError::Provider(format!(
                "{url} returned HTTP {status}: {body}"
            )));
        }

        let envelope: Value = serde_json::from_str(body)
            .map_err(|e| LlmError::Provider(format!("envelope parse failed: {e}: {body}")))?;
        let content = envelope
            .pointer(self.content_pointer)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                LlmError::Provider(format!(
                    "response missing {}: {envelope}",
                    self.content_pointer
                ))
            })?;

        let parsed = if structured {
            serde_json::from_str::<Value>(content).map_err(|e| {
                let finish_reason = envelope
                    .pointer(self.finish_reason_pointer)
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                LlmError::SchemaViolation(format!(
                    "structured-output payload was not valid JSON (finish_reason={finish_reason}): {e}: {content}"
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

    const URL: &str = "http://example.invalid/chat";
    const CONTENT: &str = "/message/content";
    const REASON: &str = "/done_reason";

    fn decode(status: u16, body: &str, structured: bool) -> Result<CompletionResponse, LlmError> {
        ChatResponseShape::new(CONTENT, REASON).decode(URL, status, body, structured)
    }

    #[test]
    fn non_success_status_is_provider_error() {
        let err = decode(500, "boom", false).expect_err("must fail");
        assert!(matches!(&err, LlmError::Provider(m) if m.contains("HTTP 500") && m.contains(URL)));
    }

    #[test]
    fn unparseable_envelope_is_provider_error() {
        let err = decode(200, "not json", false).expect_err("must fail");
        assert!(matches!(&err, LlmError::Provider(m) if m.contains("envelope parse failed")));
    }

    #[test]
    fn missing_content_is_provider_error() {
        let err = decode(200, r#"{"message":{}}"#, false).expect_err("must fail");
        assert!(matches!(&err, LlmError::Provider(m) if m.contains(CONTENT)));
    }

    #[test]
    fn freeform_content_is_wrapped_as_json_string() {
        let body = json!({"message": {"content": "Hello."}}).to_string();
        let out = decode(200, &body, false).expect("ok");
        assert_eq!(out.parsed_output, Value::String("Hello.".into()));
    }

    #[test]
    fn structured_content_is_parsed() {
        let body = json!({"message": {"content": "{\"a\":1}"}}).to_string();
        let out = decode(200, &body, true).expect("ok");
        assert_eq!(out.parsed_output, json!({"a": 1}));
    }

    #[test]
    fn truncated_structured_content_reports_finish_reason() {
        let body =
            json!({"message": {"content": "{\"a\": \"cut"}, "done_reason": "length"}).to_string();
        let err = decode(200, &body, true).expect_err("must fail");
        assert!(
            matches!(&err, LlmError::SchemaViolation(m) if m.contains("finish_reason=length")),
            "{err:?}"
        );
    }

    #[test]
    fn missing_finish_reason_is_reported_as_unknown() {
        let body = json!({"message": {"content": "nope"}}).to_string();
        let err = decode(200, &body, true).expect_err("must fail");
        assert!(
            matches!(&err, LlmError::SchemaViolation(m) if m.contains("finish_reason=unknown"))
        );
    }
}

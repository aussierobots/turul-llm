//! Shared decoding of a chat-style provider response.
//!
//! Adapters own the request side (URL, headers, body shape) and hand the
//! raw HTTP result to [`ChatResponseShape::decode`], so status handling,
//! envelope parsing and the structured-output contract have a single
//! definition. Sending is `http::send_chat` (feature `http`).

use serde_json::Value;

use crate::error::LlmError;
use crate::response::{CompletionResponse, StopReason, Usage};

/// Where a chat-style provider puts the fields the decoder needs.
///
/// The content and finish-reason locators are required; token usage and
/// token ids are optional and added with the `with_*` builders. All are
/// RFC 6901 JSON pointers into the response envelope. The struct is
/// `#[non_exhaustive]` so further locators can be added without breaking
/// adapters.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct ChatResponseShape<'a> {
    /// Pointer to the assistant message text.
    pub content_pointer: &'a str,
    /// Pointer to the provider's finish/done reason. The value may be
    /// absent from a response; it becomes the response's stop reason and
    /// is quoted in structured-output parse errors.
    pub finish_reason_pointer: &'a str,
    /// Pointer to the prompt token count, if the provider reports one.
    pub prompt_tokens_pointer: Option<&'a str>,
    /// Pointer to the generated token count, if the provider reports one.
    pub output_tokens_pointer: Option<&'a str>,
    /// Pointer to an array of token ids, if the provider returns one.
    pub token_ids_pointer: Option<&'a str>,
}

impl<'a> ChatResponseShape<'a> {
    /// Build a shape from the two JSON pointers.
    pub const fn new(content_pointer: &'a str, finish_reason_pointer: &'a str) -> Self {
        Self {
            content_pointer,
            finish_reason_pointer,
            prompt_tokens_pointer: None,
            output_tokens_pointer: None,
            token_ids_pointer: None,
        }
    }

    /// Also read token usage from the given pointers.
    pub const fn with_usage_pointers(
        mut self,
        prompt_tokens_pointer: &'a str,
        output_tokens_pointer: &'a str,
    ) -> Self {
        self.prompt_tokens_pointer = Some(prompt_tokens_pointer);
        self.output_tokens_pointer = Some(output_tokens_pointer);
        self
    }

    /// Also read an array of token ids from the given pointer. A value
    /// at that pointer that is not an array of token ids is a provider
    /// error; an absent value is `None`.
    pub const fn with_token_ids_pointer(mut self, token_ids_pointer: &'a str) -> Self {
        self.token_ids_pointer = Some(token_ids_pointer);
        self
    }

    /// Decode an HTTP response body from a chat-style provider.
    ///
    /// - A non-2xx `status` becomes [`LlmError::Provider`].
    /// - A missing content string is [`LlmError::Provider`].
    /// - The finish reason and token counts, when present, are returned
    ///   on the response as [`StopReason`] and [`Usage`]; absent values
    ///   stay `None`.
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

        let mut response = CompletionResponse::new(parsed);
        if let Some(reason) = envelope
            .pointer(self.finish_reason_pointer)
            .and_then(Value::as_str)
        {
            response = response.with_stop_reason(StopReason::from_provider(reason));
        }
        let count = |pointer: Option<&str>| -> Result<Option<u32>, LlmError> {
            let Some((pointer, value)) = pointer.and_then(|p| Some((p, envelope.pointer(p)?)))
            else {
                return Ok(None);
            };
            value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .map(Some)
                .ok_or_else(|| {
                    LlmError::Provider(format!("{pointer} is not a token count: {value}"))
                })
        };
        if let Some(value) = self.token_ids_pointer.and_then(|p| envelope.pointer(p)) {
            let ids = value
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
                        .collect::<Option<Vec<u32>>>()
                })
                .ok_or_else(|| {
                    LlmError::Provider(format!(
                        "{} is not an array of token ids",
                        self.token_ids_pointer.unwrap_or_default()
                    ))
                })?;
            response = response.with_token_ids(ids);
        }
        let usage = Usage::new(
            count(self.prompt_tokens_pointer)?,
            count(self.output_tokens_pointer)?,
        );
        if usage != Usage::default() {
            response = response.with_usage(usage);
        }
        Ok(response)
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
    fn stop_reason_and_usage_are_decoded() {
        let body = json!({
            "message": {"content": "hi"},
            "done_reason": "length",
            "pe": 7,
            "ev": 3
        })
        .to_string();
        let out = ChatResponseShape::new(CONTENT, REASON)
            .with_usage_pointers("/pe", "/ev")
            .decode(URL, 200, &body, false)
            .expect("ok");
        assert_eq!(out.stop_reason, Some(StopReason::Length));
        assert_eq!(out.usage, Some(Usage::new(Some(7), Some(3))));
    }

    #[test]
    fn absent_stop_reason_and_usage_stay_none() {
        let body = json!({"message": {"content": "hi"}}).to_string();
        let out = ChatResponseShape::new(CONTENT, REASON)
            .with_usage_pointers("/pe", "/ev")
            .decode(URL, 200, &body, false)
            .expect("ok");
        assert_eq!(out.stop_reason, None);
        assert_eq!(out.usage, None);
    }

    #[test]
    fn token_ids_are_decoded_verbatim_and_validated() {
        let shape = ChatResponseShape::new(CONTENT, REASON).with_token_ids_pointer("/ctx");
        let body = json!({"message": {"content": "hi"}, "ctx": [1, 2, 3]}).to_string();
        assert_eq!(
            shape.decode(URL, 200, &body, false).unwrap().token_ids,
            Some(vec![1, 2, 3])
        );

        let absent = json!({"message": {"content": "hi"}}).to_string();
        assert_eq!(
            shape.decode(URL, 200, &absent, false).unwrap().token_ids,
            None
        );

        let bad = json!({"message": {"content": "hi"}, "ctx": [1, "x"]}).to_string();
        let err = shape.decode(URL, 200, &bad, false).expect_err("must fail");
        assert!(
            matches!(&err, LlmError::Provider(m) if m.contains("/ctx")),
            "{err:?}"
        );
    }

    #[test]
    fn malformed_token_count_is_a_provider_error() {
        let shape = ChatResponseShape::new(CONTENT, REASON).with_usage_pointers("/pe", "/ev");
        for bad in [json!(-1), json!(1.5), json!("7"), json!(5_000_000_000u64)] {
            let body = json!({"message": {"content": "hi"}, "pe": bad, "ev": 1}).to_string();
            let err = shape.decode(URL, 200, &body, false).expect_err("must fail");
            assert!(
                matches!(&err, LlmError::Provider(m) if m.contains("/pe")),
                "{bad}: {err:?}"
            );
        }
    }

    #[test]
    fn unknown_stop_reason_is_kept_verbatim() {
        let body =
            json!({"message": {"content": "hi"}, "done_reason": "content_filter"}).to_string();
        let out = decode(200, &body, false).expect("ok");
        assert_eq!(
            out.stop_reason,
            Some(StopReason::Other("content_filter".into()))
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

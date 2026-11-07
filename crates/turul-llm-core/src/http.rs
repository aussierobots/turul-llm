//! Shared HTTP send path for chat-style adapters (feature `http`).
//!
//! Adapters build the provider-specific request (URL, headers, body) and
//! hand it to [`send_chat`], which owns transport error classification
//! and response decoding. The trait and types in this crate do not
//! depend on `reqwest`; only this module does, and only when the `http`
//! feature is enabled.
//!
//! # Semver
//!
//! [`send_chat`] takes a `reqwest::RequestBuilder`, so this module's
//! public API follows reqwest's major version: moving to a new reqwest
//! major is a breaking change of the `http` feature, not of the trait,
//! types or errors in this crate.

use crate::decode::ChatResponseShape;
use crate::error::LlmError;
use crate::response::CompletionResponse;

/// Send `request` and decode the reply.
///
/// - A timeout while sending becomes [`LlmError::Timeout`]; any other
///   send or body-read failure becomes [`LlmError::Transport`].
/// - The status and body are decoded by [`ChatResponseShape::decode`].
///
/// `url` is used only to give error messages context.
pub async fn send_chat(
    request: reqwest::RequestBuilder,
    url: &str,
    shape: &ChatResponseShape<'_>,
    structured: bool,
) -> Result<CompletionResponse, LlmError> {
    let resp = request.send().await.map_err(|e| {
        if e.is_timeout() {
            LlmError::Timeout
        } else {
            LlmError::Transport(format!("POST {url} failed: {e}"))
        }
    })?;

    let status = resp.status().as_u16();
    let text = resp
        .text()
        .await
        .map_err(|e| LlmError::Transport(format!("read body from {url} failed: {e}")))?;

    shape.decode(url, status, &text, structured)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SHAPE: ChatResponseShape<'static> = ChatResponseShape::new("/message/content", "/done");

    #[tokio::test(flavor = "current_thread")]
    async fn success_is_decoded() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"message": {"content": "hi"}})),
            )
            .mount(&server)
            .await;

        let url = format!("{}/chat", server.uri());
        let out = send_chat(reqwest::Client::new().post(&url), &url, &SHAPE, false)
            .await
            .expect("ok");
        assert_eq!(out.parsed_output, Value::String("hi".into()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn slow_server_maps_to_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
            .mount(&server)
            .await;

        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let url = server.uri();
        let err = send_chat(http.post(&url), &url, &SHAPE, false)
            .await
            .expect_err("must fail");
        assert!(matches!(err, LlmError::Timeout), "got {err:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unreachable_server_maps_to_transport() {
        let url = "http://127.0.0.1:1/chat";
        let err = send_chat(reqwest::Client::new().post(url), url, &SHAPE, false)
            .await
            .expect_err("must fail");
        assert!(matches!(err, LlmError::Transport(_)), "got {err:?}");
    }
}

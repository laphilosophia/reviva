use httpmock::{Method::POST, MockServer};
use reviva_backend::{BackendError, CompletionBackend, LlamaCompletionBackend};
use reviva_core::{BackendSettings, ResponseInterpretation, RevivaRequest};
use std::time::Duration;

fn request(base_url: String, timeout_ms: u64) -> RevivaRequest {
    RevivaRequest {
        backend: BackendSettings {
            base_url,
            model: Some("test-model".to_string()),
            temperature: 0.1,
            max_tokens: 256,
            timeout_ms,
            stop_sequences: Vec::new(),
        },
        prompt: "review this".to_string(),
    }
}

#[test]
fn parses_valid_completion_response() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"content":"SUMMARY:\n- good"}"#);
    });

    let backend = LlamaCompletionBackend::new();
    let response = backend
        .complete(&request(server.url(""), 10_000))
        .expect("request should succeed");
    mock.assert();
    match response.response_interpretation {
        ResponseInterpretation::Completed { content } => {
            assert!(content.contains("SUMMARY"));
        }
        _ => panic!("expected completed interpretation"),
    }
    assert!(response.raw_http_body.contains("\"content\""));
}

#[test]
fn falls_back_to_openai_completion_endpoint() {
    let server = MockServer::start();
    let legacy_mock = server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"error":"Unexpected endpoint or method. (POST /completion)"}"#);
    });
    let openai_mock = server.mock(|when, then| {
        when.method(POST).path("/v1/completions");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"choices":[{"text":"SUMMARY:\n- fallback works"}]}"#);
    });

    let backend = LlamaCompletionBackend::new();
    let response = backend
        .complete(&request(server.url(""), 10_000))
        .expect("fallback request should succeed");

    legacy_mock.assert();
    openai_mock.assert();
    match response.response_interpretation {
        ResponseInterpretation::Completed { content } => {
            assert!(content.contains("fallback works"));
        }
        _ => panic!("expected completed interpretation"),
    }
    assert!(response.raw_http_body.contains("\"choices\""));
}

#[test]
fn empty_response_maps_to_empty_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).body(r#"{"content":""}"#);
    });

    let backend = LlamaCompletionBackend::new();
    let error = backend
        .complete(&request(server.url(""), 10_000))
        .expect_err("should fail");
    match error {
        BackendError::EmptyResponse {
            status_code,
            raw_http_body,
        } => {
            assert_eq!(status_code, 200);
            assert!(raw_http_body.contains("content"));
        }
        _ => panic!("unexpected error: {error:?}"),
    }
}

#[test]
fn malformed_json_maps_to_malformed_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).body("not-json");
    });

    let backend = LlamaCompletionBackend::new();
    let error = backend
        .complete(&request(server.url(""), 10_000))
        .expect_err("should fail");
    match error {
        BackendError::MalformedResponse {
            status_code,
            raw_http_body,
        } => {
            assert_eq!(status_code, 200);
            assert_eq!(raw_http_body, "not-json");
        }
        _ => panic!("unexpected error: {error:?}"),
    }
}

#[test]
fn server_error_maps_to_server_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(503).body("down");
    });

    let backend = LlamaCompletionBackend::new();
    let error = backend
        .complete(&request(server.url(""), 10_000))
        .expect_err("should fail");
    match error {
        BackendError::ServerError {
            status_code,
            raw_http_body,
        } => {
            assert_eq!(status_code, 503);
            assert_eq!(raw_http_body, "down");
        }
        _ => panic!("unexpected error: {error:?}"),
    }
}

#[test]
fn unreachable_backend_maps_to_unreachable() {
    let backend = LlamaCompletionBackend::new();
    let error = backend
        .complete(&request("http://127.0.0.1:1".to_string(), 300))
        .expect_err("should fail");
    match error {
        BackendError::Unreachable(_) | BackendError::Transport(_) | BackendError::Timeout => {}
        _ => panic!("unexpected error: {error:?}"),
    }
}

#[test]
fn timeout_maps_to_timeout_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200)
            .delay(Duration::from_millis(250))
            .body(r#"{"content":"late"}"#);
    });

    let backend = LlamaCompletionBackend::new();
    let error = backend
        .complete(&request(server.url(""), 50))
        .expect_err("should timeout");
    match error {
        BackendError::Timeout | BackendError::Transport(_) => {}
        _ => panic!("unexpected error: {error:?}"),
    }
}

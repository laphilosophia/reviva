use reqwest::blocking::Client;
use reviva_core::{ResponseInterpretation, RevivaRequest, RevivaResponse};
use serde_json::Value;
use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    Unreachable(String),
    Timeout,
    ServerError {
        status_code: u16,
        raw_http_body: String,
    },
    HttpError {
        status_code: u16,
        raw_http_body: String,
    },
    EmptyResponse {
        status_code: u16,
        raw_http_body: String,
    },
    MalformedResponse {
        status_code: u16,
        raw_http_body: String,
    },
    Transport(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(message) => write!(f, "backend unreachable: {message}"),
            Self::Timeout => f.write_str("backend request timed out"),
            Self::ServerError { status_code, .. } => {
                write!(f, "backend server error: HTTP {status_code}")
            }
            Self::HttpError { status_code, .. } => {
                write!(f, "backend returned HTTP error: {status_code}")
            }
            Self::EmptyResponse { status_code, .. } => {
                write!(f, "backend returned empty response: HTTP {status_code}")
            }
            Self::MalformedResponse { status_code, .. } => write!(
                f,
                "backend returned malformed response payload: HTTP {status_code}"
            ),
            Self::Transport(message) => write!(f, "backend transport error: {message}"),
        }
    }
}

impl std::error::Error for BackendError {}

pub trait CompletionBackend {
    fn complete(&self, request: &RevivaRequest) -> Result<RevivaResponse, BackendError>;
}

#[derive(Debug, Default, Clone)]
pub struct LlamaCompletionBackend;

impl LlamaCompletionBackend {
    pub fn new() -> Self {
        Self
    }
}

impl CompletionBackend for LlamaCompletionBackend {
    fn complete(&self, request: &RevivaRequest) -> Result<RevivaResponse, BackendError> {
        let timeout = Duration::from_millis(request.backend.timeout_ms);
        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .build()
            .map_err(|error| BackendError::Transport(error.to_string()))?;
        match complete_via_legacy_endpoint(&client, request) {
            Ok(response) => Ok(response),
            Err(error) if should_try_openai_fallback(&error) => {
                complete_via_openai_endpoint(&client, request)
            }
            Err(error) => Err(error),
        }
    }
}

fn complete_via_legacy_endpoint(
    client: &Client,
    request: &RevivaRequest,
) -> Result<RevivaResponse, BackendError> {
    let url = format!(
        "{}/completion",
        request.backend.base_url.trim_end_matches('/')
    );
    let (status_code, raw_http_body) = post_json(client, &url, &legacy_payload(request))?;
    parse_legacy_completion_response(status_code, raw_http_body)
}

fn complete_via_openai_endpoint(
    client: &Client,
    request: &RevivaRequest,
) -> Result<RevivaResponse, BackendError> {
    let url = format!(
        "{}/v1/completions",
        request.backend.base_url.trim_end_matches('/')
    );
    let (status_code, raw_http_body) = post_json(client, &url, &openai_payload(request))?;
    parse_openai_completion_response(status_code, raw_http_body)
}

fn post_json(client: &Client, url: &str, payload: &Value) -> Result<(u16, String), BackendError> {
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .json(payload)
        .send();
    match response {
        Ok(response) => {
            let status_code = response.status().as_u16();
            let raw_http_body = response
                .text()
                .map_err(|error| BackendError::Transport(error.to_string()))?;
            Ok((status_code, raw_http_body))
        }
        Err(error) => {
            if error.is_timeout() {
                return Err(BackendError::Timeout);
            }
            if error.is_connect() {
                return Err(BackendError::Unreachable(error.to_string()));
            }
            Err(BackendError::Transport(error.to_string()))
        }
    }
}

fn legacy_payload(request: &RevivaRequest) -> Value {
    let mut payload = serde_json::json!({
        "prompt": request.prompt,
        "temperature": request.backend.temperature,
        "n_predict": request.backend.max_tokens,
        "stop": request.backend.stop_sequences,
        "cache_prompt": request.backend.cache_prompt,
    });
    if let Some(slot_id) = request.backend.slot_id {
        payload["id_slot"] = Value::Number(slot_id.into());
    }
    if let Some(model) = &request.backend.model {
        payload["model"] = Value::String(model.clone());
    }
    payload
}

fn openai_payload(request: &RevivaRequest) -> Value {
    let mut payload = serde_json::json!({
        "prompt": request.prompt,
        "temperature": request.backend.temperature,
        "max_tokens": request.backend.max_tokens,
        "stop": request.backend.stop_sequences,
    });
    if let Some(model) = &request.backend.model {
        payload["model"] = Value::String(model.clone());
    }
    payload
}

fn parse_legacy_completion_response(
    status_code: u16,
    raw_http_body: String,
) -> Result<RevivaResponse, BackendError> {
    if status_code >= 500 {
        return Err(BackendError::ServerError {
            status_code,
            raw_http_body,
        });
    }
    if status_code >= 400 {
        return Err(BackendError::HttpError {
            status_code,
            raw_http_body,
        });
    }
    if raw_http_body.trim().is_empty() {
        return Err(BackendError::EmptyResponse {
            status_code,
            raw_http_body,
        });
    }

    let json: Value =
        serde_json::from_str(&raw_http_body).map_err(|_| BackendError::MalformedResponse {
            status_code,
            raw_http_body: raw_http_body.clone(),
        })?;
    let Some(content) = json.get("content").and_then(Value::as_str) else {
        return Err(BackendError::MalformedResponse {
            status_code,
            raw_http_body,
        });
    };
    if content.trim().is_empty() {
        return Err(BackendError::EmptyResponse {
            status_code,
            raw_http_body,
        });
    }

    Ok(RevivaResponse {
        status_code: Some(status_code),
        raw_http_body,
        response_interpretation: ResponseInterpretation::Completed {
            content: content.to_string(),
        },
    })
}

fn parse_openai_completion_response(
    status_code: u16,
    raw_http_body: String,
) -> Result<RevivaResponse, BackendError> {
    if status_code >= 500 {
        return Err(BackendError::ServerError {
            status_code,
            raw_http_body,
        });
    }
    if status_code >= 400 {
        return Err(BackendError::HttpError {
            status_code,
            raw_http_body,
        });
    }
    if raw_http_body.trim().is_empty() {
        return Err(BackendError::EmptyResponse {
            status_code,
            raw_http_body,
        });
    }

    let json: Value =
        serde_json::from_str(&raw_http_body).map_err(|_| BackendError::MalformedResponse {
            status_code,
            raw_http_body: raw_http_body.clone(),
        })?;
    let Some(content) = json
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("text"))
        .and_then(Value::as_str)
    else {
        return Err(BackendError::MalformedResponse {
            status_code,
            raw_http_body,
        });
    };
    if content.trim().is_empty() {
        return Err(BackendError::EmptyResponse {
            status_code,
            raw_http_body,
        });
    }

    Ok(RevivaResponse {
        status_code: Some(status_code),
        raw_http_body,
        response_interpretation: ResponseInterpretation::Completed {
            content: content.to_string(),
        },
    })
}

fn should_try_openai_fallback(error: &BackendError) -> bool {
    match error {
        BackendError::HttpError {
            status_code,
            raw_http_body,
        } => {
            *status_code == 404
                || *status_code == 405
                || raw_http_body.contains("Unexpected endpoint or method")
        }
        BackendError::MalformedResponse { raw_http_body, .. } => {
            raw_http_body.contains("Unexpected endpoint or method")
        }
        _ => false,
    }
}

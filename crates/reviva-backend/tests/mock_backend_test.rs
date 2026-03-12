//! Mock backend integration tests for reviva-backend.
//!
//! These tests spin up a local HTTP mock server (httpmock).
//! No live inference server is required.
//!
//! Coverage targets (populated as reviva-backend is implemented):
//!   - valid completion response is parsed correctly
//!   - empty response body is surfaced as BackendError::EmptyResponse
//!   - malformed JSON is surfaced as BackendError::MalformedResponse
//!   - HTTP 5xx is surfaced as BackendError::ServerError
//!   - connection refused is surfaced as BackendError::Unreachable
//!   - timeout is surfaced as BackendError::Timeout (use short timeout in test)
//!   - raw response is always preserved regardless of error kind

// Placeholder — will be filled as reviva-backend is implemented.
#[test]
fn placeholder_mock_backend() {
    // TODO: start a MockServer, configure each error scenario,
    //       call the backend client, assert the correct BackendError variant.
    assert!(true, "placeholder — replace with httpmock test");
}

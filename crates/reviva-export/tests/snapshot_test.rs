use reviva_core::{
    BackendSettings, Confidence, Finding, NormalizationState, ProfileMetadata,
    ResponseInterpretation, RevivaMode, RevivaResponse, RevivaTarget, Session, Severity,
    SeverityOrigin,
};
use reviva_export::{export_session_json, export_session_markdown};

fn fixture_session() -> Session {
    Session {
        id: "session-123".to_string(),
        repository_root: "/repo".to_string(),
        review_mode: RevivaMode::Boundary,
        selected_target: RevivaTarget::Boundary(reviva_core::BoundaryTarget {
            left: "src/left.rs".to_string(),
            right: "src/right.rs".to_string(),
        }),
        prompt_preview: "PROMPT".to_string(),
        prompt_sent: "PROMPT".to_string(),
        backend: BackendSettings {
            base_url: "http://127.0.0.1:8080".to_string(),
            model: Some("model".to_string()),
            temperature: 0.1,
            max_tokens: 256,
            timeout_ms: 1000,
            stop_sequences: vec![],
            cache_prompt: false,
            slot_id: None,
        },
        response: RevivaResponse {
            status_code: Some(200),
            raw_http_body: "{\"content\":\"ok\"}".to_string(),
            response_interpretation: ResponseInterpretation::Completed {
                content: "SUMMARY:\n- ok".to_string(),
            },
        },
        findings: vec![Finding {
            id: "f1".to_string(),
            session_id: "session-123".to_string(),
            review_mode: RevivaMode::Boundary,
            target: "src/left.rs,src/right.rs".to_string(),
            summary: "Boundary contract mismatch".to_string(),
            why_it_matters: Some("May leak internal assumptions".to_string()),
            severity: Some(Severity::High),
            severity_origin: SeverityOrigin::ModelLabeled,
            confidence: Confidence::Medium,
            risk_class: None,
            action: Some("Normalize contract at adapter".to_string()),
            status: None,
            location_hint: Some("src/adapter.rs".to_string()),
            evidence_text: Some("left returns Option, right expects Result".to_string()),
            raw_labels: vec!["high".to_string()],
            normalization_state: NormalizationState::Structured,
        }],
        profile: ProfileMetadata {
            name: "launch-readiness".to_string(),
            source: "built_in".to_string(),
            path: None,
            hash: "profile-hash".to_string(),
        },
        created_at: "1700000000".to_string(),
        warnings: vec![],
    }
}

#[test]
fn markdown_export_snapshot() {
    let markdown = export_session_markdown(&fixture_session());
    insta::assert_snapshot!(
        markdown,
        @r###"
# Reviva Session Export

- Session ID: `session-123`
- Created At: `1700000000`
- Mode: `boundary`
- Profile: `launch-readiness`
- Profile Source: `built_in`
- Profile Hash: `profile-hash`
- Target: `boundary:left=src/left.rs right=src/right.rs`

## Prompt

```text
PROMPT
```

## Raw Response

```text
{"content":"ok"}
```

## Findings

### Boundary contract mismatch

- Normalization State: `structured`
- Severity Origin: `model_labeled`
- Severity: `high`
- Confidence: `medium`
- Location: `src/adapter.rs`
- Evidence: left returns Option, right expects Result
- Why It Matters: May leak internal assumptions
- Action: Normalize contract at adapter

"###
    );
}

#[test]
fn json_export_is_valid_and_contains_fields() {
    let json = export_session_json(&fixture_session());
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(parsed["session"]["id"], "session-123");
    assert_eq!(parsed["session"]["profile"]["name"], "launch-readiness");
    assert_eq!(parsed["findings"][0]["normalization_state"], "structured");
    assert_eq!(parsed["findings"][0]["severity_origin"], "model_labeled");
}

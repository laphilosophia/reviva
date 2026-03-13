use reviva::core::{
    BackendSettings, Confidence, Finding, NormalizationState, ProfileMetadata,
    ResponseInterpretation, RevivaMode, RevivaResponse, RevivaTarget, Session, Severity,
    SeverityOrigin,
};
use reviva::export::{export_session_json, export_session_markdown};

fn fixture_session() -> Session {
    Session {
        id: "session-123".to_string(),
        repository_root: "/repo".to_string(),
        review_mode: RevivaMode::Boundary,
        selected_target: RevivaTarget::Boundary(reviva::core::BoundaryTarget {
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

fn fixture_session_with_duplicates() -> Session {
    let mut session = fixture_session();
    session.findings.push(Finding {
        id: "f2".to_string(),
        session_id: "session-123".to_string(),
        review_mode: RevivaMode::Boundary,
        target: "src/left.rs,src/right.rs".to_string(),
        summary: "  boundary contract mismatch  ".to_string(),
        why_it_matters: Some("Repeated signal".to_string()),
        severity: Some(Severity::High),
        severity_origin: SeverityOrigin::Normalized,
        confidence: Confidence::Low,
        risk_class: Some("operator-trust".to_string()),
        action: Some("Deduplicate in triage".to_string()),
        status: None,
        location_hint: Some("src/adapter.rs".to_string()),
        evidence_text: Some("same issue stated differently".to_string()),
        raw_labels: vec!["high".to_string()],
        normalization_state: NormalizationState::Partial,
    });
    session
}

fn fixture_session_with_incremental_warnings() -> Session {
    let mut session = fixture_session();
    session.warnings = vec![
        "incremental_from=HEAD".to_string(),
        "incremental_scope=diff_hunks".to_string(),
        "incremental_context_lines=3".to_string(),
        "incremental_file_count=2".to_string(),
        "incremental_fallback_full_file_count=1".to_string(),
        "incremental_fallback_full_file=src/main.rs".to_string(),
    ];
    session
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

## Prompt Metadata

- Prompt Preview Equals Sent: `true`
- Prompt Chars: `6`
- Prompt Lines: `1`
- Prompt Hash (fnv1a64): `fdcb138bff2ccc3b`
- Prompt Body: `stored_in_session`

## Parsed Response

```text
SUMMARY:
- ok
```

- Raw Body Bytes (stored in session): `16`

## Triage Diagnostics

- Duplicate Summary Clusters: `0`
- Duplicate Summary Findings: `0`
- Repeated Summaries: `none`

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
    assert_eq!(
        parsed["session"]["response"]["response_interpretation"]["kind"],
        "completed"
    );
    assert!(parsed["session"]["response"]["raw_http_body"].is_null());
    assert_eq!(parsed["session"]["incremental"]["enabled"], false);
    assert_eq!(parsed["session"]["triage"]["total_findings"], 1);
    assert_eq!(parsed["session"]["triage"]["duplicate_summary_clusters"], 0);
    assert_eq!(parsed["session"]["triage"]["duplicate_summary_findings"], 0);
    assert_eq!(parsed["session"]["prompt"]["stored_in_session"], true);
    assert_eq!(parsed["session"]["prompt"]["preview_equals_sent"], true);
    assert_eq!(parsed["session"]["prompt"]["chars"], 6);
    assert_eq!(parsed["findings"][0]["normalization_state"], "structured");
    assert_eq!(parsed["findings"][0]["severity_origin"], "model_labeled");
}

#[test]
fn json_export_reports_duplicate_summary_clusters() {
    let json = export_session_json(&fixture_session_with_duplicates());
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(parsed["session"]["triage"]["total_findings"], 2);
    assert_eq!(parsed["session"]["triage"]["duplicate_summary_clusters"], 1);
    assert_eq!(parsed["session"]["triage"]["duplicate_summary_findings"], 2);
    assert_eq!(
        parsed["session"]["triage"]["repeated_summaries"][0]["count"],
        2
    );
    assert_eq!(
        parsed["session"]["triage"]["repeated_summaries"][0]["summary"],
        "boundary contract mismatch"
    );
}

#[test]
fn markdown_export_surfaces_incremental_scope_when_present() {
    let markdown = export_session_markdown(&fixture_session_with_incremental_warnings());
    assert!(markdown.contains("## Incremental Scope"));
    assert!(markdown.contains("- Incremental Mode: `enabled`"));
    assert!(markdown.contains("- Base Ref: `HEAD`"));
    assert!(markdown.contains("- Scope: `diff_hunks`"));
    assert!(markdown.contains("- Full-File Fallback Count: `1`"));
    assert!(markdown.contains("`src/main.rs`"));
}

#[test]
fn json_export_surfaces_incremental_scope_when_present() {
    let json = export_session_json(&fixture_session_with_incremental_warnings());
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(parsed["session"]["incremental"]["enabled"], true);
    assert_eq!(parsed["session"]["incremental"]["from"], "HEAD");
    assert_eq!(parsed["session"]["incremental"]["scope"], "diff_hunks");
    assert_eq!(parsed["session"]["incremental"]["context_lines"], 3);
    assert_eq!(parsed["session"]["incremental"]["file_count"], 2);
    assert_eq!(
        parsed["session"]["incremental"]["fallback_full_file_count"],
        1
    );
    assert_eq!(
        parsed["session"]["incremental"]["fallback_full_files"][0],
        "src/main.rs"
    );
}

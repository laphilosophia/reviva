use reviva_core::{BoundaryTarget, RevivaMode, RevivaTarget};
use reviva_prompts::{
    apply_prompt_wrapper, build_prompt, default_review_profile, normalize_findings,
    normalize_findings_with_reasons, parse_prompt_wrapper, parse_review_profile_toml,
    PromptBuildConfig, PromptFile, PromptWrapper,
};

fn base_file(path: &str) -> PromptFile {
    PromptFile {
        path: path.to_string(),
        content: "fn main() {\n    println!(\"ok\");\n}\n".to_string(),
        estimated_tokens: 12,
        suspicion: None,
    }
}

#[test]
fn all_modes_render_stable_shape() {
    let profile = default_review_profile();
    for mode in RevivaMode::all() {
        let result = build_prompt(
            *mode,
            &profile,
            &RevivaTarget::Single("src/lib.rs".to_string()),
            &[base_file("src/lib.rs")],
            Some("focus on production risk"),
            &PromptBuildConfig::default(),
        )
        .expect("prompt should build");

        assert!(result.prompt.contains("REVIVA REVIEW REQUEST"));
        assert!(result.prompt.contains(&format!("Mode: {}", mode.as_str())));
        assert!(result.prompt.contains("Output contract"));
        assert!(result.prompt.contains("focus on production risk"));
    }
}

#[test]
fn boundary_ordering_is_left_then_right() {
    let profile = default_review_profile();
    let result = build_prompt(
        RevivaMode::Boundary,
        &profile,
        &RevivaTarget::Boundary(BoundaryTarget {
            left: "src/left.rs".to_string(),
            right: "src/right.rs".to_string(),
        }),
        &[base_file("src/left.rs"), base_file("src/right.rs")],
        None,
        &PromptBuildConfig::default(),
    )
    .expect("boundary prompt should build");

    insta::assert_snapshot!(
        result.prompt,
        @r###"
REVIVA REVIEW REQUEST
=====================
You are a constrained code reviewer.
Do not invent context outside provided files.
Do not ask follow-up questions.
Do not propose autonomous workflows.

Mode: boundary
Profile: default
Profile goal: Focused semantic review with explicit constraints.
Profile focus: correctness, security, boundedness, failure-semantics, maintainability
Profile severity scale: low, medium, high, critical, unknown
Profile confidence scale: low, medium, high, unknown
Profile risk classes: correctness, security, memory, performance, maintainability, operator-trust, public-surface
Focus: Inspect trust boundaries, adapter leakage, normalization drift, and ownership ambiguity.
Target: boundary:left=src/left.rs right=src/right.rs
Estimated token budget is heuristic, not exact.

Selected files:
- src/left.rs (estimated_tokens=12 )
- src/right.rs (estimated_tokens=12 )

Code:

--- BEGIN FILE src/left.rs ---
fn main() {
    println!("ok");
}
--- END FILE src/left.rs ---

--- BEGIN FILE src/right.rs ---
fn main() {
    println!("ok");
}
--- END FILE src/right.rs ---

Output contract (plain text):
SUMMARY:
- one short summary line
FINDINGS:
- summary: <text>
severity: <low|medium|high|critical|unknown>
confidence: <low|medium|high|unknown>
risk_class: <correctness|security|memory|performance|maintainability|operator-trust|public-surface|unknown>
location: <path or symbol>
evidence: <short quote or hint>
why: <impact>
action: <fix guidance>
"###
    );
}

#[test]
fn oversized_budget_refusal_message_is_explicit() {
    let profile = default_review_profile();
    let error = build_prompt(
        RevivaMode::Contract,
        &profile,
        &RevivaTarget::Single("src/huge.rs".to_string()),
        &[PromptFile {
            path: "src/huge.rs".to_string(),
            content: "x".repeat(10_000),
            estimated_tokens: 5000,
            suspicion: None,
        }],
        None,
        &PromptBuildConfig {
            estimated_prompt_tokens: 100,
        },
    )
    .expect_err("must reject oversized prompt");

    assert_eq!(
        error.to_string(),
        "selection exceeds estimated token budget: estimated=5193, limit=100. Narrow the target set or shorten the note."
    );
}

#[test]
fn normalization_states_cover_structured_partial_raw_only() {
    let structured = "SUMMARY:\n- ok\nFINDINGS:\n- summary: Missing timeout\nseverity: high\nconfidence: high\nlocation: src/main.rs\nevidence: client.call()\nwhy: can hang\naction: add timeout\n";
    let partial = "SUMMARY:\n- ok\nFINDINGS:\n- summary: Weak guard\nconfidence: medium\n";
    let raw_only = "This is free text with no findings section.";

    let (state_structured, findings_structured) =
        normalize_findings("s1", RevivaMode::Contract, "src/main.rs", structured);
    let (state_partial, findings_partial) =
        normalize_findings("s1", RevivaMode::Contract, "src/main.rs", partial);
    let (state_raw, findings_raw) =
        normalize_findings("s1", RevivaMode::Contract, "src/main.rs", raw_only);

    assert_eq!(state_structured.as_str(), "structured");
    assert_eq!(findings_structured.len(), 1);
    assert_eq!(state_partial.as_str(), "partial");
    assert_eq!(findings_partial.len(), 1);
    assert_eq!(state_raw.as_str(), "raw_only");
    assert_eq!(findings_raw.len(), 0);
}

#[test]
fn review_profile_toml_parses() {
    let profile = parse_review_profile_toml(
        r#"
name = "tracehound-launch"
goal = "Launch review for security-sensitive runtime"
global_rules = ["No code generation", "Mark uncertainty explicitly"]
focus = ["failure-semantics", "operator-trust"]
severity_scale = ["release-blocker", "pre-launch-fix", "post-launch-watch"]
confidence_scale = ["definite", "likely", "uncertain"]
risk_classes = ["correctness", "security", "operator-trust"]
"#,
    )
    .expect("valid profile toml");

    assert_eq!(profile.name, "tracehound-launch");
    assert_eq!(profile.severity_scale.len(), 3);
    assert_eq!(profile.risk_classes[0], "correctness");
}

#[test]
fn launch_labels_map_to_canonical_enums() {
    let output = "SUMMARY:\n- ok\nFINDINGS:\n- summary: Launch blocker\nseverity: release-blocker\nconfidence: definite\nrisk_class: operator-trust\nlocation: src/main.rs\nevidence: implicit default\nwhy: startup confusion\naction: make explicit\n";
    let (state, findings) =
        normalize_findings("s2", RevivaMode::LaunchReadiness, "src/main.rs", output);
    assert_eq!(state.as_str(), "structured");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity_origin.as_str(), "normalized");
    assert_eq!(findings[0].confidence.as_str(), "high");
    assert_eq!(findings[0].risk_class.as_deref(), Some("operator-trust"));
}

#[test]
fn raw_only_and_partial_reasons_are_reported() {
    let raw = normalize_findings_with_reasons("s3", RevivaMode::Contract, "src/a.rs", "");
    assert_eq!(raw.state.as_str(), "raw_only");
    assert!(raw.reason_tags.iter().any(|tag| tag == "empty_output"));
    assert!(raw
        .reason_tags
        .iter()
        .any(|tag| tag == "missing_findings_section"));

    let partial_output = "SUMMARY:\n- ok\nFINDINGS:\n- summary: Unknown labels\nseverity: release-x\nconfidence: maybe\n";
    let partial =
        normalize_findings_with_reasons("s4", RevivaMode::Contract, "src/b.rs", partial_output);
    assert_eq!(partial.state.as_str(), "partial");
    assert!(partial
        .reason_tags
        .iter()
        .any(|tag| tag == "invalid_severity_label"));
    assert!(partial
        .reason_tags
        .iter()
        .any(|tag| tag == "invalid_confidence_label"));
}

#[test]
fn prompt_wrapper_qwen_chatml_is_parseable_and_stable() {
    let wrapper = parse_prompt_wrapper("qwen-chatml").expect("valid wrapper");
    assert_eq!(wrapper, PromptWrapper::QwenChatMl);

    let wrapped = apply_prompt_wrapper("REVIVA REVIEW REQUEST", wrapper);
    assert!(wrapped.contains("<|im_start|>system"));
    assert!(wrapped.contains("<|im_start|>user"));
    assert!(wrapped.contains("REVIVA REVIEW REQUEST"));
    assert!(wrapped.contains("<|im_start|>assistant"));
}

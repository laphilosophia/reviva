use reviva::core::{BoundaryTarget, RevivaMode, RevivaTarget};
use reviva::prompts::{
    apply_prompt_wrapper, build_prompt, built_in_review_profile, default_review_profile,
    normalize_findings, normalize_findings_for_profile_with_reasons,
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

Findings policy:
- Prefer underclaiming to speculation.
- Report findings only when location and evidence are concrete in provided files.
- If evidence is weak, use the lowest confidence label and explain uncertainty in `why`.

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
  risk_class: <correctness|security|memory|performance|maintainability|operator-trust|public-surface>
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
fn docs_only_targets_add_documentation_policy_guardrail() {
    let profile =
        built_in_review_profile("launch-readiness").expect("launch-readiness profile must exist");
    let result = build_prompt(
        RevivaMode::LaunchReadiness,
        &profile,
        &RevivaTarget::Single("packages/cli/README.md".to_string()),
        &[PromptFile {
            path: "packages/cli/README.md".to_string(),
            content: "# docs".to_string(),
            estimated_tokens: 16,
            suspicion: None,
        }],
        None,
        &PromptBuildConfig::default(),
    )
    .expect("prompt should build");

    assert!(result.prompt.contains("Documentation-only policy:"));
    assert!(result
        .prompt
        .contains("do not infer runtime bugs that require source/runtime evidence"));
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

[limits]
max_findings = 5
max_output_tokens = 256
"#,
    )
    .expect("valid profile toml");

    assert_eq!(profile.name, "tracehound-launch");
    assert_eq!(profile.severity_scale.len(), 3);
    assert_eq!(profile.risk_classes[0], "correctness");
    assert_eq!(profile.limits.max_findings, Some(5));
    assert_eq!(profile.limits.max_output_tokens, Some(256));
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
fn launch_label_variants_are_accepted() {
    let profile =
        built_in_review_profile("launch-readiness").expect("launch-readiness profile must exist");
    let output = "SUMMARY:\n- ok\nFINDINGS:\n- summary: Release gate\nseverity: release blocker\nconfidence: definite.\nlocation: src/main.rs\nevidence: startup path\nwhy: startup can fail\naction: tighten default\n- summary: Needs prelaunch fix\nseverity: prelaunch-fix\nconfidence: likely\nlocation: src/main.rs\nevidence: fallback branch\nwhy: operator confusion\naction: make explicit\n- summary: Watch after launch\nseverity: post-launch-monitor\nconfidence: uncertain\nlocation: src/main.rs\nevidence: dynamic branch\nwhy: low confidence drift\naction: add observability\n- summary: Another blocker\nseverity: \"ship-blocker\"\nconfidence: confirmed\nlocation: src/main.rs\nevidence: implicit global\nwhy: hidden coupling\naction: isolate state\n";
    let report = normalize_findings_for_profile_with_reasons(
        &profile,
        "s8",
        RevivaMode::LaunchReadiness,
        "src/main.rs",
        output,
    );
    assert_eq!(report.state.as_str(), "structured");
    assert_eq!(report.findings.len(), 4);
    assert!(report.reason_tags.is_empty());
    assert_eq!(
        report.findings[0].severity.as_ref().map(|v| v.as_str()),
        Some("critical")
    );
    assert_eq!(report.findings[0].confidence.as_str(), "high");
    assert_eq!(
        report.findings[1].severity.as_ref().map(|v| v.as_str()),
        Some("high")
    );
    assert_eq!(report.findings[1].confidence.as_str(), "medium");
    assert_eq!(
        report.findings[2].severity.as_ref().map(|v| v.as_str()),
        Some("medium")
    );
    assert_eq!(report.findings[2].confidence.as_str(), "low");
    assert_eq!(
        report.findings[3].severity.as_ref().map(|v| v.as_str()),
        Some("critical")
    );
    assert_eq!(report.findings[3].confidence.as_str(), "high");
    assert!(report
        .findings
        .iter()
        .all(|finding| finding.severity_origin.as_str() == "normalized"));
}

#[test]
fn invalid_launch_labels_are_tagged() {
    let profile =
        built_in_review_profile("launch-readiness").expect("launch-readiness profile must exist");
    let output = "SUMMARY:\n- ok\nFINDINGS:\n- summary: Unsupported labels\nseverity: release-gate\nconfidence: maybe-sure\nlocation: src/main.rs\nevidence: startup\nwhy: unclear\naction: clarify\n";
    let report = normalize_findings_for_profile_with_reasons(
        &profile,
        "s9",
        RevivaMode::LaunchReadiness,
        "src/main.rs",
        output,
    );
    assert_eq!(report.state.as_str(), "partial");
    assert!(report
        .reason_tags
        .iter()
        .any(|tag| tag == "invalid_severity_label"));
    assert!(report
        .reason_tags
        .iter()
        .any(|tag| tag == "invalid_confidence_label"));
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
fn normalization_truncates_findings_when_profile_has_limit() {
    let mut profile = default_review_profile();
    profile.limits.max_findings = Some(1);
    let output = "SUMMARY:\n- ok\nFINDINGS:\n- summary: First\nseverity: high\nconfidence: high\nlocation: src/main.rs\nevidence: a\nwhy: a\naction: a\n- summary: Second\nseverity: medium\nconfidence: medium\nlocation: src/main.rs\nevidence: b\nwhy: b\naction: b\n";
    let report = normalize_findings_for_profile_with_reasons(
        &profile,
        "s5",
        RevivaMode::Contract,
        "src/main.rs",
        output,
    );
    assert_eq!(report.findings.len(), 1);
    assert!(report
        .reason_tags
        .iter()
        .any(|tag| tag == "max_findings_truncated"));
}

#[test]
fn markdown_numbered_findings_are_parsed() {
    let output = r#"SUMMARY:
- Queue review

FINDINGS:
1. **Potential contract mismatch on overflow semantics**
   - **severity: high**
   - **confidence: medium**
   - **risk_class: correctness**
   - **location:** `lane-queue.ts:handleOverflow`
   - **evidence:** `case 'drop_lowest'`
   - **why:** callers cannot predict eviction lane deterministically
   - **action:** document or enforce deterministic overflow contract
"#;
    let (state, findings) =
        normalize_findings("s6", RevivaMode::Contract, "src/lane-queue.ts", output);
    assert_eq!(state.as_str(), "structured");
    assert_eq!(findings.len(), 1);
    assert!(findings[0].summary.contains("Potential contract mismatch"));
    assert_eq!(
        findings[0].location_hint.as_deref(),
        Some("lane-queue.ts:handleOverflow")
    );
}

#[test]
fn finding_without_summary_but_with_fields_is_retained() {
    let output = r#"SUMMARY:
- Queue review
FINDINGS:
1. **severity: medium**
   - **confidence: high**
   - **risk_class: correctness**
   - **location:** `lane-queue.ts:enqueue`
   - **evidence:** `if (lane.length >= laneConfig.maxSize)`
   - **why:** overflow behavior is ambiguous
   - **action:** make overflow semantics explicit
"#;
    let (state, findings) =
        normalize_findings("s7", RevivaMode::Contract, "src/lane-queue.ts", output);
    assert_eq!(state.as_str(), "structured");
    assert_eq!(findings.len(), 1);
    assert!(findings[0]
        .summary
        .contains("overflow behavior is ambiguous"));
}

#[test]
fn prompt_wrapper_chatml_is_parseable_and_stable() {
    let wrapper = parse_prompt_wrapper("chatml").expect("valid wrapper");
    assert_eq!(wrapper, PromptWrapper::ChatMl);

    let alias_removed = parse_prompt_wrapper("chatml-v2").expect_err("unsupported alias removed");
    assert!(alias_removed
        .to_string()
        .contains("supported: plain, chatml"));

    let wrapped = apply_prompt_wrapper("REVIVA REVIEW REQUEST", wrapper);
    assert!(wrapped.contains("<|im_start|>system"));
    assert!(wrapped.contains("<|im_start|>user"));
    assert!(wrapped.contains("REVIVA REVIEW REQUEST"));
    assert!(wrapped.contains("<|im_start|>assistant"));
}

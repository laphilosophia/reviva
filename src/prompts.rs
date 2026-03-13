use crate::core;
use core::{
    Confidence, Finding, NormalizationState, RevivaMode, RevivaTarget, Severity, SeverityOrigin,
};
use serde::Deserialize;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewProfileSpec {
    pub name: String,
    pub goal: String,
    pub global_rules: Vec<String>,
    pub focus: Vec<String>,
    pub severity_scale: Vec<String>,
    pub confidence_scale: Vec<String>,
    pub risk_classes: Vec<String>,
    pub limits: ReviewProfileLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewProfileLimits {
    pub max_findings: Option<usize>,
    pub max_output_tokens: Option<u32>,
}

impl ReviewProfileSpec {
    pub fn canonical_text(&self) -> String {
        format!(
            "name={}\ngoal={}\nrules={}\nfocus={}\nseverity={}\nconfidence={}\nrisk={}\nmax_findings={}\nmax_output_tokens={}",
            self.name,
            self.goal,
            self.global_rules.join("|"),
            self.focus.join("|"),
            self.severity_scale.join("|"),
            self.confidence_scale.join("|"),
            self.risk_classes.join("|"),
            self.limits
                .max_findings
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.limits
                .max_output_tokens
                .map(|value| value.to_string())
                .unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewProfileError {
    UnsupportedBuiltInProfile(String),
    InvalidToml(String),
    InvalidProfile(String),
}

impl fmt::Display for ReviewProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedBuiltInProfile(name) => write!(
                f,
                "unsupported review profile: {name}. available profiles: {}",
                review_profile_names().join(", ")
            ),
            Self::InvalidToml(message) => write!(f, "invalid profile TOML: {message}"),
            Self::InvalidProfile(message) => write!(f, "invalid review profile: {message}"),
        }
    }
}

impl std::error::Error for ReviewProfileError {}

pub const fn review_profile_names() -> &'static [&'static str] {
    &["default", "launch-readiness", "strict"]
}

pub fn default_review_profile() -> ReviewProfileSpec {
    built_in_review_profile("default").expect("default profile must exist")
}

pub fn built_in_review_profile(name: &str) -> Option<ReviewProfileSpec> {
    match name.trim().to_ascii_lowercase().as_str() {
        "default" => Some(ReviewProfileSpec {
            name: "default".to_string(),
            goal: "Focused semantic review with explicit constraints.".to_string(),
            global_rules: vec![
                "Do not invent context outside provided files.".to_string(),
                "Do not ask follow-up questions.".to_string(),
                "Do not propose autonomous workflows.".to_string(),
            ],
            focus: vec![
                "correctness".to_string(),
                "security".to_string(),
                "boundedness".to_string(),
                "failure-semantics".to_string(),
                "maintainability".to_string(),
            ],
            severity_scale: vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "critical".to_string(),
                "unknown".to_string(),
            ],
            confidence_scale: vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "unknown".to_string(),
            ],
            risk_classes: vec![
                "correctness".to_string(),
                "security".to_string(),
                "memory".to_string(),
                "performance".to_string(),
                "maintainability".to_string(),
                "operator-trust".to_string(),
                "public-surface".to_string(),
            ],
            limits: ReviewProfileLimits::default(),
        }),
        "launch-readiness" => Some(ReviewProfileSpec {
            name: "launch-readiness".to_string(),
            goal: "Launch-readiness semantic review with production-risk prioritization."
                .to_string(),
            global_rules: vec![
                "Do not write code or patches.".to_string(),
                "Do not praise code quality or give style advice.".to_string(),
                "Prefer underclaiming to speculation and mark uncertainty.".to_string(),
                "Focus on correctness, security, boundedness, failure semantics, maintainability, operator trust.".to_string(),
            ],
            focus: vec![
                "launch-readiness".to_string(),
                "failure-semantics".to_string(),
                "operator-trust".to_string(),
            ],
            severity_scale: vec![
                "release-blocker".to_string(),
                "pre-launch-fix".to_string(),
                "post-launch-watch".to_string(),
            ],
            confidence_scale: vec![
                "definite".to_string(),
                "likely".to_string(),
                "uncertain".to_string(),
            ],
            risk_classes: vec![
                "correctness".to_string(),
                "security".to_string(),
                "memory".to_string(),
                "performance".to_string(),
                "maintainability".to_string(),
                "operator-trust".to_string(),
                "public-surface".to_string(),
            ],
            limits: ReviewProfileLimits::default(),
        }),
        "strict" => Some(ReviewProfileSpec {
            name: "strict".to_string(),
            goal: "Conservative review: underclaim, cite evidence, avoid speculation.".to_string(),
            global_rules: vec![
                "Return only concrete findings tied to visible evidence.".to_string(),
                "If evidence is weak, mark uncertainty explicitly.".to_string(),
                "Avoid broad claims and avoid generic recommendations.".to_string(),
            ],
            focus: vec![
                "correctness".to_string(),
                "boundary".to_string(),
                "failure-semantics".to_string(),
            ],
            severity_scale: vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "critical".to_string(),
                "unknown".to_string(),
            ],
            confidence_scale: vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "unknown".to_string(),
            ],
            risk_classes: vec![
                "correctness".to_string(),
                "security".to_string(),
                "maintainability".to_string(),
                "operator-trust".to_string(),
            ],
            limits: ReviewProfileLimits::default(),
        }),
        _ => None,
    }
}

pub fn resolve_built_in_review_profile(
    name: &str,
) -> Result<ReviewProfileSpec, ReviewProfileError> {
    built_in_review_profile(name)
        .ok_or_else(|| ReviewProfileError::UnsupportedBuiltInProfile(name.to_string()))
}

#[derive(Debug, Deserialize)]
struct ReviewProfileToml {
    name: Option<String>,
    goal: Option<String>,
    global_rules: Option<Vec<String>>,
    focus: Option<Vec<String>>,
    severity_scale: Option<Vec<String>>,
    confidence_scale: Option<Vec<String>>,
    risk_classes: Option<Vec<String>>,
    limits: Option<ReviewProfileLimitsToml>,
}

#[derive(Debug, Deserialize)]
struct ReviewProfileLimitsToml {
    max_findings: Option<usize>,
    max_output_tokens: Option<u32>,
}

pub fn parse_review_profile_toml(content: &str) -> Result<ReviewProfileSpec, ReviewProfileError> {
    let parsed: ReviewProfileToml = toml::from_str(content)
        .map_err(|error| ReviewProfileError::InvalidToml(error.to_string()))?;
    let name = parsed
        .name
        .unwrap_or_else(|| "custom".to_string())
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(ReviewProfileError::InvalidProfile(
            "name cannot be empty".to_string(),
        ));
    }

    let goal = parsed
        .goal
        .unwrap_or_else(|| "Focused semantic review with explicit constraints.".to_string())
        .trim()
        .to_string();
    if goal.is_empty() {
        return Err(ReviewProfileError::InvalidProfile(
            "goal cannot be empty".to_string(),
        ));
    }

    Ok(ReviewProfileSpec {
        name,
        goal,
        global_rules: parsed.global_rules.unwrap_or_default(),
        focus: parsed.focus.unwrap_or_default(),
        severity_scale: parsed.severity_scale.unwrap_or_default(),
        confidence_scale: parsed.confidence_scale.unwrap_or_default(),
        risk_classes: parsed.risk_classes.unwrap_or_default(),
        limits: parsed
            .limits
            .map(|limits| ReviewProfileLimits {
                max_findings: limits.max_findings,
                max_output_tokens: limits.max_output_tokens,
            })
            .unwrap_or_default(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptWrapper {
    Plain,
    ChatMl,
}

impl PromptWrapper {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::ChatMl => "chatml",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePromptWrapperError {
    pub value: String,
}

impl fmt::Display for ParsePromptWrapperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported prompt wrapper: {} (supported: plain, chatml)",
            self.value
        )
    }
}

impl std::error::Error for ParsePromptWrapperError {}

pub fn parse_prompt_wrapper(value: &str) -> Result<PromptWrapper, ParsePromptWrapperError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "plain" => Ok(PromptWrapper::Plain),
        "chatml" => Ok(PromptWrapper::ChatMl),
        _ => Err(ParsePromptWrapperError {
            value: value.to_string(),
        }),
    }
}

pub fn apply_prompt_wrapper(prompt: &str, wrapper: PromptWrapper) -> String {
    match wrapper {
        PromptWrapper::Plain => prompt.to_string(),
        PromptWrapper::ChatMl => format!(
            "<|im_start|>system\nYou are a constrained code reviewer. Follow the user's output contract exactly.\n<|im_end|>\n<|im_start|>user\n{prompt}\n<|im_end|>\n<|im_start|>assistant\n"
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptFile {
    pub path: String,
    pub content: String,
    pub estimated_tokens: usize,
    pub suspicion: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromptBuildConfig {
    pub estimated_prompt_tokens: usize,
}

impl Default for PromptBuildConfig {
    fn default() -> Self {
        Self {
            estimated_prompt_tokens: 16_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBuildResult {
    pub prompt: String,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptError {
    EstimatedBudgetExceeded {
        estimated_tokens: usize,
        max_tokens: usize,
    },
    BoundaryTargetMismatch,
}

impl fmt::Display for PromptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EstimatedBudgetExceeded {
                estimated_tokens,
                max_tokens,
            } => write!(
                f,
                "selection exceeds estimated token budget: estimated={estimated_tokens}, limit={max_tokens}. Narrow the target set or shorten the note."
            ),
            Self::BoundaryTargetMismatch => write!(
                f,
                "boundary review requires deterministic left/right target files in that order"
            ),
        }
    }
}

impl std::error::Error for PromptError {}

fn mode_focus(mode: RevivaMode) -> &'static str {
    match mode {
        RevivaMode::Contract => "Inspect contract clarity, preconditions, postconditions, and hidden invariants.",
        RevivaMode::Boundary => {
            "Inspect trust boundaries, adapter leakage, normalization drift, and ownership ambiguity."
        }
        RevivaMode::Boundedness => "Inspect unbounded loops, queues, growth vectors, and resource limits.",
        RevivaMode::FailureSemantics => {
            "Inspect error semantics, retries, propagation, and recovery behavior."
        }
        RevivaMode::PerformanceRisk => {
            "Inspect performance hotspots, avoidable work, and complexity risks."
        }
        RevivaMode::MemoryRisk => "Inspect allocations, retention risks, and lifecycle mismatches.",
        RevivaMode::OperatorCorrectness => {
            "Inspect runtime operations, observability correctness, and safety controls."
        }
        RevivaMode::LaunchReadiness => {
            "Inspect release risk, failure blast radius, and production readiness gaps."
        }
        RevivaMode::Maintainability => {
            "Inspect coupling, readability, change risk, and testability concerns."
        }
    }
}

pub fn build_prompt(
    mode: RevivaMode,
    profile: &ReviewProfileSpec,
    target: &RevivaTarget,
    files: &[PromptFile],
    note: Option<&str>,
    config: &PromptBuildConfig,
) -> Result<PromptBuildResult, PromptError> {
    if let RevivaTarget::Boundary(boundary) = target {
        let valid_order =
            files.len() == 2 && files[0].path == boundary.left && files[1].path == boundary.right;
        if !valid_order {
            return Err(PromptError::BoundaryTargetMismatch);
        }
    }

    let estimated_tokens = files
        .iter()
        .map(|file| file.estimated_tokens)
        .sum::<usize>()
        + estimate_tokens(note.unwrap_or(""))
        + 192;

    if estimated_tokens > config.estimated_prompt_tokens {
        return Err(PromptError::EstimatedBudgetExceeded {
            estimated_tokens,
            max_tokens: config.estimated_prompt_tokens,
        });
    }

    let mut prompt = String::new();
    prompt.push_str("REVIVA REVIEW REQUEST\n");
    prompt.push_str("=====================\n");
    prompt.push_str("You are a constrained code reviewer.\n");
    for rule in &profile.global_rules {
        prompt.push_str(rule);
        prompt.push('\n');
    }
    prompt.push('\n');

    prompt.push_str(&format!("Mode: {}\n", mode.as_str()));
    prompt.push_str(&format!("Profile: {}\n", profile.name));
    prompt.push_str(&format!("Profile goal: {}\n", profile.goal));
    if !profile.focus.is_empty() {
        prompt.push_str(&format!("Profile focus: {}\n", profile.focus.join(", ")));
    }
    if !profile.severity_scale.is_empty() {
        prompt.push_str(&format!(
            "Profile severity scale: {}\n",
            profile.severity_scale.join(", ")
        ));
    }
    if !profile.confidence_scale.is_empty() {
        prompt.push_str(&format!(
            "Profile confidence scale: {}\n",
            profile.confidence_scale.join(", ")
        ));
    }
    if !profile.risk_classes.is_empty() {
        prompt.push_str(&format!(
            "Profile risk classes: {}\n",
            profile.risk_classes.join(", ")
        ));
    }
    if let Some(max_findings) = profile.limits.max_findings {
        prompt.push_str(&format!("Profile limit max findings: {max_findings}\n"));
    }
    if let Some(max_output_tokens) = profile.limits.max_output_tokens {
        prompt.push_str(&format!(
            "Profile limit max output tokens: {max_output_tokens}\n"
        ));
    }
    prompt.push_str(&format!("Focus: {}\n", mode_focus(mode)));
    prompt.push_str(&format!(
        "Target: {}\n",
        match target {
            RevivaTarget::Single(path) => format!("single:{path}"),
            RevivaTarget::Set(paths) => format!("set:{} files", paths.len()),
            RevivaTarget::Boundary(boundary) => {
                format!("boundary:left={} right={}", boundary.left, boundary.right)
            }
        }
    ));
    prompt.push_str("Estimated token budget is heuristic, not exact.\n\n");
    prompt.push_str("Findings policy:\n");
    prompt.push_str("- Prefer underclaiming to speculation.\n");
    prompt.push_str(
        "- Report findings only when location and evidence are concrete in provided files.\n",
    );
    prompt.push_str("- If evidence is weak, use the lowest confidence label and explain uncertainty in `why`.\n\n");
    if is_docs_only_selection(files) {
        prompt.push_str("Documentation-only policy:\n");
        prompt.push_str(
            "- Documentation-only target detected; do not infer runtime bugs that require source/runtime evidence.\n",
        );
        prompt.push_str(
            "- Use security/memory/performance risk only when text explicitly contains unsafe instruction, contradictory contract, or secret-handling issue.\n",
        );
        prompt.push_str(
            "- If issue is mostly missing detail or clarity, prefer maintainability/operator-trust with low confidence.\n\n",
        );
    }

    if let Some(note) = note {
        prompt.push_str("User note:\n");
        prompt.push_str(note);
        prompt.push_str("\n\n");
    }

    prompt.push_str("Selected files:\n");
    for file in files {
        let suspicion = file
            .suspicion
            .as_deref()
            .map(|value| format!(" suspicion={value}"))
            .unwrap_or_default();
        prompt.push_str(&format!(
            "- {} (estimated_tokens={}{} )\n",
            file.path, file.estimated_tokens, suspicion
        ));
    }

    prompt.push_str("\nCode:\n");
    for file in files {
        prompt.push_str(&format!("\n--- BEGIN FILE {} ---\n", file.path));
        prompt.push_str(&file.content);
        if !file.content.ends_with('\n') {
            prompt.push('\n');
        }
        prompt.push_str(&format!("--- END FILE {} ---\n", file.path));
    }

    let severity_contract =
        format_scale(&profile.severity_scale, "low|medium|high|critical|unknown");
    let confidence_contract = format_scale(&profile.confidence_scale, "low|medium|high|unknown");
    let risk_contract = format_scale(
        &profile.risk_classes,
        "correctness|security|memory|performance|maintainability|operator-trust|public-surface|unknown",
    );
    prompt.push_str("\nOutput contract (plain text):\n");
    prompt.push_str("SUMMARY:\n- one short summary line\nFINDINGS:\n");
    if let Some(max_findings) = profile.limits.max_findings {
        prompt.push_str(&format!("- return at most {max_findings} findings\n"));
    }
    prompt.push_str(
        "- summary: <text>\n\
  severity: <",
    );
    prompt.push_str(&severity_contract);
    prompt.push_str(">\n  confidence: <");
    prompt.push_str(&confidence_contract);
    prompt.push_str(">\n  risk_class: <");
    prompt.push_str(&risk_contract);
    prompt.push_str(
        ">\n\
  location: <path or symbol>\n\
  evidence: <short quote or hint>\n\
  why: <impact>\n\
  action: <fix guidance>\n",
    );

    Ok(PromptBuildResult {
        prompt,
        estimated_tokens,
    })
}

fn is_docs_only_selection(files: &[PromptFile]) -> bool {
    !files.is_empty() && files.iter().all(|file| is_docs_like_path(&file.path))
}

fn is_docs_like_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md")
        || lower.ends_with(".mdx")
        || lower.ends_with(".rst")
        || lower.ends_with(".adoc")
        || lower.ends_with(".txt")
        || lower.ends_with(".rdoc")
        || lower.ends_with("readme")
        || lower.ends_with("changelog")
        || lower.ends_with("license")
        || lower.contains("/docs/")
        || lower.contains("\\docs\\")
}

pub fn normalize_findings(
    session_id: &str,
    mode: RevivaMode,
    target: &str,
    raw_output: &str,
) -> (NormalizationState, Vec<Finding>) {
    let report = normalize_findings_with_reasons(session_id, mode, target, raw_output);
    (report.state, report.findings)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizationReport {
    pub state: NormalizationState,
    pub findings: Vec<Finding>,
    pub reason_tags: Vec<String>,
}

pub fn normalize_findings_with_reasons(
    session_id: &str,
    mode: RevivaMode,
    target: &str,
    raw_output: &str,
) -> NormalizationReport {
    normalize_findings_for_profile_with_reasons(
        &default_review_profile(),
        session_id,
        mode,
        target,
        raw_output,
    )
}

pub fn normalize_findings_for_profile_with_reasons(
    profile: &ReviewProfileSpec,
    session_id: &str,
    mode: RevivaMode,
    target: &str,
    raw_output: &str,
) -> NormalizationReport {
    let mut findings = Vec::new();
    let mut current_lines = Vec::<String>::new();
    let mut in_findings = false;
    let mut saw_findings_header = false;
    let mut dropped_blocks = 0_usize;
    let mut findings_payload_lines = 0_usize;
    let mut invalid_severity_label = false;
    let mut invalid_confidence_label = false;
    let mut saw_severity_label = false;
    let mut saw_confidence_label = false;

    for line in raw_output.lines() {
        let trimmed = line.trim();
        if is_findings_header(trimmed) {
            in_findings = true;
            saw_findings_header = true;
            continue;
        }

        if !in_findings {
            continue;
        }

        if let Some(value) = extract_field_value(trimmed, "severity") {
            saw_severity_label = true;
            if parse_model_severity(value.trim(), profile).is_none()
                && !is_explicit_unknown_severity(&value)
            {
                invalid_severity_label = true;
            }
        }
        if let Some(value) = extract_field_value(trimmed, "confidence") {
            saw_confidence_label = true;
            let normalized = parse_model_confidence(&value, profile);
            if normalized == Confidence::Unknown && !is_explicit_unknown_confidence(&value) {
                invalid_confidence_label = true;
            }
        }

        if is_finding_start_line(trimmed) && !current_lines.is_empty() {
            if let Some(finding) = parse_finding_block(
                profile,
                session_id,
                mode,
                target,
                findings.len() + 1,
                &current_lines,
            ) {
                findings.push(finding);
            } else {
                dropped_blocks += 1;
            }
            current_lines.clear();
        }

        if is_finding_start_line(trimmed)
            || extract_field_value(trimmed, "severity").is_some()
            || extract_field_value(trimmed, "confidence").is_some()
            || extract_field_value(trimmed, "risk_class").is_some()
            || extract_field_value(trimmed, "location").is_some()
            || extract_field_value(trimmed, "evidence").is_some()
            || extract_field_value(trimmed, "why").is_some()
            || extract_field_value(trimmed, "action").is_some()
        {
            findings_payload_lines += 1;
            current_lines.push(trimmed.to_string());
        }
    }

    if !current_lines.is_empty() {
        if let Some(finding) = parse_finding_block(
            profile,
            session_id,
            mode,
            target,
            findings.len() + 1,
            &current_lines,
        ) {
            findings.push(finding);
        } else {
            dropped_blocks += 1;
        }
    }

    let mut reason_tags = Vec::new();
    if findings.is_empty() {
        if raw_output.trim().is_empty() {
            reason_tags.push("empty_output".to_string());
        }
        if !saw_findings_header {
            reason_tags.push("missing_findings_section".to_string());
        } else if findings_payload_lines == 0 {
            reason_tags.push("empty_findings_block".to_string());
        } else {
            reason_tags.push("findings_unparseable".to_string());
        }
        if dropped_blocks > 0 {
            reason_tags.push("dropped_finding_blocks".to_string());
        }
        if invalid_severity_label {
            reason_tags.push("invalid_severity_label".to_string());
        }
        if invalid_confidence_label {
            reason_tags.push("invalid_confidence_label".to_string());
        }
        return NormalizationReport {
            state: NormalizationState::RawOnly,
            findings,
            reason_tags,
        };
    }

    let structured = findings.iter().all(|finding| {
        finding.severity.is_some()
            && finding.confidence != Confidence::Unknown
            && !finding.summary.trim().is_empty()
    });
    let state = if structured {
        NormalizationState::Structured
    } else {
        NormalizationState::Partial
    };

    if state == NormalizationState::Partial {
        if findings.iter().any(|finding| finding.severity.is_none()) {
            if saw_severity_label {
                if invalid_severity_label {
                    reason_tags.push("invalid_severity_label".to_string());
                } else {
                    reason_tags.push("unmapped_severity_label".to_string());
                }
            } else {
                reason_tags.push("missing_severity_label".to_string());
            }
        }
        if findings
            .iter()
            .any(|finding| finding.confidence == Confidence::Unknown)
        {
            if saw_confidence_label {
                if invalid_confidence_label {
                    reason_tags.push("invalid_confidence_label".to_string());
                } else {
                    reason_tags.push("unmapped_confidence_label".to_string());
                }
            } else {
                reason_tags.push("missing_confidence_label".to_string());
            }
        }
        if dropped_blocks > 0 {
            reason_tags.push("dropped_finding_blocks".to_string());
        }
    }

    for finding in &mut findings {
        finding.normalization_state = state;
    }
    if let Some(max_findings) = profile.limits.max_findings {
        if findings.len() > max_findings {
            findings.truncate(max_findings);
            reason_tags.push("max_findings_truncated".to_string());
        }
    }
    NormalizationReport {
        state,
        findings,
        reason_tags,
    }
}

fn parse_finding_block(
    profile: &ReviewProfileSpec,
    session_id: &str,
    mode: RevivaMode,
    target: &str,
    index: usize,
    lines: &[String],
) -> Option<Finding> {
    let mut summary = None::<String>;
    let mut severity = None::<Severity>;
    let mut severity_origin = SeverityOrigin::Unrated;
    let mut confidence = Confidence::Unknown;
    let mut risk_class = None::<String>;
    let mut location = None::<String>;
    let mut evidence = None::<String>;
    let mut why = None::<String>;
    let mut action = None::<String>;
    let mut raw_labels = Vec::new();

    for line in lines {
        if let Some(value) = extract_field_value(line, "summary") {
            summary = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = extract_field_value(line, "severity") {
            let label = value.trim().to_ascii_lowercase();
            raw_labels.push(label.clone());
            if let Some((parsed, origin)) = parse_model_severity(&label, profile) {
                severity_origin = origin;
                severity = Some(parsed);
            }
            continue;
        }
        if let Some(value) = extract_field_value(line, "confidence") {
            confidence = parse_model_confidence(&value, profile);
            continue;
        }
        if let Some(value) = extract_field_value(line, "risk_class") {
            risk_class = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = extract_field_value(line, "location") {
            location = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = extract_field_value(line, "evidence") {
            evidence = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = extract_field_value(line, "why") {
            why = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = extract_field_value(line, "action") {
            action = Some(value.trim().to_string());
            continue;
        }
        if summary.is_none() {
            let candidate = normalize_findings_line(line);
            if !candidate.is_empty() && !looks_like_field_line(&candidate) {
                summary = Some(candidate);
            }
        }
    }

    let summary = match summary {
        Some(value) => value,
        None => {
            if severity.is_some()
                || confidence != Confidence::Unknown
                || risk_class.is_some()
                || location.is_some()
                || evidence.is_some()
                || why.is_some()
                || action.is_some()
            {
                why.clone()
                    .or_else(|| evidence.clone())
                    .or_else(|| location.clone())
                    .unwrap_or_else(|| format!("Finding {index}"))
            } else {
                return None;
            }
        }
    };
    Some(Finding {
        id: format!("{session_id}-{index}"),
        session_id: session_id.to_string(),
        review_mode: mode,
        target: target.to_string(),
        summary,
        why_it_matters: why,
        severity,
        severity_origin,
        confidence,
        risk_class,
        action,
        status: None,
        location_hint: location,
        evidence_text: evidence,
        raw_labels,
        normalization_state: NormalizationState::RawOnly,
    })
}

fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() / 4).saturating_add(1)
}

fn parse_model_severity(
    label: &str,
    profile: &ReviewProfileSpec,
) -> Option<(Severity, SeverityOrigin)> {
    let normalized = normalize_model_label(label);
    if is_explicit_unknown_severity(&normalized) {
        return None;
    }
    match normalized.as_str() {
        "low" => Some((Severity::Low, SeverityOrigin::ModelLabeled)),
        "medium" => Some((Severity::Medium, SeverityOrigin::ModelLabeled)),
        "high" => Some((Severity::High, SeverityOrigin::ModelLabeled)),
        "critical" => Some((Severity::Critical, SeverityOrigin::ModelLabeled)),
        "release-blocker"
        | "blocker"
        | "launch-blocker"
        | "ship-blocker"
        | "release-critical"
        | "must-fix-now"
        | "must-fix-immediately" => Some((Severity::Critical, SeverityOrigin::Normalized)),
        "pre-launch-fix"
        | "prelaunch-fix"
        | "pre-launch"
        | "must-fix"
        | "needs-fix-before-launch"
        | "launch-fix" => Some((Severity::High, SeverityOrigin::Normalized)),
        "post-launch-watch" | "postlaunch-watch" | "post-launch-monitor" | "watch" | "monitor" => {
            Some((Severity::Medium, SeverityOrigin::Normalized))
        }
        _ => profile
            .severity_scale
            .iter()
            .position(|candidate| normalize_label(candidate) == normalized)
            .map(|index| {
                (
                    severity_from_scale_index(index, profile.severity_scale.len()),
                    SeverityOrigin::Normalized,
                )
            }),
    }
}

fn parse_model_confidence(value: &str, profile: &ReviewProfileSpec) -> Confidence {
    let normalized = normalize_model_label(value);
    match normalized.as_str() {
        "low" => Confidence::Low,
        "medium" => Confidence::Medium,
        "high" => Confidence::High,
        "definite" | "certain" | "confirmed" => Confidence::High,
        "likely" | "probable" | "plausible" => Confidence::Medium,
        "uncertain" | "unsure" | "tentative" | "possible" | "speculative" => Confidence::Low,
        "unknown" | "unrated" | "na" | "n/a" => Confidence::Unknown,
        _ => profile
            .confidence_scale
            .iter()
            .position(|candidate| normalize_label(candidate) == normalized)
            .map(confidence_from_scale_index)
            .unwrap_or(Confidence::Unknown),
    }
}

fn is_findings_header(line: &str) -> bool {
    normalize_findings_line(line).eq_ignore_ascii_case("findings:")
}

fn is_finding_start_line(line: &str) -> bool {
    let normalized = normalize_findings_line(line);
    if normalized.is_empty() {
        return false;
    }
    if normalized.to_ascii_lowercase().starts_with("summary:") {
        return true;
    }
    is_numbered_line(line) && !looks_like_field_line(&normalized)
}

fn is_numbered_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let bytes = trimmed.as_bytes();
    let mut index = 0_usize;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    index > 0
        && index < bytes.len()
        && bytes[index] == b'.'
        && index + 1 < bytes.len()
        && bytes[index + 1].is_ascii_whitespace()
}

fn normalize_findings_line(line: &str) -> String {
    let mut value = line.trim();
    value = strip_list_prefix(value);
    let mut normalized = value.replace("**", "");
    normalized = normalized.trim().to_string();
    normalized = normalized.trim_matches('`').trim().to_string();
    normalized
}

fn strip_list_prefix(value: &str) -> &str {
    let mut current = value.trim_start();
    loop {
        if let Some(rest) = current.strip_prefix("- ") {
            current = rest.trim_start();
            continue;
        }
        if let Some(rest) = current.strip_prefix("* ") {
            current = rest.trim_start();
            continue;
        }
        let bytes = current.as_bytes();
        let mut index = 0_usize;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index > 0
            && index < bytes.len()
            && bytes[index] == b'.'
            && index + 1 < bytes.len()
            && bytes[index + 1].is_ascii_whitespace()
        {
            current = current[index + 1..].trim_start();
            continue;
        }
        break;
    }
    current
}

fn extract_field_value(line: &str, key: &str) -> Option<String> {
    let normalized = normalize_findings_line(line);
    let lowered = normalized.to_ascii_lowercase();
    let key_prefix = format!("{key}:");
    if !lowered.starts_with(&key_prefix) {
        return None;
    }
    let value = normalized[key_prefix.len()..]
        .trim()
        .trim_matches('`')
        .trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

fn looks_like_field_line(normalized_line: &str) -> bool {
    let lowered = normalized_line.to_ascii_lowercase();
    lowered.starts_with("summary:")
        || lowered.starts_with("severity:")
        || lowered.starts_with("confidence:")
        || lowered.starts_with("risk_class:")
        || lowered.starts_with("location:")
        || lowered.starts_with("evidence:")
        || lowered.starts_with("why:")
        || lowered.starts_with("action:")
}

fn is_explicit_unknown_confidence(value: &str) -> bool {
    matches!(
        normalize_model_label(value).as_str(),
        "unknown" | "unrated" | "na" | "n/a"
    )
}

fn is_explicit_unknown_severity(value: &str) -> bool {
    matches!(
        normalize_model_label(value).as_str(),
        "unknown" | "unrated" | "na" | "n/a"
    )
}

fn format_scale(scale: &[String], fallback: &str) -> String {
    if scale.is_empty() {
        fallback.to_string()
    } else {
        scale.join("|")
    }
}

fn normalize_label(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', ' '], "-")
}

fn normalize_model_label(value: &str) -> String {
    let mut normalized = normalize_label(value);
    if let Some((head, _)) = normalized.split_once('(') {
        normalized = head.trim().to_string();
    }
    while normalized.contains("--") {
        normalized = normalized.replace("--", "-");
    }
    normalized
        .trim_matches(|character: char| {
            matches!(
                character,
                '-' | '.' | ',' | ';' | ':' | '!' | '?' | '"' | '\'' | '`'
            )
        })
        .to_string()
}

fn severity_from_scale_index(index: usize, scale_len: usize) -> Severity {
    if scale_len == 0 {
        return Severity::Medium;
    }
    if index == 0 {
        return Severity::Critical;
    }
    if index == 1 {
        return Severity::High;
    }
    if index == 2 {
        return Severity::Medium;
    }
    Severity::Low
}

fn confidence_from_scale_index(index: usize) -> Confidence {
    match index {
        0 => Confidence::High,
        1 => Confidence::Medium,
        _ => Confidence::Low,
    }
}

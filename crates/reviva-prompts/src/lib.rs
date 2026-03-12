use reviva_core::{
    Confidence, Finding, NormalizationState, RevivaMode, RevivaTarget, Severity, SeverityOrigin,
};
use std::fmt;

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
    prompt.push_str("Do not invent context outside provided files.\n");
    prompt.push_str("Do not ask follow-up questions.\n");
    prompt.push_str("Do not propose autonomous workflows.\n\n");

    prompt.push_str(&format!("Mode: {}\n", mode.as_str()));
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

    prompt.push_str(
        "\nOutput contract (plain text):\n\
SUMMARY:\n\
- one short summary line\n\
FINDINGS:\n\
- summary: <text>\n\
  severity: <low|medium|high|critical|unknown>\n\
  confidence: <low|medium|high|unknown>\n\
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

pub fn normalize_findings(
    session_id: &str,
    mode: RevivaMode,
    target: &str,
    raw_output: &str,
) -> (NormalizationState, Vec<Finding>) {
    let mut findings = Vec::new();
    let mut current_lines = Vec::<String>::new();
    let mut in_findings = false;

    for line in raw_output.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("FINDINGS:") {
            in_findings = true;
            continue;
        }

        if !in_findings {
            continue;
        }

        if trimmed.starts_with("- summary:") && !current_lines.is_empty() {
            if let Some(finding) =
                parse_finding_block(session_id, mode, target, findings.len() + 1, &current_lines)
            {
                findings.push(finding);
            }
            current_lines.clear();
        }

        if trimmed.starts_with('-')
            || trimmed.starts_with("severity:")
            || trimmed.starts_with("confidence:")
            || trimmed.starts_with("location:")
            || trimmed.starts_with("evidence:")
            || trimmed.starts_with("why:")
            || trimmed.starts_with("action:")
        {
            current_lines.push(trimmed.to_string());
        }
    }

    if !current_lines.is_empty() {
        if let Some(finding) =
            parse_finding_block(session_id, mode, target, findings.len() + 1, &current_lines)
        {
            findings.push(finding);
        }
    }

    if findings.is_empty() {
        return (NormalizationState::RawOnly, findings);
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

    for finding in &mut findings {
        finding.normalization_state = state;
    }
    (state, findings)
}

fn parse_finding_block(
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
    let mut location = None::<String>;
    let mut evidence = None::<String>;
    let mut why = None::<String>;
    let mut action = None::<String>;
    let mut raw_labels = Vec::new();

    for line in lines {
        if let Some(value) = line.strip_prefix("- summary:") {
            summary = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("severity:") {
            let label = value.trim().to_ascii_lowercase();
            raw_labels.push(label.clone());
            let parsed = match label.as_str() {
                "low" => Some(Severity::Low),
                "medium" => Some(Severity::Medium),
                "high" => Some(Severity::High),
                "critical" => Some(Severity::Critical),
                _ => None,
            };
            if parsed.is_some() {
                severity_origin = SeverityOrigin::ModelLabeled;
            }
            severity = parsed;
            continue;
        }
        if let Some(value) = line.strip_prefix("confidence:") {
            confidence = match value.trim().to_ascii_lowercase().as_str() {
                "low" => Confidence::Low,
                "medium" => Confidence::Medium,
                "high" => Confidence::High,
                _ => Confidence::Unknown,
            };
            continue;
        }
        if let Some(value) = line.strip_prefix("location:") {
            location = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("evidence:") {
            evidence = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("why:") {
            why = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("action:") {
            action = Some(value.trim().to_string());
            continue;
        }
    }

    let summary = summary?;
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
        risk_class: None,
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

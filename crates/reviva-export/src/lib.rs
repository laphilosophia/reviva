use reviva_core::{ResponseInterpretation, Session};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn export_session_markdown(session: &Session) -> String {
    let repeated_summaries = repeated_summary_clusters(&session.findings);
    let duplicate_summary_findings = repeated_summaries
        .iter()
        .map(|(_, count)| *count)
        .sum::<usize>();
    let incremental = extract_incremental_warnings(&session.warnings);

    let mut output = String::new();
    output.push_str("# Reviva Session Export\n\n");
    output.push_str(&format!("- Session ID: `{}`\n", session.id));
    output.push_str(&format!("- Created At: `{}`\n", session.created_at));
    output.push_str(&format!("- Mode: `{}`\n", session.review_mode.as_str()));
    output.push_str(&format!("- Profile: `{}`\n", session.profile.name));
    output.push_str(&format!("- Profile Source: `{}`\n", session.profile.source));
    if let Some(path) = &session.profile.path {
        output.push_str(&format!("- Profile Path: `{}`\n", path));
    }
    output.push_str(&format!("- Profile Hash: `{}`\n", session.profile.hash));
    output.push_str(&format!(
        "- Target: `{}`\n\n",
        format_target(&session.selected_target)
    ));

    output.push_str("## Prompt Metadata\n\n");
    output.push_str(&format!(
        "- Prompt Preview Equals Sent: `{}`\n",
        session.prompt_preview == session.prompt_sent
    ));
    output.push_str(&format!(
        "- Prompt Chars: `{}`\n",
        session.prompt_sent.chars().count()
    ));
    output.push_str(&format!(
        "- Prompt Lines: `{}`\n",
        line_count(&session.prompt_sent)
    ));
    output.push_str(&format!(
        "- Prompt Hash (fnv1a64): `{}`\n",
        fnv1a64_hex(&session.prompt_sent)
    ));
    output.push_str("- Prompt Body: `stored_in_session`\n\n");

    output.push_str("## Parsed Response\n\n```text\n");
    let interpreted =
        render_interpreted_response_excerpt(&session.response.response_interpretation, 120, 8_000);
    output.push_str(&interpreted);
    if !interpreted.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("```\n\n");
    output.push_str(&format!(
        "- Raw Body Bytes (stored in session): `{}`\n\n",
        session.response.raw_http_body.len()
    ));

    if !session.warnings.is_empty() {
        output.push_str("## Warnings\n\n");
        for warning in &session.warnings {
            output.push_str(&format!("- `{warning}`\n"));
        }
        output.push('\n');
    }

    if let Some(incremental) = &incremental {
        output.push_str("## Incremental Scope\n\n");
        output.push_str("- Incremental Mode: `enabled`\n");
        if let Some(from) = incremental.from.as_deref() {
            output.push_str(&format!("- Base Ref: `{from}`\n"));
        }
        if let Some(scope) = incremental.scope.as_deref() {
            output.push_str(&format!("- Scope: `{scope}`\n"));
        }
        if let Some(context_lines) = incremental.context_lines {
            output.push_str(&format!("- Context Lines: `{context_lines}`\n"));
        }
        if let Some(file_count) = incremental.file_count {
            output.push_str(&format!("- Incremental File Count: `{file_count}`\n"));
        }
        if let Some(fallback_count) = incremental.fallback_full_file_count {
            output.push_str(&format!("- Full-File Fallback Count: `{fallback_count}`\n"));
        }
        if !incremental.fallback_full_files.is_empty() {
            output.push_str("- Full-File Fallback Files:\n");
            for path in &incremental.fallback_full_files {
                output.push_str(&format!("  - `{path}`\n"));
            }
        }
        output.push_str(
            "- Scope Note: `diff_hunks` reviews git diff hunks; fallback files are reviewed as full file content.\n\n",
        );
    }

    output.push_str("## Triage Diagnostics\n\n");
    output.push_str(&format!(
        "- Duplicate Summary Clusters: `{}`\n",
        repeated_summaries.len()
    ));
    output.push_str(&format!(
        "- Duplicate Summary Findings: `{}`\n",
        duplicate_summary_findings
    ));
    if repeated_summaries.is_empty() {
        output.push_str("- Repeated Summaries: `none`\n\n");
    } else {
        output.push_str("- Repeated Summaries:\n");
        for (summary, count) in repeated_summaries.iter().take(5) {
            output.push_str(&format!("  - `{count}` x `{summary}`\n"));
        }
        output.push('\n');
    }

    output.push_str("## Findings\n\n");
    if session.findings.is_empty() {
        output.push_str("_No extracted findings._\n");
        return output;
    }

    for finding in &session.findings {
        output.push_str(&format!("### {}\n\n", finding.summary));
        output.push_str(&format!(
            "- Normalization State: `{}`\n",
            finding.normalization_state.as_str()
        ));
        output.push_str(&format!(
            "- Severity Origin: `{}`\n",
            finding.severity_origin.as_str()
        ));
        output.push_str(&format!(
            "- Severity: `{}`\n",
            finding
                .severity
                .map(|severity| severity.as_str().to_string())
                .unwrap_or_else(|| "unrated".to_string())
        ));
        output.push_str(&format!(
            "- Confidence: `{}`\n",
            finding.confidence.as_str()
        ));
        if let Some(location) = &finding.location_hint {
            output.push_str(&format!("- Location: `{location}`\n"));
        }
        if let Some(evidence) = &finding.evidence_text {
            output.push_str(&format!("- Evidence: {evidence}\n"));
        }
        if let Some(why) = &finding.why_it_matters {
            output.push_str(&format!("- Why It Matters: {why}\n"));
        }
        if let Some(action) = &finding.action {
            output.push_str(&format!("- Action: {action}\n"));
        }
        output.push('\n');
    }

    output
}

pub fn export_session_json(session: &Session) -> String {
    let repeated_summaries = repeated_summary_clusters(&session.findings);
    let duplicate_summary_findings = repeated_summaries
        .iter()
        .map(|(_, count)| *count)
        .sum::<usize>();
    let incremental = extract_incremental_warnings(&session.warnings);
    let findings = session
        .findings
        .iter()
        .map(|finding| {
            json!({
                "id": finding.id,
                "session_id": finding.session_id,
                "review_mode": finding.review_mode.as_str(),
                "target": finding.target,
                "summary": finding.summary,
                "why_it_matters": finding.why_it_matters,
                "severity": finding.severity.map(|severity| severity.as_str()),
                "severity_origin": finding.severity_origin.as_str(),
                "confidence": finding.confidence.as_str(),
                "risk_class": finding.risk_class,
                "action": finding.action,
                "status": finding.status,
                "location_hint": finding.location_hint,
                "evidence_text": finding.evidence_text,
                "raw_labels": finding.raw_labels,
                "normalization_state": finding.normalization_state.as_str(),
            })
        })
        .collect::<Vec<Value>>();

    let payload = json!({
        "session": {
            "id": session.id,
            "repository_root": session.repository_root,
            "review_mode": session.review_mode.as_str(),
            "selected_target": format_target(&session.selected_target),
            "profile": {
                "name": session.profile.name,
                "source": session.profile.source,
                "path": session.profile.path,
                "hash": session.profile.hash,
            },
            "prompt": {
                "preview_equals_sent": session.prompt_preview == session.prompt_sent,
                "chars": session.prompt_sent.chars().count(),
                "lines": line_count(&session.prompt_sent),
                "hash_fnv1a64": fnv1a64_hex(&session.prompt_sent),
                "stored_in_session": true,
            },
            "backend": {
                "base_url": session.backend.base_url,
                "model": session.backend.model,
                "temperature": session.backend.temperature,
                "max_tokens": session.backend.max_tokens,
                "timeout_ms": session.backend.timeout_ms,
                "stop_sequences": session.backend.stop_sequences,
            },
            "response": {
                "status_code": session.response.status_code,
                "response_interpretation": response_interpretation_to_json(&session.response.response_interpretation),
                "raw_http_body_bytes": session.response.raw_http_body.len(),
            },
            "incremental": incremental_to_json(incremental.as_ref()),
            "triage": {
                "total_findings": session.findings.len(),
                "duplicate_summary_clusters": repeated_summaries.len(),
                "duplicate_summary_findings": duplicate_summary_findings,
                "repeated_summaries": repeated_summaries
                    .iter()
                    .map(|(summary, count)| {
                        json!({
                            "summary": summary,
                            "count": count,
                        })
                    })
                    .collect::<Vec<Value>>(),
            },
            "warnings": session.warnings,
            "created_at": session.created_at,
        },
        "findings": findings
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
}

fn render_interpreted_response_excerpt(
    interpretation: &ResponseInterpretation,
    max_lines: usize,
    max_chars: usize,
) -> String {
    match interpretation {
        ResponseInterpretation::Completed { content } => {
            clip_text_for_humans(content, max_lines, max_chars)
        }
        ResponseInterpretation::Empty => "<empty>".to_string(),
        ResponseInterpretation::Malformed { reason } => reason.clone(),
    }
}

fn response_interpretation_to_json(interpretation: &ResponseInterpretation) -> Value {
    match interpretation {
        ResponseInterpretation::Completed { content } => json!({
            "kind": "completed",
            "content": content,
        }),
        ResponseInterpretation::Empty => json!({
            "kind": "empty",
        }),
        ResponseInterpretation::Malformed { reason } => json!({
            "kind": "malformed",
            "reason": reason,
        }),
    }
}

fn clip_text_for_humans(content: &str, max_lines: usize, max_chars: usize) -> String {
    if content.is_empty() {
        return String::new();
    }

    let mut clipped = String::new();
    let mut chars_written = 0_usize;

    for (lines_written, line) in content.lines().enumerate() {
        let line_len = line.chars().count();
        let next_chars = chars_written + line_len + if lines_written > 0 { 1 } else { 0 };
        if lines_written >= max_lines || next_chars > max_chars {
            clipped
                .push_str("\n... <truncated for readability; full content is stored in session>");
            return clipped;
        }
        if lines_written > 0 {
            clipped.push('\n');
            chars_written += 1;
        }
        clipped.push_str(line);
        chars_written += line_len;
    }

    if content.ends_with('\n') {
        clipped.push('\n');
    }
    clipped
}

fn fnv1a64_hex(value: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn line_count(value: &str) -> usize {
    if value.is_empty() {
        0
    } else {
        value.lines().count()
    }
}

fn format_target(target: &reviva_core::RevivaTarget) -> String {
    match target {
        reviva_core::RevivaTarget::Single(path) => format!("single:{path}"),
        reviva_core::RevivaTarget::Set(paths) => format!("set:[{}]", paths.join(", ")),
        reviva_core::RevivaTarget::Boundary(boundary) => {
            format!("boundary:left={} right={}", boundary.left, boundary.right)
        }
    }
}

fn repeated_summary_clusters(findings: &[reviva_core::Finding]) -> Vec<(String, usize)> {
    let mut summary_counts = BTreeMap::new();
    for finding in findings {
        let key = normalize_summary_for_triage(&finding.summary);
        let count = summary_counts.entry(key).or_insert(0_usize);
        *count += 1;
    }
    let mut repeats = summary_counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .collect::<Vec<_>>();
    repeats.sort_by(|(left_summary, left_count), (right_summary, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_summary.cmp(right_summary))
    });
    repeats
}

fn normalize_summary_for_triage(summary: &str) -> String {
    summary
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct IncrementalWarnings {
    from: Option<String>,
    scope: Option<String>,
    context_lines: Option<usize>,
    file_count: Option<usize>,
    fallback_full_file_count: Option<usize>,
    fallback_full_files: Vec<String>,
}

fn extract_incremental_warnings(warnings: &[String]) -> Option<IncrementalWarnings> {
    let mut result = IncrementalWarnings::default();
    for warning in warnings {
        if let Some(value) = warning_value(warning, "incremental_from=") {
            result.from = Some(value.to_string());
            continue;
        }
        if let Some(value) = warning_value(warning, "incremental_scope=") {
            result.scope = Some(value.to_string());
            continue;
        }
        if let Some(value) = warning_value(warning, "incremental_context_lines=") {
            result.context_lines = value.parse::<usize>().ok();
            continue;
        }
        if let Some(value) = warning_value(warning, "incremental_file_count=") {
            result.file_count = value.parse::<usize>().ok();
            continue;
        }
        if let Some(value) = warning_value(warning, "incremental_fallback_full_file_count=") {
            result.fallback_full_file_count = value.parse::<usize>().ok();
            continue;
        }
        if let Some(value) = warning_value(warning, "incremental_fallback_full_file=") {
            result.fallback_full_files.push(value.to_string());
        }
    }
    if result.from.is_some() || result.scope.is_some() {
        Some(result)
    } else {
        None
    }
}

fn warning_value<'a>(warning: &'a str, prefix: &str) -> Option<&'a str> {
    warning.strip_prefix(prefix)
}

fn incremental_to_json(incremental: Option<&IncrementalWarnings>) -> Value {
    match incremental {
        Some(value) => json!({
            "enabled": true,
            "from": value.from,
            "scope": value.scope,
            "context_lines": value.context_lines,
            "file_count": value.file_count,
            "fallback_full_file_count": value.fallback_full_file_count,
            "fallback_full_files": value.fallback_full_files,
            "scope_note": "diff_hunks reviews git diff hunks; fallback files are reviewed as full file content.",
        }),
        None => json!({
            "enabled": false,
        }),
    }
}

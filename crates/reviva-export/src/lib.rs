use reviva_core::Session;
use serde_json::{json, Value};

pub fn export_session_markdown(session: &Session) -> String {
    let mut output = String::new();
    output.push_str("# Reviva Session Export\n\n");
    output.push_str(&format!("- Session ID: `{}`\n", session.id));
    output.push_str(&format!("- Created At: `{}`\n", session.created_at));
    output.push_str(&format!("- Mode: `{}`\n", session.review_mode.as_str()));
    output.push_str(&format!(
        "- Target: `{}`\n\n",
        format_target(&session.selected_target)
    ));

    output.push_str("## Prompt\n\n```text\n");
    output.push_str(&session.prompt_sent);
    if !session.prompt_sent.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("```\n\n");

    output.push_str("## Raw Response\n\n```text\n");
    output.push_str(&session.response.raw_http_body);
    if !session.response.raw_http_body.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("```\n\n");

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
            "prompt_preview": session.prompt_preview,
            "prompt_sent": session.prompt_sent,
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
                "raw_http_body": session.response.raw_http_body,
                "response_interpretation": format!("{:?}", session.response.response_interpretation),
            },
            "warnings": session.warnings,
            "created_at": session.created_at,
        },
        "findings": findings
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
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

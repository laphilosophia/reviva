use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RevivaMode {
    Contract,
    Boundary,
    Boundedness,
    FailureSemantics,
    PerformanceRisk,
    MemoryRisk,
    OperatorCorrectness,
    LaunchReadiness,
    Maintainability,
}

impl RevivaMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::Boundary => "boundary",
            Self::Boundedness => "boundedness",
            Self::FailureSemantics => "failure-semantics",
            Self::PerformanceRisk => "performance-risk",
            Self::MemoryRisk => "memory-risk",
            Self::OperatorCorrectness => "operator-correctness",
            Self::LaunchReadiness => "launch-readiness",
            Self::Maintainability => "maintainability",
        }
    }

    pub const fn all() -> &'static [RevivaMode] {
        &[
            Self::Contract,
            Self::Boundary,
            Self::Boundedness,
            Self::FailureSemantics,
            Self::PerformanceRisk,
            Self::MemoryRisk,
            Self::OperatorCorrectness,
            Self::LaunchReadiness,
            Self::Maintainability,
        ]
    }
}

impl fmt::Display for RevivaMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseModeError {
    pub value: String,
}

impl fmt::Display for ParseModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported review mode: {}", self.value)
    }
}

impl std::error::Error for ParseModeError {}

impl FromStr for RevivaMode {
    type Err = ParseModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "contract" => Ok(Self::Contract),
            "boundary" => Ok(Self::Boundary),
            "boundedness" => Ok(Self::Boundedness),
            "failure-semantics" => Ok(Self::FailureSemantics),
            "performance-risk" => Ok(Self::PerformanceRisk),
            "memory-risk" => Ok(Self::MemoryRisk),
            "operator-correctness" => Ok(Self::OperatorCorrectness),
            "launch-readiness" => Ok(Self::LaunchReadiness),
            "maintainability" => Ok(Self::Maintainability),
            _ => Err(ParseModeError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryTarget {
    pub left: String,
    pub right: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevivaTarget {
    Single(String),
    Set(Vec<String>),
    Boundary(BoundaryTarget),
}

impl RevivaTarget {
    pub fn as_paths(&self) -> Vec<&str> {
        match self {
            Self::Single(path) => vec![path.as_str()],
            Self::Set(paths) => paths.iter().map(|path| path.as_str()).collect(),
            Self::Boundary(boundary) => vec![boundary.left.as_str(), boundary.right.as_str()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeverityOrigin {
    ModelLabeled,
    Normalized,
    Unrated,
}

impl SeverityOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelLabeled => "model_labeled",
            Self::Normalized => "normalized",
            Self::Unrated => "unrated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Low,
    Medium,
    High,
    Unknown,
}

impl Confidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizationState {
    Structured,
    Partial,
    RawOnly,
}

impl NormalizationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Structured => "structured",
            Self::Partial => "partial",
            Self::RawOnly => "raw_only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub id: String,
    pub session_id: String,
    pub review_mode: RevivaMode,
    pub target: String,
    pub summary: String,
    pub why_it_matters: Option<String>,
    pub severity: Option<Severity>,
    pub severity_origin: SeverityOrigin,
    pub confidence: Confidence,
    pub risk_class: Option<String>,
    pub action: Option<String>,
    pub status: Option<String>,
    pub location_hint: Option<String>,
    pub evidence_text: Option<String>,
    pub raw_labels: Vec<String>,
    pub normalization_state: NormalizationState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BackendSettings {
    pub base_url: String,
    pub model: Option<String>,
    pub temperature: f32,
    pub max_tokens: u32,
    pub timeout_ms: u64,
    pub stop_sequences: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RevivaRequest {
    pub backend: BackendSettings,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseInterpretation {
    Completed { content: String },
    Empty,
    Malformed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevivaResponse {
    pub status_code: Option<u16>,
    pub raw_http_body: String,
    pub response_interpretation: ResponseInterpretation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub repository_root: String,
    pub review_mode: RevivaMode,
    pub selected_target: RevivaTarget,
    pub prompt_preview: String,
    pub prompt_sent: String,
    pub backend: BackendSettings,
    pub response: RevivaResponse,
    pub findings: Vec<Finding>,
    pub created_at: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSet {
    pub name: String,
    pub paths: Vec<String>,
}

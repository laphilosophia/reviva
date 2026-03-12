use reviva_core::{
    BackendSettings, BoundaryTarget, Confidence, Finding, NamedSet, NormalizationState,
    ProfileMetadata, RevivaMode, RevivaResponse, RevivaTarget, Session, Severity, SeverityOrigin,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub backend_url: String,
    pub model: Option<String>,
    pub prompt_wrapper: Option<String>,
    pub llama_lifecycle_policy: Option<String>,
    pub review_profile: Option<String>,
    pub review_profile_file: Option<String>,
    pub llama_server_path: Option<String>,
    pub llama_model_path: Option<String>,
    pub timeout_ms: u64,
    pub max_tokens: u32,
    pub temperature: f32,
    pub stop_sequences: Vec<String>,
    pub max_file_bytes: usize,
    pub estimated_prompt_tokens: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            backend_url: "http://127.0.0.1:8080".to_string(),
            model: None,
            prompt_wrapper: None,
            llama_lifecycle_policy: None,
            review_profile: None,
            review_profile_file: None,
            llama_server_path: None,
            llama_model_path: None,
            timeout_ms: 60_000,
            max_tokens: 2048,
            temperature: 0.1,
            stop_sequences: Vec::new(),
            max_file_bytes: 256 * 1024,
            estimated_prompt_tokens: 16_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub created_at: String,
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    Io(String),
    Serialize(String),
    Deserialize(String),
    NotFound(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(f, "storage I/O error: {message}"),
            Self::Serialize(message) => write!(f, "storage serialization error: {message}"),
            Self::Deserialize(message) => write!(f, "storage deserialization error: {message}"),
            Self::NotFound(message) => write!(f, "storage object not found: {message}"),
        }
    }
}

impl std::error::Error for StorageError {}

pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(repository_root: &Path) -> Self {
        Self {
            root: repository_root.join(".reviva"),
        }
    }

    pub fn from_reviva_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn init(&self) -> Result<(), StorageError> {
        fs::create_dir_all(self.root.join("sessions"))
            .and_then(|_| fs::create_dir_all(self.root.join("sets")))
            .and_then(|_| fs::create_dir_all(self.root.join("findings")))
            .and_then(|_| fs::create_dir_all(self.root.join("exports")))
            .map_err(|error| StorageError::Io(error.to_string()))
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<(), StorageError> {
        self.init()?;
        let dto = AppConfigDto::from(config.clone());
        let toml = toml::to_string_pretty(&dto)
            .map_err(|error| StorageError::Serialize(error.to_string()))?;
        fs::write(self.root.join("config.toml"), toml)
            .map_err(|error| StorageError::Io(error.to_string()))
    }

    pub fn load_config(&self) -> Result<AppConfig, StorageError> {
        let path = self.root.join("config.toml");
        if !path.exists() {
            return Ok(AppConfig::default());
        }
        let content =
            fs::read_to_string(&path).map_err(|error| StorageError::Io(error.to_string()))?;
        let dto: AppConfigDto = toml::from_str(&content)
            .map_err(|error| StorageError::Deserialize(error.to_string()))?;
        Ok(dto.into())
    }

    pub fn save_session(&self, session: &Session) -> Result<PathBuf, StorageError> {
        self.init()?;
        let dto = SessionDto::from(session.clone());
        let json = serde_json::to_string_pretty(&dto)
            .map_err(|error| StorageError::Serialize(error.to_string()))?;
        let path = self
            .root
            .join("sessions")
            .join(format!("{}.json", session.id));
        fs::write(&path, json).map_err(|error| StorageError::Io(error.to_string()))?;
        Ok(path)
    }

    pub fn load_session(&self, session_id: &str) -> Result<Session, StorageError> {
        let path = self
            .root
            .join("sessions")
            .join(format!("{session_id}.json"));
        if !path.exists() {
            return Err(StorageError::NotFound(path.display().to_string()));
        }
        let content =
            fs::read_to_string(&path).map_err(|error| StorageError::Io(error.to_string()))?;
        let dto: SessionDto = serde_json::from_str(&content)
            .map_err(|error| StorageError::Deserialize(error.to_string()))?;
        Ok(dto.into())
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>, StorageError> {
        let sessions_path = self.root.join("sessions");
        if !sessions_path.exists() {
            return Ok(Vec::new());
        }
        let mut summaries = Vec::new();
        let entries =
            fs::read_dir(sessions_path).map_err(|error| StorageError::Io(error.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|error| StorageError::Io(error.to_string()))?;
            if !entry.path().is_file() {
                continue;
            }
            let content = fs::read_to_string(entry.path())
                .map_err(|error| StorageError::Io(error.to_string()))?;
            let dto: SessionDto = serde_json::from_str(&content)
                .map_err(|error| StorageError::Deserialize(error.to_string()))?;
            summaries.push(SessionSummary {
                id: dto.id,
                created_at: dto.created_at,
                mode: dto.review_mode,
            });
        }
        summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(summaries)
    }

    pub fn save_named_set(&self, set: &NamedSet) -> Result<PathBuf, StorageError> {
        self.init()?;
        let json = serde_json::to_string_pretty(&NamedSetDto::from(set.clone()))
            .map_err(|error| StorageError::Serialize(error.to_string()))?;
        let path = self.root.join("sets").join(format!("{}.json", set.name));
        fs::write(&path, json).map_err(|error| StorageError::Io(error.to_string()))?;
        Ok(path)
    }

    pub fn load_named_set(&self, name: &str) -> Result<NamedSet, StorageError> {
        let path = self.root.join("sets").join(format!("{name}.json"));
        if !path.exists() {
            return Err(StorageError::NotFound(path.display().to_string()));
        }
        let content =
            fs::read_to_string(path).map_err(|error| StorageError::Io(error.to_string()))?;
        let dto: NamedSetDto = serde_json::from_str(&content)
            .map_err(|error| StorageError::Deserialize(error.to_string()))?;
        Ok(dto.into())
    }

    pub fn list_named_sets(&self) -> Result<Vec<NamedSet>, StorageError> {
        let sets_path = self.root.join("sets");
        if !sets_path.exists() {
            return Ok(Vec::new());
        }
        let mut sets: Vec<NamedSet> = Vec::new();
        let entries =
            fs::read_dir(sets_path).map_err(|error| StorageError::Io(error.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|error| StorageError::Io(error.to_string()))?;
            if !entry.path().is_file() {
                continue;
            }
            let content = fs::read_to_string(entry.path())
                .map_err(|error| StorageError::Io(error.to_string()))?;
            let dto: NamedSetDto = serde_json::from_str(&content)
                .map_err(|error| StorageError::Deserialize(error.to_string()))?;
            sets.push(dto.into());
        }
        sets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(sets)
    }

    pub fn list_findings(
        &self,
        session_filter: Option<&str>,
    ) -> Result<Vec<Finding>, StorageError> {
        let sessions = if let Some(session_id) = session_filter {
            vec![self.load_session(session_id)?]
        } else {
            let mut sessions = Vec::new();
            for summary in self.list_sessions()? {
                sessions.push(self.load_session(&summary.id)?);
            }
            sessions
        };

        let mut findings = Vec::new();
        for session in sessions {
            findings.extend(session.findings);
        }
        Ok(findings)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfigDto {
    backend_url: String,
    model: Option<String>,
    prompt_wrapper: Option<String>,
    llama_lifecycle_policy: Option<String>,
    review_profile: Option<String>,
    review_profile_file: Option<String>,
    llama_server_path: Option<String>,
    llama_model_path: Option<String>,
    timeout_ms: u64,
    max_tokens: u32,
    temperature: f32,
    stop_sequences: Vec<String>,
    max_file_bytes: usize,
    estimated_prompt_tokens: usize,
}

impl From<AppConfig> for AppConfigDto {
    fn from(value: AppConfig) -> Self {
        Self {
            backend_url: value.backend_url,
            model: value.model,
            prompt_wrapper: value.prompt_wrapper,
            llama_lifecycle_policy: value.llama_lifecycle_policy,
            review_profile: value.review_profile,
            review_profile_file: value.review_profile_file,
            llama_server_path: value.llama_server_path,
            llama_model_path: value.llama_model_path,
            timeout_ms: value.timeout_ms,
            max_tokens: value.max_tokens,
            temperature: value.temperature,
            stop_sequences: value.stop_sequences,
            max_file_bytes: value.max_file_bytes,
            estimated_prompt_tokens: value.estimated_prompt_tokens,
        }
    }
}

impl From<AppConfigDto> for AppConfig {
    fn from(value: AppConfigDto) -> Self {
        Self {
            backend_url: value.backend_url,
            model: value.model,
            prompt_wrapper: value.prompt_wrapper,
            llama_lifecycle_policy: value.llama_lifecycle_policy,
            review_profile: value.review_profile,
            review_profile_file: value.review_profile_file,
            llama_server_path: value.llama_server_path,
            llama_model_path: value.llama_model_path,
            timeout_ms: value.timeout_ms,
            max_tokens: value.max_tokens,
            temperature: value.temperature,
            stop_sequences: value.stop_sequences,
            max_file_bytes: value.max_file_bytes,
            estimated_prompt_tokens: value.estimated_prompt_tokens,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct NamedSetDto {
    name: String,
    paths: Vec<String>,
}

impl From<NamedSet> for NamedSetDto {
    fn from(value: NamedSet) -> Self {
        Self {
            name: value.name,
            paths: value.paths,
        }
    }
}

impl From<NamedSetDto> for NamedSet {
    fn from(value: NamedSetDto) -> Self {
        Self {
            name: value.name,
            paths: value.paths,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionDto {
    id: String,
    repository_root: String,
    review_mode: String,
    selected_target: RevivaTargetDto,
    prompt_preview: String,
    prompt_sent: String,
    backend: BackendSettingsDto,
    response: RevivaResponseDto,
    findings: Vec<FindingDto>,
    #[serde(default)]
    profile: Option<ProfileMetadataDto>,
    created_at: String,
    warnings: Vec<String>,
}

impl From<Session> for SessionDto {
    fn from(value: Session) -> Self {
        Self {
            id: value.id,
            repository_root: value.repository_root,
            review_mode: value.review_mode.as_str().to_string(),
            selected_target: value.selected_target.into(),
            prompt_preview: value.prompt_preview,
            prompt_sent: value.prompt_sent,
            backend: value.backend.into(),
            response: value.response.into(),
            findings: value.findings.into_iter().map(Into::into).collect(),
            profile: Some(value.profile.into()),
            created_at: value.created_at,
            warnings: value.warnings,
        }
    }
}

impl From<SessionDto> for Session {
    fn from(value: SessionDto) -> Self {
        Self {
            id: value.id,
            repository_root: value.repository_root,
            review_mode: parse_mode(&value.review_mode),
            selected_target: value.selected_target.into(),
            prompt_preview: value.prompt_preview,
            prompt_sent: value.prompt_sent,
            backend: value.backend.into(),
            response: value.response.into(),
            findings: value.findings.into_iter().map(Into::into).collect(),
            profile: value.profile.map(Into::into).unwrap_or(ProfileMetadata {
                name: "default".to_string(),
                source: "unknown".to_string(),
                path: None,
                hash: "unknown".to_string(),
            }),
            created_at: value.created_at,
            warnings: value.warnings,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ProfileMetadataDto {
    name: String,
    source: String,
    path: Option<String>,
    hash: String,
}

impl From<ProfileMetadata> for ProfileMetadataDto {
    fn from(value: ProfileMetadata) -> Self {
        Self {
            name: value.name,
            source: value.source,
            path: value.path,
            hash: value.hash,
        }
    }
}

impl From<ProfileMetadataDto> for ProfileMetadata {
    fn from(value: ProfileMetadataDto) -> Self {
        Self {
            name: value.name,
            source: value.source,
            path: value.path,
            hash: value.hash,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RevivaTargetDto {
    Single { path: String },
    Set { paths: Vec<String> },
    Boundary { left: String, right: String },
}

impl From<RevivaTarget> for RevivaTargetDto {
    fn from(value: RevivaTarget) -> Self {
        match value {
            RevivaTarget::Single(path) => Self::Single { path },
            RevivaTarget::Set(paths) => Self::Set { paths },
            RevivaTarget::Boundary(boundary) => Self::Boundary {
                left: boundary.left,
                right: boundary.right,
            },
        }
    }
}

impl From<RevivaTargetDto> for RevivaTarget {
    fn from(value: RevivaTargetDto) -> Self {
        match value {
            RevivaTargetDto::Single { path } => Self::Single(path),
            RevivaTargetDto::Set { paths } => Self::Set(paths),
            RevivaTargetDto::Boundary { left, right } => {
                Self::Boundary(BoundaryTarget { left, right })
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct BackendSettingsDto {
    base_url: String,
    model: Option<String>,
    temperature: f32,
    max_tokens: u32,
    timeout_ms: u64,
    stop_sequences: Vec<String>,
}

impl From<BackendSettings> for BackendSettingsDto {
    fn from(value: BackendSettings) -> Self {
        Self {
            base_url: value.base_url,
            model: value.model,
            temperature: value.temperature,
            max_tokens: value.max_tokens,
            timeout_ms: value.timeout_ms,
            stop_sequences: value.stop_sequences,
        }
    }
}

impl From<BackendSettingsDto> for BackendSettings {
    fn from(value: BackendSettingsDto) -> Self {
        Self {
            base_url: value.base_url,
            model: value.model,
            temperature: value.temperature,
            max_tokens: value.max_tokens,
            timeout_ms: value.timeout_ms,
            stop_sequences: value.stop_sequences,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RevivaResponseDto {
    status_code: Option<u16>,
    raw_http_body: String,
    response_interpretation: ResponseInterpretationDto,
}

impl From<RevivaResponse> for RevivaResponseDto {
    fn from(value: RevivaResponse) -> Self {
        Self {
            status_code: value.status_code,
            raw_http_body: value.raw_http_body,
            response_interpretation: value.response_interpretation.into(),
        }
    }
}

impl From<RevivaResponseDto> for RevivaResponse {
    fn from(value: RevivaResponseDto) -> Self {
        Self {
            status_code: value.status_code,
            raw_http_body: value.raw_http_body,
            response_interpretation: value.response_interpretation.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ResponseInterpretationDto {
    Completed { content: String },
    Empty,
    Malformed { reason: String },
}

impl From<reviva_core::ResponseInterpretation> for ResponseInterpretationDto {
    fn from(value: reviva_core::ResponseInterpretation) -> Self {
        match value {
            reviva_core::ResponseInterpretation::Completed { content } => {
                Self::Completed { content }
            }
            reviva_core::ResponseInterpretation::Empty => Self::Empty,
            reviva_core::ResponseInterpretation::Malformed { reason } => Self::Malformed { reason },
        }
    }
}

impl From<ResponseInterpretationDto> for reviva_core::ResponseInterpretation {
    fn from(value: ResponseInterpretationDto) -> Self {
        match value {
            ResponseInterpretationDto::Completed { content } => Self::Completed { content },
            ResponseInterpretationDto::Empty => Self::Empty,
            ResponseInterpretationDto::Malformed { reason } => Self::Malformed { reason },
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FindingDto {
    id: String,
    session_id: String,
    review_mode: String,
    target: String,
    summary: String,
    why_it_matters: Option<String>,
    severity: Option<String>,
    severity_origin: String,
    confidence: String,
    risk_class: Option<String>,
    action: Option<String>,
    status: Option<String>,
    location_hint: Option<String>,
    evidence_text: Option<String>,
    raw_labels: Vec<String>,
    normalization_state: String,
}

impl From<Finding> for FindingDto {
    fn from(value: Finding) -> Self {
        Self {
            id: value.id,
            session_id: value.session_id,
            review_mode: value.review_mode.as_str().to_string(),
            target: value.target,
            summary: value.summary,
            why_it_matters: value.why_it_matters,
            severity: value.severity.map(|severity| severity.as_str().to_string()),
            severity_origin: value.severity_origin.as_str().to_string(),
            confidence: value.confidence.as_str().to_string(),
            risk_class: value.risk_class,
            action: value.action,
            status: value.status,
            location_hint: value.location_hint,
            evidence_text: value.evidence_text,
            raw_labels: value.raw_labels,
            normalization_state: value.normalization_state.as_str().to_string(),
        }
    }
}

impl From<FindingDto> for Finding {
    fn from(value: FindingDto) -> Self {
        Self {
            id: value.id,
            session_id: value.session_id,
            review_mode: parse_mode(&value.review_mode),
            target: value.target,
            summary: value.summary,
            why_it_matters: value.why_it_matters,
            severity: value.severity.as_deref().and_then(parse_severity),
            severity_origin: parse_severity_origin(&value.severity_origin),
            confidence: parse_confidence(&value.confidence),
            risk_class: value.risk_class,
            action: value.action,
            status: value.status,
            location_hint: value.location_hint,
            evidence_text: value.evidence_text,
            raw_labels: value.raw_labels,
            normalization_state: parse_normalization_state(&value.normalization_state),
        }
    }
}

fn parse_mode(value: &str) -> RevivaMode {
    value.parse().unwrap_or(RevivaMode::Maintainability)
}

fn parse_severity(value: &str) -> Option<Severity> {
    match value {
        "low" => Some(Severity::Low),
        "medium" => Some(Severity::Medium),
        "high" => Some(Severity::High),
        "critical" => Some(Severity::Critical),
        _ => None,
    }
}

fn parse_severity_origin(value: &str) -> SeverityOrigin {
    match value {
        "model_labeled" => SeverityOrigin::ModelLabeled,
        "normalized" => SeverityOrigin::Normalized,
        _ => SeverityOrigin::Unrated,
    }
}

fn parse_confidence(value: &str) -> Confidence {
    match value {
        "low" => Confidence::Low,
        "medium" => Confidence::Medium,
        "high" => Confidence::High,
        _ => Confidence::Unknown,
    }
}

fn parse_normalization_state(value: &str) -> NormalizationState {
    match value {
        "structured" => NormalizationState::Structured,
        "partial" => NormalizationState::Partial,
        _ => NormalizationState::RawOnly,
    }
}

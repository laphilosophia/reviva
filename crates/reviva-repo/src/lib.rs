use ignore::WalkBuilder;
use reviva_core::RevivaTarget;
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSuspicion {
    Generated,
    Minified,
}

impl FileSuspicion {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Minified => "minified",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RepoScanConfig {
    pub max_file_bytes: usize,
    pub include_extensions: Option<Vec<String>>,
}

impl Default for RepoScanConfig {
    fn default() -> Self {
        Self {
            max_file_bytes: 256 * 1024,
            include_extensions: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    pub path: String,
    pub size_bytes: u64,
    pub estimated_tokens: usize,
    pub review_priority_heuristic: u32,
    pub suspicion: Option<FileSuspicion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanResult {
    pub entries: Vec<ScanEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedFile {
    pub path: String,
    pub content: String,
    pub size_bytes: usize,
    pub estimated_tokens: usize,
    pub suspicion: Option<FileSuspicion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoError {
    Io(String),
    FileTooLarge {
        path: String,
        file_size: usize,
        max_file_bytes: usize,
    },
    BinaryFileRejected {
        path: String,
    },
    NonUtf8File {
        path: String,
    },
    PathOutsideRoot {
        path: String,
    },
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(f, "repository I/O error: {message}"),
            Self::FileTooLarge {
                path,
                file_size,
                max_file_bytes,
            } => write!(
                f,
                "file exceeds size limit: {path} ({file_size} bytes > {max_file_bytes} bytes)"
            ),
            Self::BinaryFileRejected { path } => {
                write!(f, "binary file is not reviewable: {path}")
            }
            Self::NonUtf8File { path } => write!(f, "file is not valid UTF-8: {path}"),
            Self::PathOutsideRoot { path } => {
                write!(f, "path escapes repository root: {path}")
            }
        }
    }
}

impl std::error::Error for RepoError {}

pub fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn extension_set(config: &RepoScanConfig) -> Option<HashSet<String>> {
    config.include_extensions.as_ref().map(|items| {
        items
            .iter()
            .map(|item| item.trim_start_matches('.').to_ascii_lowercase())
            .collect()
    })
}

fn has_allowed_extension(path: &Path, allowed: &Option<HashSet<String>>) -> bool {
    match allowed {
        Some(allowed) => {
            let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
                return false;
            };
            allowed.contains(&extension.to_ascii_lowercase())
        }
        None => true,
    }
}

fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() / 4).saturating_add(1)
}

fn estimate_tokens_from_size(size_bytes: usize) -> usize {
    (size_bytes / 4).saturating_add(1)
}

fn detect_binary(path: &Path) -> Result<bool, RepoError> {
    let mut file = fs::File::open(path).map_err(|error| RepoError::Io(error.to_string()))?;
    let mut buffer = [0_u8; 1024];
    let read = file
        .read(&mut buffer)
        .map_err(|error| RepoError::Io(error.to_string()))?;
    Ok(buffer[..read].contains(&0))
}

fn detect_suspicion(path: &str, content: &str) -> Option<FileSuspicion> {
    let lower_path = path.to_ascii_lowercase();
    if lower_path.contains("/dist/")
        || lower_path.contains("/build/")
        || lower_path.contains(".min.")
        || lower_path.ends_with(".lock")
        || content
            .lines()
            .take(4)
            .any(|line| line.to_ascii_lowercase().contains("generated"))
    {
        return Some(FileSuspicion::Generated);
    }

    let line_count = content.lines().count();
    if line_count == 0 {
        return None;
    }

    let avg_line_len = content.len() / line_count;
    if avg_line_len > 180 {
        return Some(FileSuspicion::Minified);
    }
    None
}

fn heuristic_score(path: &str, size_bytes: usize, content: Option<&str>) -> u32 {
    let lower = path.to_ascii_lowercase();
    let mut score = 0_u32;
    if lower.contains("auth")
        || lower.contains("permission")
        || lower.contains("boundary")
        || lower.contains("controller")
        || lower.contains("handler")
    {
        score += 25;
    }
    if lower.contains("error") || lower.contains("retry") {
        score += 10;
    }
    if lower.contains("cache") || lower.contains("state") || lower.contains("memory") {
        score += 10;
    }
    score += ((size_bytes / 1024).min(30)) as u32;
    if let Some(content) = content {
        if content.contains("unsafe") {
            score += 15;
        }
        if content.contains("unwrap(") || content.contains("panic!(") {
            score += 10;
        }
    }
    score
}

pub fn scan_repository(root: &Path, config: &RepoScanConfig) -> Result<ScanResult, RepoError> {
    let allowed_extensions = extension_set(config);
    let local_ignores = read_local_ignores(root)?;
    let mut entries = Vec::new();
    let mut walker = WalkBuilder::new(root);
    walker.standard_filters(true);
    walker.hidden(false);
    walker.git_ignore(true);
    walker.git_exclude(true);

    for item in walker.build() {
        let entry = item.map_err(|error| RepoError::Io(error.to_string()))?;
        let path = entry.path();
        if !path.is_file() || !has_allowed_extension(path, &allowed_extensions) {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|_| RepoError::PathOutsideRoot {
                path: path.display().to_string(),
            })?;
        let normalized = normalize_path(relative);
        if is_ignored(&normalized, &local_ignores) {
            continue;
        }

        if detect_binary(path)? {
            continue;
        }

        let metadata = fs::metadata(path).map_err(|error| RepoError::Io(error.to_string()))?;
        let size_bytes = metadata.len() as usize;
        let estimated_tokens = estimate_tokens_from_size(size_bytes);
        let score = heuristic_score(&normalized, size_bytes, None);
        let suspicion = fs::read_to_string(path)
            .ok()
            .and_then(|content| detect_suspicion(&normalized, &content));

        entries.push(ScanEntry {
            path: normalized,
            size_bytes: metadata.len(),
            estimated_tokens,
            review_priority_heuristic: score,
            suspicion,
        });
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ScanResult { entries })
}

fn read_local_ignores(root: &Path) -> Result<Vec<String>, RepoError> {
    let ignore_path = root.join(".gitignore");
    if !ignore_path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(ignore_path).map_err(|error| RepoError::Io(error.to_string()))?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.trim_start_matches("./").to_string())
        .collect())
}

fn is_ignored(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        if let Some(prefix) = pattern.strip_suffix('/') {
            path.starts_with(prefix)
        } else if pattern.contains('*') {
            let needle = pattern.trim_matches('*');
            !needle.is_empty() && path.contains(needle)
        } else {
            path == pattern || path.ends_with(&format!("/{}", pattern))
        }
    })
}

fn resolve_target_path(root: &Path, relative: &str) -> Result<PathBuf, RepoError> {
    let candidate = root.join(relative);
    let canonical_root = root
        .canonicalize()
        .map_err(|error| RepoError::Io(error.to_string()))?;
    let canonical_candidate = candidate
        .canonicalize()
        .map_err(|error| RepoError::Io(error.to_string()))?;

    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(RepoError::PathOutsideRoot {
            path: relative.to_string(),
        });
    }
    Ok(canonical_candidate)
}

pub fn load_target_files(
    root: &Path,
    target: &RevivaTarget,
    config: &RepoScanConfig,
) -> Result<Vec<LoadedFile>, RepoError> {
    let mut files = Vec::new();
    for relative in target.as_paths() {
        let absolute = resolve_target_path(root, relative)?;
        if detect_binary(&absolute)? {
            return Err(RepoError::BinaryFileRejected {
                path: relative.to_string(),
            });
        }

        let metadata = fs::metadata(&absolute).map_err(|error| RepoError::Io(error.to_string()))?;
        let size_bytes = metadata.len() as usize;
        if size_bytes > config.max_file_bytes {
            return Err(RepoError::FileTooLarge {
                path: relative.to_string(),
                file_size: size_bytes,
                max_file_bytes: config.max_file_bytes,
            });
        }

        let bytes = fs::read(&absolute).map_err(|error| RepoError::Io(error.to_string()))?;
        let content = String::from_utf8(bytes).map_err(|_| RepoError::NonUtf8File {
            path: relative.to_string(),
        })?;
        let suspicion = detect_suspicion(relative, &content);
        files.push(LoadedFile {
            path: relative.to_string(),
            estimated_tokens: estimate_tokens(&content),
            size_bytes,
            suspicion,
            content,
        });
    }
    Ok(files)
}

pub fn estimated_target_tokens(files: &[LoadedFile], note: Option<&str>) -> usize {
    let note_tokens = note.map(estimate_tokens).unwrap_or(0);
    files
        .iter()
        .map(|file| file.estimated_tokens)
        .sum::<usize>()
        + note_tokens
        + 128
}

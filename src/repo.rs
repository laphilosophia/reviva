use crate::core;
use ignore::WalkBuilder;
use core::RevivaTarget;
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

impl Default for RepoScanConfig {
    fn default() -> Self {
        Self {
            max_file_bytes: 256 * 1024,
            include_extensions: None,
            include: Vec::new(),
            exclude: Vec::new(),
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
pub struct IncrementalLoadResult {
    pub files: Vec<LoadedFile>,
    pub fallback_full_files: Vec<String>,
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
    PathExcludedByConfig {
        path: String,
        reason: String,
    },
    GitUnavailable,
    GitDiffFailed {
        from: String,
        message: String,
    },
    NoReviewableChangedFiles {
        from: String,
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
            Self::PathExcludedByConfig { path, reason } => {
                write!(
                    f,
                    "path is excluded from review by config/ignore rules: {path} ({reason})"
                )
            }
            Self::GitUnavailable => write!(
                f,
                "git command is not available in PATH; incremental mode requires git"
            ),
            Self::GitDiffFailed { from, message } => write!(
                f,
                "unable to resolve incremental target from git diff ({from}): {message}"
            ),
            Self::NoReviewableChangedFiles { from } => write!(
                f,
                "incremental mode found no reviewable changed files for base '{from}'"
            ),
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

fn normalize_pattern(pattern: &str) -> String {
    let mut normalized = pattern
        .trim()
        .trim_start_matches("./")
        .trim_start_matches('/')
        .replace('\\', "/")
        .to_ascii_lowercase();
    // Support common globstar forms used in tsconfig-like include/exclude patterns.
    while normalized.contains("/**/*") {
        normalized = normalized.replace("/**/*", "/*");
    }
    while normalized.contains("/**") {
        normalized = normalized.replace("/**", "/*");
    }
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    normalized
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern_chars = pattern.chars().collect::<Vec<_>>();
    let text_chars = text.chars().collect::<Vec<_>>();
    let mut pattern_index = 0_usize;
    let mut text_index = 0_usize;
    let mut star_index = None::<usize>;
    let mut match_index = 0_usize;

    while text_index < text_chars.len() {
        if pattern_index < pattern_chars.len()
            && (pattern_chars[pattern_index] == '?'
                || pattern_chars[pattern_index] == text_chars[text_index])
        {
            pattern_index += 1;
            text_index += 1;
            continue;
        }
        if pattern_index < pattern_chars.len() && pattern_chars[pattern_index] == '*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            match_index = text_index;
            continue;
        }
        if let Some(star) = star_index {
            pattern_index = star + 1;
            match_index += 1;
            text_index = match_index;
            continue;
        }
        return false;
    }

    while pattern_index < pattern_chars.len() && pattern_chars[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern_chars.len()
}

fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    let normalized_path = path.to_ascii_lowercase();
    let normalized_pattern = normalize_pattern(pattern);
    if normalized_pattern.is_empty() {
        return false;
    }
    if normalized_pattern.ends_with('/') {
        return normalized_path.starts_with(&normalized_pattern);
    }
    if !normalized_pattern.contains('*') && !normalized_pattern.contains('?') {
        return normalized_path == normalized_pattern
            || normalized_path.starts_with(&format!("{normalized_pattern}/"));
    }
    wildcard_match(&normalized_pattern, &normalized_path)
}

fn path_matches_any(path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pattern| path_matches_pattern(path, pattern))
}

fn exclusion_reason_for_path(
    path: &str,
    config: &RepoScanConfig,
    local_ignores: &[String],
) -> Option<&'static str> {
    if !config.include.is_empty() && !path_matches_any(path, &config.include) {
        return Some("not_matched_by_include");
    }
    if path_matches_any(path, &config.exclude) {
        return Some("matched_exclude");
    }
    if is_ignored(path, local_ignores) {
        return Some("matched_local_ignore");
    }
    if is_excluded_from_auto_review(path) {
        return Some("auto_excluded");
    }
    None
}

fn is_excluded_from_auto_review(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower == ".git"
        || lower.starts_with(".git/")
        || lower == ".github"
        || lower.starts_with(".github/")
        || lower == ".reviva"
        || lower.starts_with(".reviva/")
        || lower == "node_modules"
        || lower.starts_with("node_modules/")
    {
        return true;
    }

    let file_name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    if file_name.starts_with(".env") {
        return true;
    }
    if matches!(
        file_name.as_str(),
        ".gitignore"
            | ".gitattributes"
            | ".gitmodules"
            | ".editorconfig"
            | ".npmrc"
            | ".yarnrc"
            | ".yarnrc.yml"
            | ".pypirc"
            | ".netrc"
            | ".prettierignore"
            | ".prettierrc.json"
            | "eslint.config.js"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "bun.lockb"
            | "tsup.config.ts"
            | "tsconfig.json"
            | "tsconfig.base.json"
            | "tsconfig.build.json"
            | "vitest.config.ts"
            | "jsconfig.json"
            | "id_rsa"
            | "id_ed25519"
            | "CHANGELOG.md"
            | "CODE_OF_CONDUCT.md"
            | "CONTRIBUTING.md"
            | "LICENSE"
            | "README.md"
            | "SECURITY.md"
    ) {
        return true;
    }

    lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
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
        if exclusion_reason_for_path(&normalized, config, &local_ignores).is_some() {
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
    let local_ignores = read_local_ignores(root)?;
    let mut files = Vec::new();
    for relative in target.as_paths() {
        if let Some(reason) = exclusion_reason_for_path(relative, config, &local_ignores) {
            return Err(RepoError::PathExcludedByConfig {
                path: relative.to_string(),
                reason: reason.to_string(),
            });
        }
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

pub fn load_incremental_target_files(
    root: &Path,
    target: &RevivaTarget,
    config: &RepoScanConfig,
    from: &str,
    context_lines: usize,
) -> Result<IncrementalLoadResult, RepoError> {
    let local_ignores = read_local_ignores(root)?;
    let mut files = Vec::new();
    let mut fallback_full_files = Vec::new();

    for relative in target.as_paths() {
        if let Some(reason) = exclusion_reason_for_path(relative, config, &local_ignores) {
            return Err(RepoError::PathExcludedByConfig {
                path: relative.to_string(),
                reason: reason.to_string(),
            });
        }
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
        let full_content = String::from_utf8(bytes).map_err(|_| RepoError::NonUtf8File {
            path: relative.to_string(),
        })?;
        let suspicion = detect_suspicion(relative, &full_content);

        let content = match git_diff_patch_for_file(root, from, relative, context_lines)? {
            Some(patch) => format!(
                "<<< REVIVA INCREMENTAL DIFF (base={from}, context_lines={context_lines}) >>>\n{patch}\n<<< END REVIVA INCREMENTAL DIFF >>>\n"
            ),
            None => {
                fallback_full_files.push(relative.to_string());
                full_content
            }
        };

        files.push(LoadedFile {
            path: relative.to_string(),
            estimated_tokens: estimate_tokens(&content),
            size_bytes,
            suspicion,
            content,
        });
    }

    Ok(IncrementalLoadResult {
        files,
        fallback_full_files,
    })
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

pub fn resolve_incremental_target(
    root: &Path,
    from: &str,
    config: &RepoScanConfig,
) -> Result<RevivaTarget, RepoError> {
    let changed = changed_files_from_git_diff(root, from)?;
    let scan = scan_repository(root, config)?;
    let reviewable_paths = scan
        .entries
        .into_iter()
        .map(|entry| entry.path)
        .collect::<HashSet<_>>();

    let mut selected = changed
        .into_iter()
        .filter(|path| reviewable_paths.contains(path))
        .collect::<Vec<_>>();
    selected.sort();
    selected.dedup();

    if selected.is_empty() {
        return Err(RepoError::NoReviewableChangedFiles {
            from: from.to_string(),
        });
    }

    Ok(if selected.len() == 1 {
        RevivaTarget::Single(selected[0].clone())
    } else {
        RevivaTarget::Set(selected)
    })
}

fn changed_files_from_git_diff(root: &Path, from: &str) -> Result<Vec<String>, RepoError> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("diff")
        .arg("--name-only")
        .arg("--diff-filter=ACMR")
        .arg(from)
        .arg("--")
        .current_dir(root)
        .output();

    let output = match output {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(RepoError::GitUnavailable);
        }
        Err(error) => return Err(RepoError::Io(error.to_string())),
    };

    if !output.status.success() {
        return Err(RepoError::GitDiffFailed {
            from: from.to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = normalize_path(Path::new(trimmed));
        files.push(normalized);
    }
    Ok(files)
}

fn git_diff_patch_for_file(
    root: &Path,
    from: &str,
    relative_path: &str,
    context_lines: usize,
) -> Result<Option<String>, RepoError> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("diff")
        .arg("--no-color")
        .arg(format!("--unified={context_lines}"))
        .arg(from)
        .arg("--")
        .arg(relative_path)
        .current_dir(root)
        .output();

    let output = match output {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(RepoError::GitUnavailable);
        }
        Err(error) => return Err(RepoError::Io(error.to_string())),
    };

    if !output.status.success() {
        return Err(RepoError::GitDiffFailed {
            from: from.to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{load_target_files, scan_repository, RepoError, RepoScanConfig};
    use crate::core::RevivaTarget;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn scan_excludes_meta_and_sensitive_files_from_auto_review() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");
        fs::create_dir_all(temp.path().join(".github").join("workflows")).expect("mkdir github");
        fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir reviva");
        fs::create_dir_all(temp.path().join("node_modules").join("pkg")).expect("mkdir node");
        fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").expect("write src");
        fs::write(temp.path().join(".env"), "SECRET=1\n").expect("write env");
        fs::write(temp.path().join(".gitignore"), "target/\n").expect("write gitignore");
        fs::write(temp.path().join("package.json"), "{\"name\":\"x\"}\n").expect("write pkg");
        fs::write(temp.path().join("package-lock.json"), "{}\n").expect("write pkg lock");
        fs::write(temp.path().join("tsup.config.ts"), "export default {};\n").expect("write tsup");
        fs::write(temp.path().join("tsconfig.json"), "{}\n").expect("write tsconfig");
        fs::write(temp.path().join("vitest.config.ts"), "export default {};\n")
            .expect("write vitest");
        fs::write(temp.path().join(".github/workflows/ci.yml"), "name: ci\n")
            .expect("write workflow");
        fs::write(
            temp.path().join(".reviva/config.toml"),
            "backend_url = \"x\"\n",
        )
        .expect("write reviva");
        fs::write(
            temp.path().join("node_modules/pkg/index.js"),
            "module.exports = {}\n",
        )
        .expect("write node module");
        fs::write(temp.path().join("server.key"), "PRIVATE-KEY\n").expect("write key");

        let result = scan_repository(temp.path(), &RepoScanConfig::default()).expect("scan");
        let paths = result
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();

        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(!paths.contains(&".env".to_string()));
        assert!(!paths.contains(&".gitignore".to_string()));
        assert!(!paths.contains(&"package.json".to_string()));
        assert!(!paths.contains(&"package-lock.json".to_string()));
        assert!(!paths.contains(&"tsup.config.ts".to_string()));
        assert!(!paths.contains(&"tsconfig.json".to_string()));
        assert!(!paths.contains(&"vitest.config.ts".to_string()));
        assert!(!paths.contains(&".github/workflows/ci.yml".to_string()));
        assert!(!paths.contains(&".reviva/config.toml".to_string()));
        assert!(!paths.contains(&"node_modules/pkg/index.js".to_string()));
        assert!(!paths.contains(&"server.key".to_string()));
    }

    #[test]
    fn scan_honors_include_and_exclude_patterns() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/gen")).expect("mkdir src");
        fs::create_dir_all(temp.path().join("tests")).expect("mkdir tests");
        fs::write(temp.path().join("src/main.ts"), "export const ok = 1;\n").expect("main ts");
        fs::write(
            temp.path().join("src/gen/tmp.ts"),
            "export const tmp = 1;\n",
        )
        .expect("tmp ts");
        fs::write(
            temp.path().join("tests/main.test.ts"),
            "it('ok', () => {});\n",
        )
        .expect("test ts");

        let result = scan_repository(
            temp.path(),
            &RepoScanConfig {
                max_file_bytes: 256 * 1024,
                include_extensions: None,
                include: vec!["src/**/*".to_string()],
                exclude: vec!["src/gen/**/*".to_string()],
            },
        )
        .expect("scan");
        let paths = result
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();

        assert!(paths.contains(&"src/main.ts".to_string()));
        assert!(!paths.contains(&"src/gen/tmp.ts".to_string()));
        assert!(!paths.contains(&"tests/main.test.ts".to_string()));
    }

    #[test]
    fn explicit_target_is_blocked_when_excluded_by_config() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("packages/docs")).expect("mkdir docs");
        fs::write(temp.path().join("packages/docs/guide.md"), "# guide\n").expect("guide");

        let target = RevivaTarget::Single("packages/docs/guide.md".to_string());
        let error = load_target_files(
            temp.path(),
            &target,
            &RepoScanConfig {
                max_file_bytes: 256 * 1024,
                include_extensions: None,
                include: vec!["packages/**/*".to_string()],
                exclude: vec!["packages/docs/**/*".to_string()],
            },
        )
        .expect_err("explicit excluded target must fail");

        match error {
            RepoError::PathExcludedByConfig { path, reason } => {
                assert_eq!(path, "packages/docs/guide.md");
                assert_eq!(reason, "matched_exclude");
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}

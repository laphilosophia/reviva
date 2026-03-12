use reviva_backend::{CompletionBackend, LlamaCompletionBackend};
use reviva_core::{
    BackendSettings, BoundaryTarget, NamedSet, ProfileMetadata, ResponseInterpretation, RevivaMode,
    RevivaRequest, RevivaResponse, RevivaTarget, Session,
};
use reviva_export::{export_session_json, export_session_markdown};
use reviva_prompts::{
    apply_prompt_wrapper, build_prompt, normalize_findings_with_reasons, parse_prompt_wrapper,
    parse_review_profile_toml, resolve_built_in_review_profile, PromptBuildConfig, PromptFile,
    PromptWrapper, ReviewProfileSpec,
};
use reviva_repo::{estimated_target_tokens, load_target_files, scan_repository, RepoScanConfig};
use reviva_storage::{AppConfig, Storage};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "scan" => cmd_scan(&args[2..]),
        "review" => cmd_review(&args[2..]),
        "set" => cmd_set(&args[2..]),
        "session" => cmd_session(&args[2..]),
        "findings" => cmd_findings(&args[2..]),
        "export" => cmd_export(&args[2..]),
        _ => {
            print_usage();
            Ok(())
        }
    }
}

fn cmd_scan(args: &[String]) -> Result<(), String> {
    let repo = parse_repo_arg(args)?;
    let storage = Storage::new(&repo);
    storage.init().map_err(|error| error.to_string())?;
    let config = storage.load_config().map_err(|error| error.to_string())?;
    let repo_config = RepoScanConfig {
        max_file_bytes: config.max_file_bytes,
        include_extensions: None,
    };
    let result = scan_repository(&repo, &repo_config).map_err(|error| error.to_string())?;
    for entry in result.entries {
        let suspicion = entry
            .suspicion
            .as_ref()
            .map(|value| format!(" warning={}", value.as_str()))
            .unwrap_or_default();
        println!(
            "{} size={} estimated_tokens={} review_priority_heuristic={}{}",
            entry.path,
            entry.size_bytes,
            entry.estimated_tokens,
            entry.review_priority_heuristic,
            suspicion
        );
    }
    Ok(())
}

fn cmd_review(args: &[String]) -> Result<(), String> {
    let repo = parse_repo_arg(args)?;
    let mode_arg = parse_optional_arg(args, "--mode");
    let profile = parse_optional_arg(args, "--profile");
    let profile_file = parse_optional_arg(args, "--profile-file");
    let note = parse_optional_arg(args, "--note");
    let model = parse_optional_arg(args, "--model");
    let prompt_wrapper_arg = parse_optional_arg(args, "--prompt-wrapper");
    let kv_cache_arg = parse_optional_arg(args, "--kv-cache");
    let kv_slot_arg = parse_optional_arg(args, "--kv-slot");
    let llama_lifecycle_arg = parse_optional_arg(args, "--llama-lifecycle");
    let llama_model_path = parse_optional_arg(args, "--llama-model-path");
    let llama_server_path = parse_optional_arg(args, "--llama-server-path");
    let preview_only = has_flag(args, "--preview-only");
    let files = parse_repeat_arg(args, "--file");
    let boundary_left = parse_optional_arg(args, "--boundary-left");
    let boundary_right = parse_optional_arg(args, "--boundary-right");

    let storage = Storage::new(&repo);
    storage.init().map_err(|error| error.to_string())?;
    let mut config = storage.load_config().map_err(|error| error.to_string())?;
    let mut config_updated = false;
    if let Some(path) = llama_model_path {
        config.llama_model_path = Some(path);
        config_updated = true;
    }
    if let Some(path) = llama_server_path {
        config.llama_server_path = Some(path);
        config_updated = true;
    }
    if let Some(value) = prompt_wrapper_arg {
        let parsed = parse_prompt_wrapper(&value).map_err(|error| error.to_string())?;
        config.prompt_wrapper = Some(parsed.as_str().to_string());
    }
    if let Some(value) = kv_cache_arg {
        config.llama_kv_cache = Some(parse_kv_cache_flag(&value)?);
        config_updated = true;
    }
    if let Some(value) = kv_slot_arg {
        config.llama_slot_id = Some(parse_kv_slot_id(&value)?);
        config_updated = true;
    }
    if let Some(value) = llama_lifecycle_arg {
        let parsed = parse_llama_lifecycle_policy(&value)?;
        config.llama_lifecycle_policy = Some(parsed.as_str().to_string());
        config_updated = true;
    }
    if config_updated {
        storage
            .save_config(&config)
            .map_err(|error| error.to_string())?;
    }
    let resolved_profile = resolve_review_profile(
        &repo,
        profile.as_deref(),
        profile_file.as_deref(),
        config.review_profile.as_deref(),
        config.review_profile_file.as_deref(),
    )?;
    let (mode, mode_source) = resolve_review_mode(mode_arg.as_deref(), &resolved_profile.spec)?;
    let target = resolve_target(&repo, args, files, boundary_left, boundary_right)?;
    let repo_config = RepoScanConfig {
        max_file_bytes: config.max_file_bytes,
        include_extensions: None,
    };
    let loaded =
        load_target_files(&repo, &target, &repo_config).map_err(|error| error.to_string())?;

    for file in &loaded {
        if let Some(suspicion) = &file.suspicion {
            eprintln!("warning: {} may be {}", file.path, suspicion.as_str());
        }
    }

    let prompt_files = loaded
        .iter()
        .map(|file| PromptFile {
            path: file.path.clone(),
            content: file.content.clone(),
            estimated_tokens: file.estimated_tokens,
            suspicion: file
                .suspicion
                .as_ref()
                .map(|value| value.as_str().to_string()),
        })
        .collect::<Vec<_>>();
    let prompt_result = build_prompt(
        mode,
        &resolved_profile.spec,
        &target,
        &prompt_files,
        note.as_deref(),
        &PromptBuildConfig {
            estimated_prompt_tokens: config.estimated_prompt_tokens,
        },
    )
    .map_err(|error| error.to_string())?;
    let prompt_wrapper = resolve_prompt_wrapper(config.prompt_wrapper.as_deref())?;
    let llama_lifecycle = resolve_llama_lifecycle_policy(config.llama_lifecycle_policy.as_deref())?;
    let kv_cache_enabled = config.llama_kv_cache.unwrap_or(false);
    let kv_slot_id = config.llama_slot_id;
    let wrapped_prompt = apply_prompt_wrapper(&prompt_result.prompt, prompt_wrapper);

    println!("--- PROMPT PREVIEW START ---");
    println!("{}", wrapped_prompt);
    println!("--- PROMPT PREVIEW END ---");
    if preview_only {
        return Ok(());
    }

    let backend_settings = BackendSettings {
        base_url: config.backend_url.clone(),
        model: model.or_else(|| config.model.clone()),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        timeout_ms: config.timeout_ms,
        stop_sequences: config.stop_sequences.clone(),
        cache_prompt: kv_cache_enabled,
        slot_id: kv_slot_id,
    };
    let request = RevivaRequest {
        backend: backend_settings.clone(),
        prompt: wrapped_prompt.clone(),
    };
    let cache_key = build_review_cache_key(&request);
    let cached_session_id = match storage.load_review_cache_session_id(&cache_key) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("warning: review cache read failed: {error}");
            None
        }
    };
    let mut review_cache_source = None::<String>;
    let (response, llama_server_action, llama_server_guard) =
        if let Some(cache_session_id) = cached_session_id {
            match storage.load_session(&cache_session_id) {
                Ok(cached_session) => {
                    eprintln!(
                        "review-cache: hit (source session: {cache_session_id}), backend skipped"
                    );
                    review_cache_source = Some(cache_session_id);
                    (
                        cached_session.response,
                        LlamaServerAction::CacheHitBackendSkipped,
                        LlamaServerGuard::noop(),
                    )
                }
                Err(error) => {
                    eprintln!(
                        "warning: review cache entry is stale and will be ignored: {} ({error})",
                        cache_session_id
                    );
                    execute_backend_review(
                        &storage,
                        &mut config,
                        &backend_settings,
                        llama_lifecycle,
                        &request,
                    )?
                }
            }
        } else {
            execute_backend_review(
                &storage,
                &mut config,
                &backend_settings,
                llama_lifecycle,
                &request,
            )?
        };
    let _llama_server_guard = llama_server_guard;

    let model_output = response_content_for_normalization(&response);
    let session_id = session_id();
    let normalization_report = normalize_findings_with_reasons(
        &session_id,
        mode,
        &target.as_paths().join(","),
        &model_output,
    );
    let normalization_state = normalization_report.state;
    let mut findings = normalization_report.findings;
    for finding in &mut findings {
        finding.normalization_state = normalization_state;
    }

    let mut warnings = vec![
        format!("mode_source={mode_source}"),
        format!("profile={}", resolved_profile.spec.name),
        format!("profile_source={}", resolved_profile.source),
        format!("prompt_wrapper={}", prompt_wrapper.as_str()),
        format!("llama_lifecycle={}", llama_lifecycle.as_str()),
        format!("llama_server_action={}", llama_server_action.as_str()),
        format!(
            "review_cache={}",
            if review_cache_source.is_some() {
                "hit"
            } else {
                "miss"
            }
        ),
        format!("review_cache_key={cache_key}"),
        format!("kv_cache={}", if kv_cache_enabled { "on" } else { "off" }),
        format!(
            "kv_slot={}",
            kv_slot_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "auto".to_string())
        ),
        format!(
            "estimated_token_budget={}",
            estimated_target_tokens(&loaded, note.as_deref())
        ),
    ];
    if let Some(source) = &review_cache_source {
        warnings.push(format!("review_cache_source={source}"));
    }
    for reason in normalization_report.reason_tags {
        warnings.push(format!("normalization_reason={reason}"));
    }

    let session = Session {
        id: session_id.clone(),
        repository_root: repo.display().to_string(),
        review_mode: mode,
        selected_target: target,
        prompt_preview: wrapped_prompt.clone(),
        prompt_sent: wrapped_prompt,
        backend: backend_settings,
        response,
        findings,
        profile: ProfileMetadata {
            name: resolved_profile.spec.name.clone(),
            source: resolved_profile.source.clone(),
            path: resolved_profile.path.clone(),
            hash: resolved_profile.hash.clone(),
        },
        created_at: current_timestamp(),
        warnings,
    };
    let session_path = storage
        .save_session(&session)
        .map_err(|error| error.to_string())?;
    if let Err(error) = storage.save_review_cache_session_id(&cache_key, &session_id) {
        eprintln!("warning: review cache write failed: {error}");
    }

    println!("session saved: {}", session_path.display());
    println!("normalization_state: {}", normalization_state.as_str());
    println!("findings: {}", session.findings.len());
    Ok(())
}

fn cmd_set(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("set requires a subcommand: save|load|list".to_string());
    }
    let repo = parse_repo_arg(args)?;
    let storage = Storage::new(&repo);
    storage.init().map_err(|error| error.to_string())?;

    match args[0].as_str() {
        "save" => {
            let name = parse_required_arg(args, "--name")?;
            let files = parse_repeat_arg(args, "--file");
            if files.is_empty() {
                return Err("set save requires at least one --file".to_string());
            }
            let set = NamedSet { name, paths: files };
            let path = storage
                .save_named_set(&set)
                .map_err(|error| error.to_string())?;
            println!("set saved: {}", path.display());
            Ok(())
        }
        "load" => {
            let name = parse_required_arg(args, "--name")?;
            let set = storage
                .load_named_set(&name)
                .map_err(|error| error.to_string())?;
            println!("{}", set.paths.join("\n"));
            Ok(())
        }
        "list" => {
            let sets = storage
                .list_named_sets()
                .map_err(|error| error.to_string())?;
            for set in sets {
                println!("{} ({})", set.name, set.paths.len());
            }
            Ok(())
        }
        _ => Err("unknown set subcommand".to_string()),
    }
}

fn cmd_session(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("session requires subcommand list|show".to_string());
    }
    let repo = parse_repo_arg(args)?;
    let storage = Storage::new(&repo);
    match args[0].as_str() {
        "list" => {
            for summary in storage.list_sessions().map_err(|error| error.to_string())? {
                println!("{} {} {}", summary.id, summary.created_at, summary.mode);
            }
            Ok(())
        }
        "show" => {
            let id = parse_required_arg(args, "--id")?;
            let session = storage
                .load_session(&id)
                .map_err(|error| error.to_string())?;
            println!("id: {}", session.id);
            println!("mode: {}", session.review_mode.as_str());
            println!("target: {}", session.selected_target.as_paths().join(","));
            println!("profile: {}", session.profile.name);
            println!("profile_source: {}", session.profile.source);
            println!("profile_hash: {}", session.profile.hash);
            if let Some(path) = session.profile.path.as_deref() {
                println!("profile_path: {path}");
            }
            println!("created_at: {}", session.created_at);
            println!("findings: {}", session.findings.len());
            if !session.warnings.is_empty() {
                println!("warnings: {}", session.warnings.len());
                for warning in &session.warnings {
                    println!("warning: {warning}");
                }
            }
            Ok(())
        }
        _ => Err("unknown session subcommand".to_string()),
    }
}

fn cmd_findings(args: &[String]) -> Result<(), String> {
    if args.is_empty() || args[0] != "list" {
        return Err("findings requires: list [--session ID]".to_string());
    }
    let repo = parse_repo_arg(args)?;
    let storage = Storage::new(&repo);
    let session_id = parse_optional_arg(args, "--session");
    let findings = storage
        .list_findings(session_id.as_deref())
        .map_err(|error| error.to_string())?;
    for finding in findings {
        println!(
            "{} | {} | {} | {}",
            finding.id,
            finding.normalization_state.as_str(),
            finding.severity_origin.as_str(),
            finding.summary
        );
    }
    Ok(())
}

fn cmd_export(args: &[String]) -> Result<(), String> {
    let repo = parse_repo_arg(args)?;
    let storage = Storage::new(&repo);
    let session_id = parse_required_arg(args, "--session")?;
    let format = parse_optional_arg(args, "--format").unwrap_or_else(|| "md".to_string());
    let output = parse_optional_arg(args, "--output");

    let session = storage
        .load_session(&session_id)
        .map_err(|error| error.to_string())?;
    let rendered = if format == "json" {
        export_session_json(&session)
    } else {
        export_session_markdown(&session)
    };

    let path = if let Some(output) = output {
        PathBuf::from(output)
    } else {
        storage.root().join("exports").join(format!(
            "{}.{}",
            session_id,
            if format == "json" { "json" } else { "md" }
        ))
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, rendered).map_err(|error| error.to_string())?;
    println!("exported: {}", path.display());
    Ok(())
}

fn resolve_target(
    repo: &Path,
    args: &[String],
    files: Vec<String>,
    boundary_left: Option<String>,
    boundary_right: Option<String>,
) -> Result<RevivaTarget, String> {
    if let (Some(left), Some(right)) = (boundary_left, boundary_right) {
        return Ok(RevivaTarget::Boundary(BoundaryTarget { left, right }));
    }
    if !files.is_empty() {
        return if files.len() == 1 {
            Ok(RevivaTarget::Single(files[0].clone()))
        } else {
            Ok(RevivaTarget::Set(files))
        };
    }

    if !io::stdin().is_terminal() {
        return Err(
            "no explicit target provided. Use --file (or --boundary-left/--boundary-right) in non-interactive shells."
                .to_string(),
        );
    }
    interactive_target_selection(repo, args)
}

fn interactive_target_selection(repo: &Path, args: &[String]) -> Result<RevivaTarget, String> {
    let storage = Storage::new(repo);
    let config = storage
        .load_config()
        .unwrap_or_else(|_| AppConfig::default());
    let repo_config = RepoScanConfig {
        max_file_bytes: config.max_file_bytes,
        include_extensions: None,
    };
    let scan = scan_repository(repo, &repo_config).map_err(|error| error.to_string())?;
    if scan.entries.is_empty() {
        return Err("scan returned no reviewable files".to_string());
    }
    for (index, entry) in scan.entries.iter().take(50).enumerate() {
        println!("[{}] {}", index + 1, entry.path);
    }
    print!("Select file numbers (comma-separated): ");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|error| error.to_string())?;
    let mut selected = Vec::new();
    for token in input.split(',') {
        let index = token.trim().parse::<usize>().map_err(|_| {
            "interactive selection expects comma-separated numeric indices".to_string()
        })?;
        if index == 0 || index > scan.entries.len() {
            return Err("interactive selection index out of range".to_string());
        }
        selected.push(scan.entries[index - 1].path.clone());
    }
    if selected.is_empty() {
        return Err("interactive selection produced no targets".to_string());
    }
    if selected.len() == 1 {
        Ok(RevivaTarget::Single(selected[0].clone()))
    } else {
        let _ = args;
        Ok(RevivaTarget::Set(selected))
    }
}

struct LlamaServerGuard {
    child: Option<Child>,
    stop_on_drop: bool,
}

impl LlamaServerGuard {
    fn noop() -> Self {
        Self {
            child: None,
            stop_on_drop: false,
        }
    }

    fn started(child: Child) -> Self {
        Self {
            child: Some(child),
            stop_on_drop: true,
        }
    }
}

impl Drop for LlamaServerGuard {
    fn drop(&mut self) {
        if !self.stop_on_drop {
            return;
        }
        if let Some(child) = &mut self.child {
            eprintln!("llama-server: stopping local process started by reviva");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LlamaServerAction {
    PolicyManual,
    NonLocalBackendIgnored,
    ReusedActiveServer,
    StartedKeepRunning,
    StartedAndStopOnExit,
    CacheHitBackendSkipped,
}

impl LlamaServerAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PolicyManual => "policy_manual",
            Self::NonLocalBackendIgnored => "non_local_backend_ignored",
            Self::ReusedActiveServer => "reused_active_server",
            Self::StartedKeepRunning => "started_keep_running",
            Self::StartedAndStopOnExit => "started_and_stop_on_exit",
            Self::CacheHitBackendSkipped => "cache_hit_backend_skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LlamaLifecyclePolicy {
    Manual,
    EnsureRunning,
    EnsureRunningAndStop,
}

impl LlamaLifecyclePolicy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::EnsureRunning => "ensure-running",
            Self::EnsureRunningAndStop => "ensure-running-and-stop",
        }
    }
}

fn parse_llama_lifecycle_policy(raw: &str) -> Result<LlamaLifecyclePolicy, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "manual" => Ok(LlamaLifecyclePolicy::Manual),
        "ensure-running" => Ok(LlamaLifecyclePolicy::EnsureRunning),
        "ensure-running-and-stop" => Ok(LlamaLifecyclePolicy::EnsureRunningAndStop),
        other => Err(format!(
            "unsupported llama lifecycle policy: {other}. supported: manual, ensure-running, ensure-running-and-stop"
        )),
    }
}

fn resolve_llama_lifecycle_policy(raw: Option<&str>) -> Result<LlamaLifecyclePolicy, String> {
    match raw {
        Some(value) => parse_llama_lifecycle_policy(value),
        None => Ok(LlamaLifecyclePolicy::EnsureRunningAndStop),
    }
}

struct LlamaServerOutcome {
    guard: LlamaServerGuard,
    action: LlamaServerAction,
}

fn ensure_llama_server(
    storage: &Storage,
    config: &mut AppConfig,
    backend: &BackendSettings,
    policy: LlamaLifecyclePolicy,
) -> Result<LlamaServerOutcome, String> {
    if policy == LlamaLifecyclePolicy::Manual {
        eprintln!("llama-server: lifecycle policy manual, skipping server management");
        return Ok(LlamaServerOutcome {
            guard: LlamaServerGuard::noop(),
            action: LlamaServerAction::PolicyManual,
        });
    }

    if !should_manage_llama_server(&backend.base_url) {
        eprintln!(
            "llama-server: lifecycle policy {} ignored for non-local backend {}",
            policy.as_str(),
            backend.base_url
        );
        return Ok(LlamaServerOutcome {
            guard: LlamaServerGuard::noop(),
            action: LlamaServerAction::NonLocalBackendIgnored,
        });
    }

    if llama_server_health(&backend.base_url) {
        eprintln!("llama-server: active");
        return Ok(LlamaServerOutcome {
            guard: LlamaServerGuard::noop(),
            action: LlamaServerAction::ReusedActiveServer,
        });
    }

    let server_bin = config
        .llama_server_path
        .as_deref()
        .unwrap_or("llama-server")
        .to_string();
    assert_llama_server_installed(&server_bin)?;

    let model_path = resolve_llama_model_path(storage, config)?;
    let (host, port) = parse_http_host_port(&backend.base_url)?;
    let mut child = start_llama_server(
        &server_bin,
        &model_path,
        &host,
        port,
        backend.model.as_deref(),
    )?;
    wait_for_llama_server_ready(
        &backend.base_url,
        &mut child,
        Duration::from_secs(90),
        &model_path,
    )?;
    eprintln!("llama-server: started on {}:{}", host, port);
    if policy == LlamaLifecyclePolicy::EnsureRunning {
        eprintln!("llama-server: leaving process running after review");
        return Ok(LlamaServerOutcome {
            guard: LlamaServerGuard::noop(),
            action: LlamaServerAction::StartedKeepRunning,
        });
    }
    Ok(LlamaServerOutcome {
        guard: LlamaServerGuard::started(child),
        action: LlamaServerAction::StartedAndStopOnExit,
    })
}

fn execute_backend_review(
    storage: &Storage,
    config: &mut AppConfig,
    backend_settings: &BackendSettings,
    policy: LlamaLifecyclePolicy,
    request: &RevivaRequest,
) -> Result<(RevivaResponse, LlamaServerAction, LlamaServerGuard), String> {
    let llama_server_outcome = ensure_llama_server(storage, config, backend_settings, policy)
        .map_err(|error| format!("llama-server preflight failed: {error}"))?;
    let backend = LlamaCompletionBackend::new();
    let response = backend
        .complete(request)
        .map_err(|error| error.to_string())?;
    Ok((
        response,
        llama_server_outcome.action,
        llama_server_outcome.guard,
    ))
}

fn should_manage_llama_server(base_url: &str) -> bool {
    let normalized = base_url.trim().trim_end_matches('/').to_ascii_lowercase();
    normalized == "http://127.0.0.1:8080" || normalized == "http://localhost:8080"
}

fn assert_llama_server_installed(server_bin: &str) -> Result<(), String> {
    let result = Command::new(server_bin)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match result {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(
            "llama-server bulunamadı. Kurulum yap ve PATH'e ekle (örn. `winget install ggml.llamacpp`)."
                .to_string(),
        ),
        Err(error) => Err(format!("llama-server kontrolü başarısız: {error}")),
    }
}

fn resolve_llama_model_path(storage: &Storage, config: &mut AppConfig) -> Result<String, String> {
    if let Some(path) = config.llama_model_path.clone() {
        return normalize_llama_model_path(&path);
    }

    if !io::stdin().is_terminal() {
        return Err(
            "llama-server otomatik başlatma için model yolu gerekli. `--llama-model-path <GGUF|dizin>` ver veya `.reviva/config.toml` içine `llama_model_path` ekle."
                .to_string(),
        );
    }

    print!("llama-server modeli için GGUF dosya/dizin yolu gir: ");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|error| error.to_string())?;
    let model_path = normalize_llama_model_path(input.trim())?;
    config.llama_model_path = Some(model_path.clone());
    storage
        .save_config(config)
        .map_err(|error| format!("model yolu config'e yazılamadı: {error}"))?;
    Ok(model_path)
}

fn normalize_llama_model_path(raw_path: &str) -> Result<String, String> {
    if raw_path.trim().is_empty() {
        return Err("boş model yolu verildi".to_string());
    }
    let path = PathBuf::from(raw_path);
    if !path.exists() {
        return Err(format!("model yolu bulunamadı: {raw_path}"));
    }
    if path.is_file() {
        if is_gguf_file(&path) {
            let canonical = path
                .canonicalize()
                .map_err(|error| format!("model yolu çözümlenemedi: {error}"))?;
            return Ok(canonical.to_string_lossy().to_string());
        }
        return Err("model dosyası .gguf olmalı".to_string());
    }

    let mut candidates = fs::read_dir(&path)
        .map_err(|error| format!("model dizini okunamadı: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| candidate.is_file() && is_gguf_file(candidate))
        .collect::<Vec<_>>();
    candidates.sort();
    let Some(first) = candidates.first() else {
        return Err("model dizininde .gguf dosyası bulunamadı".to_string());
    };
    let canonical = first
        .canonicalize()
        .map_err(|error| format!("model yolu çözümlenemedi: {error}"))?;
    Ok(canonical.to_string_lossy().to_string())
}

fn is_gguf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("gguf"))
        .unwrap_or(false)
}

fn parse_http_host_port(base_url: &str) -> Result<(String, u16), String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let without_scheme = if let Some(value) = trimmed.strip_prefix("http://") {
        value
    } else {
        return Err(format!(
            "llama-server yönetimi sadece http backend için destekleniyor: {base_url}"
        ));
    };
    let authority = without_scheme
        .split('/')
        .next()
        .ok_or_else(|| format!("backend URL authority parse edilemedi: {base_url}"))?;
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| format!("backend URL port içermeli: {base_url}"))?;
    let port = port
        .parse::<u16>()
        .map_err(|error| format!("backend URL port parse edilemedi: {error}"))?;
    Ok((host.to_string(), port))
}

fn start_llama_server(
    server_bin: &str,
    model_path: &str,
    host: &str,
    port: u16,
    model_alias: Option<&str>,
) -> Result<Child, String> {
    eprintln!("llama-server: inactive, starting with model {}", model_path);
    let mut command = Command::new(server_bin);
    command
        .arg("-m")
        .arg(model_path)
        .arg("--host")
        .arg(host)
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(alias) = model_alias {
        command.arg("--alias").arg(alias);
    }
    command
        .spawn()
        .map_err(|error| format!("llama-server başlatılamadı: {error}"))
}

fn wait_for_llama_server_ready(
    base_url: &str,
    child: &mut Child,
    timeout: Duration,
    model_path: &str,
) -> Result<(), String> {
    let start = SystemTime::now();
    loop {
        if llama_server_health(base_url) {
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(format!(
                    "llama-server exited before ready (status: {status}). Check model path and startup flags. model_path={model_path}"
                ));
            }
            Ok(None) => {}
            Err(error) => {
                return Err(format!(
                    "llama-server process status check failed: {error}. model_path={model_path}"
                ));
            }
        }
        let elapsed = SystemTime::now().duration_since(start).unwrap_or_default();
        if elapsed >= timeout {
            return Err(format!(
                "llama-server not ready after {:?}. Check model path, port collisions, and permissions. backend_url={} model_path={}",
                timeout, base_url, model_path
            ));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn llama_server_health(base_url: &str) -> bool {
    let (host, port) = match parse_http_host_port(base_url) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let timeout = Duration::from_millis(800);
    if let Some(status_code) = http_status_probe(&host, port, "/health", timeout) {
        return (200..300).contains(&status_code);
    }
    false
}

fn http_status_probe(host: &str, port: u16, path: &str, timeout: Duration) -> Option<u16> {
    let Ok(mut addrs) = format!("{host}:{port}").to_socket_addrs() else {
        return None;
    };
    let addr = addrs.next()?;
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, timeout) else {
        return None;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut buf = [0_u8; 256];
    let bytes_read = stream.read(&mut buf).ok()?;
    if bytes_read == 0 {
        return None;
    }
    let head = String::from_utf8_lossy(&buf[..bytes_read]);
    let first_line = head.lines().next()?;
    let status_token = first_line.split_whitespace().nth(1)?;
    status_token.parse::<u16>().ok()
}

fn response_content_for_normalization(response: &RevivaResponse) -> String {
    match &response.response_interpretation {
        ResponseInterpretation::Completed { content } => content.clone(),
        ResponseInterpretation::Empty => String::new(),
        ResponseInterpretation::Malformed { reason } => reason.clone(),
    }
}

fn parse_repo_arg(args: &[String]) -> Result<PathBuf, String> {
    let repo = parse_optional_arg(args, "--repo").unwrap_or_else(|| ".".to_string());
    PathBuf::from(repo)
        .canonicalize()
        .map_err(|error| format!("cannot resolve repository path: {error}"))
}

fn resolve_review_mode(
    mode_arg: Option<&str>,
    profile: &ReviewProfileSpec,
) -> Result<(RevivaMode, &'static str), String> {
    if let Some(raw) = mode_arg {
        let parsed = raw
            .parse::<RevivaMode>()
            .map_err(|error| error.to_string())?;
        return Ok((parsed, "cli_mode"));
    }
    if let Ok(parsed) = profile.name.parse::<RevivaMode>() {
        return Ok((parsed, "profile_name"));
    }
    for token in &profile.focus {
        if let Ok(parsed) = token.parse::<RevivaMode>() {
            return Ok((parsed, "profile_focus"));
        }
    }
    Ok((RevivaMode::Contract, "default_contract"))
}

fn resolve_prompt_wrapper(value: Option<&str>) -> Result<PromptWrapper, String> {
    match value {
        Some(raw) => parse_prompt_wrapper(raw).map_err(|error| error.to_string()),
        None => Ok(PromptWrapper::Plain),
    }
}

fn parse_kv_cache_flag(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        other => Err(format!(
            "unsupported kv cache flag: {other}. supported: on|off"
        )),
    }
}

fn parse_kv_slot_id(raw: &str) -> Result<u32, String> {
    raw.trim()
        .parse::<u32>()
        .map_err(|error| format!("invalid --kv-slot value '{raw}': {error}"))
}

struct ResolvedProfile {
    spec: ReviewProfileSpec,
    source: String,
    path: Option<String>,
    hash: String,
}

fn resolve_review_profile(
    repo: &Path,
    profile_arg: Option<&str>,
    profile_file_arg: Option<&str>,
    config_profile: Option<&str>,
    config_profile_file: Option<&str>,
) -> Result<ResolvedProfile, String> {
    if let Some(path_value) = profile_file_arg.or(config_profile_file) {
        let profile_path = resolve_profile_path(repo, path_value)?;
        let profile_content =
            fs::read_to_string(&profile_path).map_err(|error| error.to_string())?;
        let profile_spec =
            parse_review_profile_toml(&profile_content).map_err(|error| error.to_string())?;
        let source = if profile_file_arg.is_some() {
            "cli_profile_file"
        } else {
            "config_profile_file"
        };
        return Ok(ResolvedProfile {
            spec: profile_spec,
            source: source.to_string(),
            path: Some(profile_path.to_string_lossy().to_string()),
            hash: fnv1a64_hex(&profile_content),
        });
    }

    let resolved_name = profile_arg.or(config_profile).unwrap_or("default");
    let source = if profile_arg.is_some() {
        "cli_profile"
    } else if config_profile.is_some() {
        "config_profile"
    } else {
        "default_profile"
    };
    let profile_spec =
        resolve_built_in_review_profile(resolved_name).map_err(|error| error.to_string())?;

    Ok(ResolvedProfile {
        hash: fnv1a64_hex(&profile_spec.canonical_text()),
        spec: profile_spec,
        source: source.to_string(),
        path: None,
    })
}

fn resolve_profile_path(repo: &Path, path_value: &str) -> Result<PathBuf, String> {
    let raw = PathBuf::from(path_value);
    let path = if raw.is_absolute() {
        raw
    } else {
        repo.join(raw)
    };
    if !path.exists() {
        return Err(format!("profile file not found: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("profile path is not a file: {}", path.display()));
    }
    path.canonicalize()
        .map_err(|error| format!("cannot resolve profile file path: {error}"))
}

fn fnv1a64_hex(value: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn build_review_cache_key(request: &RevivaRequest) -> String {
    let mut material = String::new();
    material.push_str("reviva-review-cache-v1\n");
    material.push_str("base_url=");
    material.push_str(&request.backend.base_url);
    material.push('\n');
    material.push_str("model=");
    material.push_str(request.backend.model.as_deref().unwrap_or(""));
    material.push('\n');
    material.push_str("temperature=");
    material.push_str(&request.backend.temperature.to_string());
    material.push('\n');
    material.push_str("max_tokens=");
    material.push_str(&request.backend.max_tokens.to_string());
    material.push('\n');
    material.push_str("timeout_ms=");
    material.push_str(&request.backend.timeout_ms.to_string());
    material.push('\n');
    material.push_str("cache_prompt=");
    material.push_str(if request.backend.cache_prompt {
        "true"
    } else {
        "false"
    });
    material.push('\n');
    material.push_str("slot_id=");
    material.push_str(
        &request
            .backend
            .slot_id
            .map(|value| value.to_string())
            .unwrap_or_default(),
    );
    material.push('\n');
    material.push_str("stop_sequences=");
    material.push_str(&request.backend.stop_sequences.join("\u{1f}"));
    material.push_str("\nprompt=\n");
    material.push_str(&request.prompt);
    fnv1a64_hex(&material)
}

fn parse_required_arg(args: &[String], flag: &str) -> Result<String, String> {
    parse_optional_arg(args, flag).ok_or_else(|| format!("missing required argument: {flag}"))
}

fn parse_optional_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|chunk| chunk[0] == flag)
        .map(|chunk| chunk[1].clone())
}

fn parse_repeat_arg(args: &[String], flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0_usize;
    while index < args.len() {
        if args[index] == flag && index + 1 < args.len() {
            values.push(args[index + 1].clone());
            index += 1;
        }
        index += 1;
    }
    values
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|value| value == flag)
}

fn current_timestamp() -> String {
    if let Ok(value) = env::var("REVIVA_TEST_TIMESTAMP") {
        return value;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

fn session_id() -> String {
    if let Ok(value) = env::var("REVIVA_TEST_SESSION_ID") {
        return value;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("session-{now}")
}

fn print_usage() {
    println!("reviva scan [--repo PATH]");
    println!("reviva review --repo PATH [--mode MODE] [--profile NAME] [--profile-file PATH] [--file PATH]... [--boundary-left PATH --boundary-right PATH] [--note TEXT] [--prompt-wrapper plain|qwen-chatml] [--kv-cache on|off] [--kv-slot SLOT_ID] [--llama-lifecycle manual|ensure-running|ensure-running-and-stop] [--preview-only] [--llama-model-path PATH_OR_DIR] [--llama-server-path PATH]");
    println!("reviva set save --repo PATH --name NAME --file PATH...");
    println!("reviva set load --repo PATH --name NAME");
    println!("reviva set list --repo PATH");
    println!("reviva session list --repo PATH");
    println!("reviva session show --repo PATH --id SESSION_ID");
    println!("reviva findings list --repo PATH [--session SESSION_ID]");
    println!("reviva export --repo PATH --session SESSION_ID [--format md|json] [--output PATH]");
}

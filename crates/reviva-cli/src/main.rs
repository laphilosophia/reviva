use reviva_backend::{CompletionBackend, LlamaCompletionBackend};
use reviva_core::{
    BackendSettings, BoundaryTarget, NamedSet, ResponseInterpretation, RevivaMode, RevivaRequest,
    RevivaTarget, Session,
};
use reviva_export::{export_session_json, export_session_markdown};
use reviva_prompts::{build_prompt, normalize_findings, PromptBuildConfig, PromptFile};
use reviva_repo::{estimated_target_tokens, load_target_files, scan_repository, RepoScanConfig};
use reviva_storage::{AppConfig, Storage};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    let mode = parse_mode_arg(args)?;
    let note = parse_optional_arg(args, "--note");
    let model = parse_optional_arg(args, "--model");
    let preview_only = has_flag(args, "--preview-only");
    let files = parse_repeat_arg(args, "--file");
    let boundary_left = parse_optional_arg(args, "--boundary-left");
    let boundary_right = parse_optional_arg(args, "--boundary-right");

    let storage = Storage::new(&repo);
    storage.init().map_err(|error| error.to_string())?;
    let config = storage.load_config().map_err(|error| error.to_string())?;
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
        &target,
        &prompt_files,
        note.as_deref(),
        &PromptBuildConfig {
            estimated_prompt_tokens: config.estimated_prompt_tokens,
        },
    )
    .map_err(|error| error.to_string())?;

    println!("--- PROMPT PREVIEW START ---");
    println!("{}", prompt_result.prompt);
    println!("--- PROMPT PREVIEW END ---");
    if preview_only {
        return Ok(());
    }

    let backend_settings = BackendSettings {
        base_url: config.backend_url,
        model: model.or(config.model),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        timeout_ms: config.timeout_ms,
        stop_sequences: config.stop_sequences,
    };
    let request = RevivaRequest {
        backend: backend_settings.clone(),
        prompt: prompt_result.prompt.clone(),
    };
    let backend = LlamaCompletionBackend::new();
    let response = backend
        .complete(&request)
        .map_err(|error| error.to_string())?;

    let model_output = match &response.response_interpretation {
        ResponseInterpretation::Completed { content } => content.clone(),
        ResponseInterpretation::Empty => String::new(),
        ResponseInterpretation::Malformed { reason } => reason.clone(),
    };
    let session_id = session_id();
    let (normalization_state, mut findings) = normalize_findings(
        &session_id,
        mode,
        &target.as_paths().join(","),
        &model_output,
    );
    for finding in &mut findings {
        finding.normalization_state = normalization_state;
    }

    let session = Session {
        id: session_id.clone(),
        repository_root: repo.display().to_string(),
        review_mode: mode,
        selected_target: target,
        prompt_preview: prompt_result.prompt.clone(),
        prompt_sent: prompt_result.prompt,
        backend: backend_settings,
        response,
        findings,
        created_at: current_timestamp(),
        warnings: vec![format!(
            "estimated_token_budget={}",
            estimated_target_tokens(&loaded, note.as_deref())
        )],
    };
    let session_path = storage
        .save_session(&session)
        .map_err(|error| error.to_string())?;

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
            println!("created_at: {}", session.created_at);
            println!("findings: {}", session.findings.len());
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

fn parse_repo_arg(args: &[String]) -> Result<PathBuf, String> {
    let repo = parse_optional_arg(args, "--repo").unwrap_or_else(|| ".".to_string());
    PathBuf::from(repo)
        .canonicalize()
        .map_err(|error| format!("cannot resolve repository path: {error}"))
}

fn parse_mode_arg(args: &[String]) -> Result<RevivaMode, String> {
    let mode = parse_required_arg(args, "--mode")?;
    mode.parse::<RevivaMode>()
        .map_err(|error| error.to_string())
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
    println!("reviva review --repo PATH --mode MODE [--file PATH]... [--boundary-left PATH --boundary-right PATH] [--note TEXT] [--preview-only]");
    println!("reviva set save --repo PATH --name NAME --file PATH...");
    println!("reviva set load --repo PATH --name NAME");
    println!("reviva set list --repo PATH");
    println!("reviva session list --repo PATH");
    println!("reviva session show --repo PATH --id SESSION_ID");
    println!("reviva findings list --repo PATH [--session SESSION_ID]");
    println!("reviva export --repo PATH --session SESSION_ID [--format md|json] [--output PATH]");
}

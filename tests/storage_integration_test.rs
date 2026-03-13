use reviva::core::{
    BackendSettings, Confidence, Finding, NormalizationState, ProfileMetadata,
    ResponseInterpretation, RevivaMode, RevivaResponse, RevivaTarget, Session, Severity,
    SeverityOrigin,
};
use reviva::storage::{AppConfig, Storage, StorageError};
use tempfile::TempDir;

fn fixture_session() -> Session {
    Session {
        id: "session-1".to_string(),
        repository_root: "/repo".to_string(),
        review_mode: RevivaMode::Contract,
        selected_target: RevivaTarget::Single("src/main.rs".to_string()),
        prompt_preview: "PROMPT".to_string(),
        prompt_sent: "PROMPT".to_string(),
        backend: BackendSettings {
            base_url: "http://127.0.0.1:8080".to_string(),
            model: Some("model".to_string()),
            temperature: 0.1,
            max_tokens: 256,
            timeout_ms: 1000,
            stop_sequences: vec![],
            cache_prompt: false,
            slot_id: None,
        },
        response: RevivaResponse {
            status_code: Some(200),
            raw_http_body: "{\"content\":\"ok\"}".to_string(),
            response_interpretation: ResponseInterpretation::Completed {
                content: "SUMMARY:\n- ok".to_string(),
            },
        },
        findings: vec![Finding {
            id: "f1".to_string(),
            session_id: "session-1".to_string(),
            review_mode: RevivaMode::Contract,
            target: "src/main.rs".to_string(),
            summary: "Missing timeout".to_string(),
            why_it_matters: Some("can hang".to_string()),
            severity: Some(Severity::High),
            severity_origin: SeverityOrigin::ModelLabeled,
            confidence: Confidence::Medium,
            risk_class: None,
            action: Some("add timeout".to_string()),
            status: None,
            location_hint: Some("src/main.rs".to_string()),
            evidence_text: Some("client.call()".to_string()),
            raw_labels: vec!["high".to_string()],
            normalization_state: NormalizationState::Structured,
        }],
        profile: ProfileMetadata {
            name: "default".to_string(),
            source: "built_in".to_string(),
            path: None,
            hash: "abc123".to_string(),
        },
        created_at: "1700000000".to_string(),
        warnings: vec!["estimated_token_budget=200".to_string()],
    }
}

#[test]
fn roundtrip_session_config_and_set() {
    let temp = TempDir::new().expect("tempdir");
    let storage = Storage::new(temp.path());
    storage.init().expect("init");

    let config = AppConfig::default();
    storage.save_config(&config).expect("save config");
    let loaded_config = storage.load_config().expect("load config");
    assert_eq!(config.backend_url, loaded_config.backend_url);
    assert_eq!(
        config.estimated_prompt_tokens,
        loaded_config.estimated_prompt_tokens
    );
    assert_eq!(
        config.review_profile_file,
        loaded_config.review_profile_file
    );
    assert_eq!(config.llama_kv_cache, loaded_config.llama_kv_cache);
    assert_eq!(config.llama_slot_id, loaded_config.llama_slot_id);
    assert_eq!(config.include, loaded_config.include);
    assert_eq!(config.exclude, loaded_config.exclude);

    let session = fixture_session();
    storage.save_session(&session).expect("save session");
    let loaded_session = storage.load_session(&session.id).expect("load session");
    assert_eq!(loaded_session.id, session.id);
    assert_eq!(loaded_session.prompt_sent, session.prompt_sent);
    assert_eq!(loaded_session.findings.len(), 1);
    assert!(!loaded_session.backend.cache_prompt);
    assert_eq!(loaded_session.backend.slot_id, None);
    assert!(storage
        .root()
        .join("findings")
        .join("main-rs-1.json")
        .exists());
    assert!(storage.root().join("findings").join("index.json").exists());

    let set = reviva::core::NamedSet {
        name: "critical".to_string(),
        paths: vec!["src/main.rs".to_string()],
    };
    storage.save_named_set(&set).expect("save set");
    let loaded_set = storage.load_named_set("critical").expect("load set");
    assert_eq!(loaded_set.paths, set.paths);
    let sets_index =
        std::fs::read_to_string(storage.root().join("sets").join("index.json")).expect("index");
    assert!(sets_index.contains("critical"));
}

#[test]
fn list_findings_from_session_truth_source() {
    let temp = TempDir::new().expect("tempdir");
    let storage = Storage::new(temp.path());
    storage.init().expect("init");

    let session = fixture_session();
    storage.save_session(&session).expect("save session");
    let findings = storage.list_findings(Some("session-1")).expect("findings");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].summary, "Missing timeout");
}

#[test]
fn corrupted_session_file_returns_deserialize_error() {
    let temp = TempDir::new().expect("tempdir");
    let storage = Storage::new(temp.path());
    storage.init().expect("init");
    let path = storage.root().join("sessions").join("bad.json");
    std::fs::write(path, "{ not valid json }").expect("write");

    let error = storage.load_session("bad").expect_err("must fail");
    assert!(matches!(error, StorageError::Deserialize(_)));
}

#[test]
fn review_cache_session_link_roundtrip() {
    let temp = TempDir::new().expect("tempdir");
    let storage = Storage::new(temp.path());
    storage.init().expect("init");

    storage
        .save_review_cache_session_id("key-1", "session-123")
        .expect("save cache link");

    let cached = storage
        .load_review_cache_session_id("key-1")
        .expect("load cache link");
    assert_eq!(cached.as_deref(), Some("session-123"));

    let missing = storage
        .load_review_cache_session_id("missing")
        .expect("load missing");
    assert_eq!(missing, None);
}

#[test]
fn config_reference_table_stays_in_sync_with_app_config_fields() {
    use std::collections::BTreeSet;

    let docs = include_str!("../docs/config-reference.md");
    let mut fields = Vec::<String>::new();
    let mut in_table = false;

    for line in docs.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("| Field | Type |") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if trimmed.is_empty() || !trimmed.starts_with('|') {
            break;
        }
        if trimmed.starts_with("|---") {
            continue;
        }
        let columns = trimmed.split('|').map(str::trim).collect::<Vec<_>>();
        if columns.len() < 3 {
            continue;
        }
        let field = columns[1];
        if let Some(name) = field
            .strip_prefix('`')
            .and_then(|value| value.strip_suffix('`'))
        {
            fields.push(name.to_string());
        }
    }

    assert!(
        !fields.is_empty(),
        "docs/config-reference.md must include the config field table"
    );

    let documented = fields.into_iter().collect::<BTreeSet<_>>();
    let expected = [
        "backend_url",
        "model",
        "prompt_wrapper",
        "llama_lifecycle_policy",
        "llama_kv_cache",
        "llama_slot_id",
        "review_profile",
        "review_profile_file",
        "llama_server_path",
        "llama_model_path",
        "timeout_ms",
        "max_tokens",
        "temperature",
        "stop_sequences",
        "max_file_bytes",
        "estimated_prompt_tokens",
        "include",
        "exclude",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();

    if documented != expected {
        let missing = expected
            .difference(&documented)
            .cloned()
            .collect::<Vec<_>>();
        let extra = documented
            .difference(&expected)
            .cloned()
            .collect::<Vec<_>>();
        panic!(
            "docs/config-reference.md field drift detected; missing={missing:?}, extra={extra:?}"
        );
    }
}

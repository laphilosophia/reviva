use httpmock::{Method::POST, MockServer};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn run_cmd(repo: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run command");
    if !output.status.success() {
        panic!(
            "command failed: {:?}\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

#[test]
fn end_to_end_cli_flow_and_prompt_inspectability() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).header("content-type", "application/json").body(
            r#"{
                "content": "SUMMARY:\n- ok\nFINDINGS:\n- summary: Missing guard\nseverity: high\nconfidence: medium\nlocation: src/main.rs\nevidence: no check\nwhy: crash risk\naction: add guard\n"
            }"#,
        );
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"test\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");

    let scan_output = run_cmd(
        temp.path(),
        &["scan", "--repo", temp.path().to_str().expect("repo str")],
    );
    assert!(scan_output.contains("src/main.rs"));

    let review_output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "contract",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-test")
        .env("REVIVA_TEST_TIMESTAMP", "1700000000")
        .current_dir(temp.path())
        .output()
        .expect("run review");
    assert!(
        review_output.status.success(),
        "review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&review_output.stdout),
        String::from_utf8_lossy(&review_output.stderr)
    );
    let review_text = String::from_utf8(review_output.stdout).expect("utf8");
    assert!(review_text.contains("PROMPT PREVIEW START"));
    assert!(review_text.contains("session saved:"));

    let session_show = run_cmd(
        temp.path(),
        &[
            "session",
            "show",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--id",
            "session-test",
        ],
    );
    assert!(session_show.contains("findings.total: 1"));

    let findings_output = run_cmd(
        temp.path(),
        &[
            "findings",
            "list",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--session",
            "session-test",
        ],
    );
    assert!(findings_output.contains("structured"));
    assert!(findings_output.contains("model_labeled"));

    let triage_output = run_cmd(
        temp.path(),
        &[
            "findings",
            "list",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--session",
            "session-test",
            "--triage",
        ],
    );
    assert!(triage_output.contains("triage.total_findings: 1"));
    assert!(triage_output.contains("triage.by_state: structured=1 partial=0 raw_only=0"));
    assert!(triage_output.contains("triage.repeated_summaries: none"));

    let export_output = run_cmd(
        temp.path(),
        &[
            "export",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--session",
            "session-test",
            "--format",
            "json",
        ],
    );
    assert!(export_output.contains("exported:"));

    let session_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-test.json");
    let session_json = fs::read_to_string(&session_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_json).expect("valid session json");
    assert_eq!(parsed["prompt_preview"], parsed["prompt_sent"]);
    assert!(!parsed["repository_root"]
        .as_str()
        .unwrap_or_default()
        .contains('\\'));
    let warnings = parsed["warnings"].as_array().expect("warnings array");
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("mode_source=cli_mode")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("llama_server_action=non_local_backend_ignored")));
}

#[test]
fn exact_hit_review_cache_skips_second_backend_call() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    let completion = server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).header("content-type", "application/json").body(
            r#"{
                "content": "SUMMARY:\n- ok\nFINDINGS:\n- summary: Missing guard\nseverity: high\nconfidence: medium\nlocation: src/main.rs\nevidence: no check\nwhy: crash risk\naction: add guard\n"
            }"#,
        );
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"test\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");

    let first = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "contract",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-cache-1")
        .env("REVIVA_TEST_TIMESTAMP", "1700000000")
        .current_dir(temp.path())
        .output()
        .expect("run first review");
    assert!(
        first.status.success(),
        "first review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    let second = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "contract",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-cache-2")
        .env("REVIVA_TEST_TIMESTAMP", "1700000001")
        .current_dir(temp.path())
        .output()
        .expect("run second review");
    assert!(
        second.status.success(),
        "second review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    assert_eq!(completion.hits(), 1, "second review must be cache hit");

    let session_two_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-cache-2.json");
    let session_two_json = fs::read_to_string(&session_two_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_two_json).expect("valid session json");
    let warnings = parsed["warnings"].as_array().expect("warnings array");
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("review_cache=hit")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("llama_server_action=cache_hit_backend_skipped")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("review_cache_source=session-cache-1")));
}

#[test]
fn qwen_chatml_wrapper_keeps_preview_equal_to_sent_prompt() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).header("content-type", "application/json").body(
            r#"{
                "content": "SUMMARY:\n- ok\nFINDINGS:\n- summary: Guard missing\nseverity: medium\nconfidence: low\nlocation: src/main.rs\nevidence: no condition\nwhy: drift risk\naction: add guard\n"
            }"#,
        );
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"qwen2.5-coder-7b-instruct\"\nprompt_wrapper = \"qwen-chatml\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");

    let review_output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "contract",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-wrapper")
        .env("REVIVA_TEST_TIMESTAMP", "1700000000")
        .current_dir(temp.path())
        .output()
        .expect("run review");
    assert!(
        review_output.status.success(),
        "review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&review_output.stdout),
        String::from_utf8_lossy(&review_output.stderr)
    );

    let session_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-wrapper.json");
    let session_json = fs::read_to_string(&session_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_json).expect("valid session json");
    assert_eq!(parsed["prompt_preview"], parsed["prompt_sent"]);
    assert!(parsed["prompt_sent"]
        .as_str()
        .expect("prompt")
        .contains("<|im_start|>system"));
    assert!(parsed["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .any(|value| value.as_str() == Some("prompt_wrapper=qwen-chatml")));
}

#[test]
fn review_without_mode_uses_profile_name_mapping() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).header("content-type", "application/json").body(
            r#"{
                "content": "SUMMARY:\n- ok\nFINDINGS:\n- summary: Launch check\nseverity: medium\nconfidence: low\nlocation: src/main.rs\nevidence: startup path\nwhy: release risk\naction: verify\n"
            }"#,
        );
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"test\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");

    let review_output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--profile",
            "launch-readiness",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-mode-profile")
        .env("REVIVA_TEST_TIMESTAMP", "1700000000")
        .current_dir(temp.path())
        .output()
        .expect("run review");
    assert!(
        review_output.status.success(),
        "review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&review_output.stdout),
        String::from_utf8_lossy(&review_output.stderr)
    );

    let session_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-mode-profile.json");
    let session_json = fs::read_to_string(&session_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_json).expect("valid session json");
    assert_eq!(parsed["review_mode"], "launch-readiness");
    assert!(parsed["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .any(|value| value.as_str() == Some("mode_source=profile_name")));
}

#[test]
fn review_profile_is_visible_in_prompt_and_session_warning() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).header("content-type", "application/json").body(
            r#"{
                "content": "SUMMARY:\n- ok\nFINDINGS:\n- summary: Boundary issue\nseverity: medium\nconfidence: low\nlocation: src/main.rs\nevidence: weak guard\nwhy: drift risk\naction: tighten boundary\n"
            }"#,
        );
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"test\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");

    let review_output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "launch-readiness",
            "--profile",
            "launch-readiness",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-profile")
        .env("REVIVA_TEST_TIMESTAMP", "1700000000")
        .current_dir(temp.path())
        .output()
        .expect("run review");

    assert!(
        review_output.status.success(),
        "review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&review_output.stdout),
        String::from_utf8_lossy(&review_output.stderr)
    );
    let review_text = String::from_utf8(review_output.stdout).expect("utf8");
    assert!(review_text.contains("Profile: launch-readiness"));

    let session_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-profile.json");
    let session_json = fs::read_to_string(&session_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_json).expect("valid session json");
    assert!(parsed["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .any(|value| value.as_str() == Some("profile=launch-readiness")));
}

#[test]
fn review_profile_file_is_resolved_and_persisted_in_session_metadata() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).header("content-type", "application/json").body(
            r#"{
                "content": "SUMMARY:\n- ok\nFINDINGS:\n- summary: Startup drift\nseverity: high\nconfidence: low\nrisk_class: operator-trust\nlocation: src/main.rs\nevidence: implicit default\nwhy: operator confusion\naction: make behavior explicit\n"
            }"#,
        );
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"test\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");

    let profile_path = temp.path().join("review-profile.toml");
    fs::write(
        &profile_path,
        r#"
name = "tracehound-launch"
goal = "Launch review for security-sensitive runtime"
global_rules = ["No code generation", "Mark uncertainty explicitly"]
focus = ["failure-semantics", "operator-trust"]
severity_scale = ["release-blocker", "pre-launch-fix", "post-launch-watch"]
confidence_scale = ["definite", "likely", "uncertain"]
risk_classes = ["correctness", "security", "operator-trust"]
"#,
    )
    .expect("profile");

    let review_output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "launch-readiness",
            "--profile-file",
            "review-profile.toml",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-profile-file")
        .env("REVIVA_TEST_TIMESTAMP", "1700000000")
        .current_dir(temp.path())
        .output()
        .expect("run review");

    assert!(
        review_output.status.success(),
        "review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&review_output.stdout),
        String::from_utf8_lossy(&review_output.stderr)
    );
    let review_text = String::from_utf8(review_output.stdout).expect("utf8");
    assert!(review_text.contains("Profile: tracehound-launch"));

    let session_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-profile-file.json");
    let session_json = fs::read_to_string(&session_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_json).expect("valid session json");
    assert_eq!(parsed["profile"]["name"], "tracehound-launch");
    assert_eq!(parsed["profile"]["source"], "cli_profile_file");
    assert_eq!(parsed["prompt_preview"], parsed["prompt_sent"]);
    assert_eq!(
        parsed["profile"]["path"].as_str().expect("profile path"),
        "review-profile.toml"
    );
}

#[test]
fn normalization_reason_tags_are_persisted_when_raw_only() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"content":"Model returned free text only."}"#);
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"test\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");

    let review_output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "contract",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-raw-only")
        .env("REVIVA_TEST_TIMESTAMP", "1700000000")
        .current_dir(temp.path())
        .output()
        .expect("run review");

    assert!(
        review_output.status.success(),
        "review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&review_output.stdout),
        String::from_utf8_lossy(&review_output.stderr)
    );

    let session_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-raw-only.json");
    let session_json = fs::read_to_string(&session_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_json).expect("valid session json");
    let warnings = parsed["warnings"].as_array().expect("warnings array");
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("normalization_reason=missing_findings_section")));
}

#[test]
fn review_incremental_from_git_ref_selects_changed_files() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "initial"]);
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    let completion = server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).header("content-type", "application/json").body(
            r#"{
                "content": "SUMMARY:\n- ok\nFINDINGS:\n- summary: Incremental finding\nseverity: medium\nconfidence: high\nlocation: src/main.rs\nevidence: changed line\nwhy: behavior changed\naction: verify\n"
            }"#,
        );
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"test\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");

    let review_output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "contract",
            "--incremental-from",
            "HEAD",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-incremental")
        .env("REVIVA_TEST_TIMESTAMP", "1700000002")
        .current_dir(temp.path())
        .output()
        .expect("run review");
    assert!(
        review_output.status.success(),
        "review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&review_output.stdout),
        String::from_utf8_lossy(&review_output.stderr)
    );
    assert_eq!(completion.hits(), 1);

    let session_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-incremental.json");
    let session_json = fs::read_to_string(&session_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_json).expect("valid session json");
    assert_eq!(parsed["selected_target"]["kind"], "single");
    assert_eq!(parsed["selected_target"]["path"], "src/main.rs");
    assert!(parsed["prompt_preview"]
        .as_str()
        .expect("prompt preview")
        .contains("REVIVA INCREMENTAL DIFF"));
    assert!(parsed["prompt_preview"]
        .as_str()
        .expect("prompt preview")
        .contains("@@"));
    let warnings = parsed["warnings"].as_array().expect("warnings array");
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("incremental_from=HEAD")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("incremental_file_count=1")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("incremental_scope=diff_hunks")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("incremental_context_lines=3")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("incremental_fallback_full_file_count=0")));
}

#[test]
fn incremental_from_rejects_explicit_file_combination() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "contract",
            "--file",
            "src/main.rs",
            "--incremental-from",
            "HEAD",
        ])
        .current_dir(temp.path())
        .output()
        .expect("run review");
    assert!(
        !output.status.success(),
        "command must fail when incremental and explicit file are combined"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(
        "cannot combine --incremental-from with --file or --boundary-left/--boundary-right"
    ));
}

#[test]
fn profile_limits_and_cli_overrides_affect_backend_and_findings() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("write");

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/completion");
        then.status(200).header("content-type", "application/json").body(
            r#"{
                "content": "SUMMARY:\n- ok\nFINDINGS:\n- summary: First\nseverity: high\nconfidence: high\nlocation: src/main.rs\nevidence: a\nwhy: a\naction: a\n- summary: Second\nseverity: medium\nconfidence: medium\nlocation: src/main.rs\nevidence: b\nwhy: b\naction: b\n"
            }"#,
        );
    });

    fs::create_dir_all(temp.path().join(".reviva")).expect("mkdir");
    fs::write(
        temp.path().join(".reviva/config.toml"),
        format!(
            "backend_url = \"{}\"\nmodel = \"test\"\ntimeout_ms = 10000\nmax_tokens = 512\ntemperature = 0.1\nstop_sequences = []\nmax_file_bytes = 262144\nestimated_prompt_tokens = 16000\n",
            server.url("")
        ),
    )
    .expect("config");
    fs::write(
        temp.path().join("profile.toml"),
        r#"
name = "limited-profile"
goal = "Limit output"
severity_scale = ["critical", "high", "medium", "low", "unknown"]
confidence_scale = ["high", "medium", "low", "unknown"]
risk_classes = ["correctness", "security", "unknown"]

[limits]
max_findings = 2
max_output_tokens = 128
"#,
    )
    .expect("profile");

    let review_output = Command::new(env!("CARGO_BIN_EXE_reviva"))
        .args([
            "review",
            "--repo",
            temp.path().to_str().expect("repo str"),
            "--mode",
            "contract",
            "--profile-file",
            "profile.toml",
            "--max-findings",
            "1",
            "--max-output-tokens",
            "42",
            "--file",
            "src/main.rs",
        ])
        .env("REVIVA_TEST_SESSION_ID", "session-profile-limits")
        .env("REVIVA_TEST_TIMESTAMP", "1700000003")
        .current_dir(temp.path())
        .output()
        .expect("run review");
    assert!(
        review_output.status.success(),
        "review failed stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&review_output.stdout),
        String::from_utf8_lossy(&review_output.stderr)
    );

    let session_json_path = temp
        .path()
        .join(".reviva")
        .join("sessions")
        .join("session-profile-limits.json");
    let session_json = fs::read_to_string(&session_json_path).expect("session json");
    let parsed: Value = serde_json::from_str(&session_json).expect("valid session json");
    assert_eq!(parsed["backend"]["max_tokens"], 42);
    assert_eq!(parsed["findings"].as_array().expect("findings").len(), 1);
    let warnings = parsed["warnings"].as_array().expect("warnings array");
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("profile_max_findings=1")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("profile_max_output_tokens=42")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("effective_max_tokens=42")));
    assert!(warnings
        .iter()
        .any(|value| value.as_str() == Some("normalization_reason=max_findings_truncated")));
}

fn init_git_repo(root: &Path) {
    git(root, &["init"]);
    git(root, &["config", "user.email", "reviva@example.com"]);
    git(root, &["config", "user.name", "reviva-test"]);
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed stdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

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
    assert!(session_show.contains("findings: 1"));

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
    assert!(parsed["profile"]["path"]
        .as_str()
        .expect("profile path")
        .contains("review-profile.toml"));
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

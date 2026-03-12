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
}

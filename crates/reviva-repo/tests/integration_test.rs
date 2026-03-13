use reviva_core::RevivaTarget;
use reviva_repo::{
    load_incremental_target_files, load_target_files, resolve_incremental_target, scan_repository,
    RepoError, RepoScanConfig,
};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn traversal_respects_ignore_and_excludes_binary() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join(".gitignore"), "ignored.rs\n").expect("write ignore");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(temp.path().join("src").join("main.rs"), "fn main() {}\n").expect("write");
    fs::write(temp.path().join("ignored.rs"), "should not appear").expect("write");
    fs::write(temp.path().join("binary.bin"), [0_u8, 159, 146, 150]).expect("write");

    let result = scan_repository(temp.path(), &RepoScanConfig::default()).expect("scan");
    let paths = result
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"src/main.rs".to_string()));
    assert!(!paths.contains(&"ignored.rs".to_string()));
    assert!(!paths.iter().any(|path| path.ends_with(".bin")));
}

#[test]
fn extension_filter_and_heuristic_are_deterministic() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(temp.path().join("src").join("auth.rs"), "fn auth() {}\n").expect("write");
    fs::write(temp.path().join("src").join("notes.md"), "# docs\n").expect("write");

    let config = RepoScanConfig {
        max_file_bytes: 1_000_000,
        include_extensions: Some(vec!["rs".to_string()]),
        include: Vec::new(),
        exclude: Vec::new(),
    };
    let first = scan_repository(temp.path(), &config).expect("scan1");
    let second = scan_repository(temp.path(), &config).expect("scan2");
    assert_eq!(first.entries.len(), 1);
    assert_eq!(first.entries[0].path, "src/auth.rs");
    assert_eq!(
        first.entries[0].review_priority_heuristic,
        second.entries[0].review_priority_heuristic
    );
}

#[test]
fn oversized_target_triggers_file_level_refusal() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(temp.path().join("src").join("large.rs"), "x".repeat(2048)).expect("write");

    let result = load_target_files(
        temp.path(),
        &RevivaTarget::Single("src/large.rs".to_string()),
        &RepoScanConfig {
            max_file_bytes: 128,
            include_extensions: None,
            include: Vec::new(),
            exclude: Vec::new(),
        },
    );
    match result.expect_err("must refuse oversized file") {
        RepoError::FileTooLarge {
            path,
            file_size,
            max_file_bytes,
        } => {
            assert_eq!(path, "src/large.rs");
            assert!(file_size > max_file_bytes);
        }
        error => panic!("unexpected error: {error:?}"),
    }
}

#[test]
fn incremental_target_resolves_changed_reviewable_files() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(temp.path().join("src").join("main.rs"), "fn main() {}\n").expect("write");
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "initial"]);

    fs::write(
        temp.path().join("src").join("main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .expect("rewrite");

    let target = resolve_incremental_target(temp.path(), "HEAD", &RepoScanConfig::default())
        .expect("resolve incremental target");
    match target {
        RevivaTarget::Single(path) => assert_eq!(path, "src/main.rs"),
        other => panic!("unexpected target: {other:?}"),
    }
}

#[test]
fn incremental_target_errors_when_no_changes() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(temp.path().join("src").join("main.rs"), "fn main() {}\n").expect("write");
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "initial"]);

    let error = resolve_incremental_target(temp.path(), "HEAD", &RepoScanConfig::default())
        .expect_err("should fail with no changes");
    assert!(matches!(
        error,
        RepoError::NoReviewableChangedFiles { from } if from == "HEAD"
    ));
}

#[test]
fn incremental_loader_prefers_diff_hunks_with_context() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(temp.path().join("src").join("main.rs"), "fn main() {}\n").expect("write");
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "initial"]);

    fs::write(
        temp.path().join("src").join("main.rs"),
        "fn main() {\n    println!(\"changed\");\n}\n",
    )
    .expect("rewrite");
    let target = RevivaTarget::Single("src/main.rs".to_string());
    let loaded =
        load_incremental_target_files(temp.path(), &target, &RepoScanConfig::default(), "HEAD", 3)
            .expect("load incremental files");

    assert!(loaded.fallback_full_files.is_empty());
    assert_eq!(loaded.files.len(), 1);
    assert!(loaded.files[0].content.contains("REVIVA INCREMENTAL DIFF"));
    assert!(loaded.files[0].content.contains("@@"));
}

#[test]
fn incremental_loader_falls_back_to_full_file_when_diff_is_empty() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    fs::create_dir_all(temp.path().join("src")).expect("mkdir");
    fs::write(temp.path().join("src").join("main.rs"), "fn main() {}\n").expect("write");
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "initial"]);

    let target = RevivaTarget::Single("src/main.rs".to_string());
    let loaded =
        load_incremental_target_files(temp.path(), &target, &RepoScanConfig::default(), "HEAD", 3)
            .expect("load incremental files");

    assert_eq!(loaded.fallback_full_files, vec!["src/main.rs".to_string()]);
    assert_eq!(loaded.files.len(), 1);
    assert_eq!(loaded.files[0].content, "fn main() {}\n");
}

fn init_git_repo(root: &std::path::Path) {
    git(root, &["init"]);
    git(root, &["config", "user.email", "reviva@example.com"]);
    git(root, &["config", "user.name", "reviva-test"]);
}

fn git(root: &std::path::Path, args: &[&str]) {
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

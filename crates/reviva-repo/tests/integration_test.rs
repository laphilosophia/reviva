use reviva_core::RevivaTarget;
use reviva_repo::{load_target_files, scan_repository, RepoError, RepoScanConfig};
use std::fs;
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

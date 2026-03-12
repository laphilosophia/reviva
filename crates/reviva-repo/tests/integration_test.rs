//! Integration tests for reviva-repo.
//!
//! These tests use a small fixture repository (no live model required).
//!
//! Coverage targets (populated as implementation progresses):
//!   - recursive traversal respects `.gitignore`
//!   - binary files are excluded by default
//!   - extension filtering works correctly
//!   - approximate token estimation is bounded
//!   - heuristic risk score is deterministic for the same input
//!   - selection state round-trips correctly
//!   - oversized target triggers refusal, not silent truncation

// Placeholder — will be filled as reviva-repo is implemented.
#[test]
fn placeholder_traversal() {
    // TODO: create a tempdir fixture with .gitignore, binary file,
    //       and source files; assert only source files are returned.
    assert!(true, "placeholder — replace with real fixture test");
}

//! Snapshot tests for reviva-prompts.
//!
//! Every review mode must produce a stable prompt shape.
//! Regressions in prompt text are caught by `insta` snapshot diffing.
//!
//! To review and accept a changed snapshot:
//!   cargo insta review
//!
//! Coverage targets (populated as reviva-prompts is implemented):
//!   - contract mode prompt contains expected sections
//!   - boundary mode prompt contains expected sections
//!   - boundedness mode prompt contains expected sections
//!   - failure-semantics mode prompt contains expected sections
//!   - performance-risk mode prompt contains expected sections
//!   - memory-risk mode prompt contains expected sections
//!   - operator-correctness mode prompt contains expected sections
//!   - launch-readiness mode prompt contains expected sections
//!   - maintainability mode prompt contains expected sections
//!   - user note is included when provided
//!   - oversized target causes prompt refusal, not silent truncation

// Placeholder — will be filled as reviva-prompts is implemented.
#[test]
fn placeholder_snapshot() {
    // TODO: build each mode's prompt from a fixture target and call
    //       insta::assert_snapshot!(...) to lock the output shape.
    insta::assert_snapshot!("placeholder", "placeholder prompt output");
}

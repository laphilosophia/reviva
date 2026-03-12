//! Integration tests for reviva-storage.
//!
//! Tests use tempfile directories — no persistent state, no side effects.
//!
//! Coverage targets (populated as reviva-storage is implemented):
//!   - session save and load round-trip produces identical structs
//!   - finding save and list returns correct results
//!   - named set save and reload round-trips correctly
//!   - config save and load round-trips correctly
//!   - missing storage directory returns StorageError, not panic
//!   - corrupted session file returns StorageError with context

// Placeholder — will be filled as reviva-storage is implemented.
#[test]
fn placeholder_storage_roundtrip() {
    // TODO: use tempfile::TempDir, save a fixture Session, load it back,
    //       assert equality.
    assert!(true, "placeholder — replace with real round-trip test");
}

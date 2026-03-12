//! Snapshot tests for reviva-export.
//!
//! The Markdown and JSON export formats are product surfaces.
//! Any schema drift in findings or sessions must be caught immediately.
//!
//! Coverage targets (populated as reviva-export is implemented):
//!   - Markdown export for a single finding matches snapshot
//!   - Markdown export for a full session matches snapshot
//!   - JSON export for a single finding is valid JSON and matches snapshot
//!   - JSON export for a full session is valid JSON and matches snapshot
//!   - findings with missing optional fields export without panic
//!   - empty findings list exports gracefully

// Placeholder — will be filled as reviva-export is implemented.
#[test]
fn placeholder_export_snapshot() {
    // TODO: construct a fixture Finding and Session, call the exporter,
    //       and lock output with insta::assert_snapshot!.
    insta::assert_snapshot!("placeholder", "placeholder export output");
}

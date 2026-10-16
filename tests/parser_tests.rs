//! Tests for the `cargo --message-format=json` parser.
//!
//! Test per il parser di `cargo --message-format=json`.
//!
//! Fixtures are minimal but faithful slices of real cargo JSON lines. We assert
//! on the typed [`CheckOutcome`]: the `ok` flag, the summary counts and the
//! flattened diagnostic fields.

use nxm_mcp_exec::exec::parser::{parse_cargo_stream, Level};
use serde_json::Value;

/// A `compiler-message` line carrying an error with a primary span.
const ERROR_LINE: &str = r#"{"reason":"compiler-message","message":{"message":"unresolved import `foo`","level":"error","code":{"code":"E0432"},"spans":[{"file_name":"src/main.rs","line_start":12,"is_primary":true}]}}"#;

/// A `compiler-message` line carrying a warning with a primary span.
const WARNING_LINE: &str = r#"{"reason":"compiler-message","message":{"message":"unused variable: `x`","level":"warning","code":{"code":"unused_variables"},"spans":[{"file_name":"src/lib.rs","line_start":3,"is_primary":true}]}}"#;

/// Non-diagnostic cargo lines that must be ignored by the parser.
const NOISE_LINES: &str = r#"{"reason":"compiler-artifact","package_id":"foo 0.1.0","target":{"name":"foo"}}
{"reason":"build-finished","success":false}"#;

#[test]
fn error_makes_ok_false() {
    let out = parse_cargo_stream(ERROR_LINE);
    assert!(!out.ok, "an error-level diagnostic must set ok=false");
    assert_eq!(out.summary.errors, 1);
    assert_eq!(out.summary.warnings, 0);
    assert_eq!(out.diagnostics.len(), 1);

    let d = &out.diagnostics[0];
    assert_eq!(d.level, Level::Error);
    assert_eq!(d.code.as_deref(), Some("E0432"));
    assert_eq!(d.file.as_deref(), Some("src/main.rs"));
    assert_eq!(d.line, Some(12));
    assert_eq!(d.message, "unresolved import `foo`");
}

#[test]
fn warning_only_keeps_ok_true() {
    let out = parse_cargo_stream(WARNING_LINE);
    assert!(out.ok, "warnings alone must keep ok=true");
    assert_eq!(out.summary.errors, 0);
    assert_eq!(out.summary.warnings, 1);
    assert_eq!(out.diagnostics.len(), 1);
    assert_eq!(out.diagnostics[0].level, Level::Warning);
    assert_eq!(out.diagnostics[0].code.as_deref(), Some("unused_variables"));
}

#[test]
fn mixed_stream_counts_both_and_is_not_ok() {
    let stream = format!("{WARNING_LINE}\n{ERROR_LINE}\n{WARNING_LINE}");
    let out = parse_cargo_stream(&stream);
    assert!(!out.ok);
    assert_eq!(out.summary.errors, 1);
    assert_eq!(out.summary.warnings, 2);
    assert_eq!(out.diagnostics.len(), 3);
}

#[test]
fn non_diagnostic_lines_are_ignored() {
    let out = parse_cargo_stream(NOISE_LINES);
    assert!(out.ok, "no compiler-message => no errors => ok=true");
    assert_eq!(out.summary.errors, 0);
    assert_eq!(out.summary.warnings, 0);
    assert!(out.diagnostics.is_empty());
}

#[test]
fn blank_and_garbage_lines_do_not_abort_parsing() {
    // A blank line, a non-JSON status line, then a real error line.
    let stream = format!("\n   Compiling foo v0.1.0\n{ERROR_LINE}\n");
    let out = parse_cargo_stream(&stream);
    assert!(!out.ok);
    assert_eq!(out.summary.errors, 1);
    assert_eq!(out.diagnostics.len(), 1);
}

#[test]
fn empty_stream_is_ok_with_no_diagnostics() {
    let out = parse_cargo_stream("");
    assert!(out.ok);
    assert_eq!(out.summary.errors, 0);
    assert_eq!(out.summary.warnings, 0);
    assert!(out.diagnostics.is_empty());
}

#[test]
fn note_and_help_levels_are_skipped() {
    let note = r#"{"reason":"compiler-message","message":{"message":"note here","level":"note","spans":[]}}"#;
    let help = r#"{"reason":"compiler-message","message":{"message":"try this","level":"help","spans":[]}}"#;
    let out = parse_cargo_stream(&format!("{note}\n{help}"));
    assert!(out.ok);
    assert!(out.diagnostics.is_empty());
}

#[test]
fn diagnostic_without_code_or_span_emits_null_fields() {
    // No `code` and no primary span: all three optional fields are None, and
    // the policy requires them to be emitted as `null` (keys always present).
    let line = r#"{"reason":"compiler-message","message":{"message":"bare warning","level":"warning","spans":[]}}"#;
    let out = parse_cargo_stream(line);
    assert_eq!(out.diagnostics.len(), 1);

    let value = serde_json::to_value(&out.diagnostics[0]).expect("diagnostic serializes");
    assert_eq!(value["code"], Value::Null, "code must be emitted as null");
    assert_eq!(value["file"], Value::Null, "file must be emitted as null");
    assert_eq!(value["line"], Value::Null, "line must be emitted as null");
    assert_eq!(value["level"], "warning");
    assert_eq!(value["message"], "bare warning");
}

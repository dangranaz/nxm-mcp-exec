//! Tests for `exec::test_parser` — pure libtest JSON output parser.
//!
//! Test per `exec::test_parser` — parser puro dell'output JSON di libtest.
//!
//! TDD: questi test sono scritti prima dell'implementazione (red → green).
//! Le fixture sono stringhe JSON libtest fedeli al formato:
//! `{ "type":"test", "event":"ok"|"failed"|"ignored", "name":..., "stdout":... }`
//! più l'evento `{ "type":"suite", "event":"ok"|"failed", "passed":N, ... }`.

use nxm_mcp_exec::exec::test_parser::{parse_libtest_stream, TestFailure, TestOutcome};

/// Una riga libtest per un test passato.
fn ok_line(name: &str) -> String {
    format!(
        r#"{{"type":"test","event":"ok","name":"{}","stdout":"","stderr":""}}"#,
        name
    )
}

/// Una riga libtest per un test fallito, con un messaggio utile nello stdout.
fn failed_line(name: &str, stdout: &str) -> String {
    format!(
        r#"{{"type":"test","event":"failed","name":"{}","stdout":"{}","stderr":""}}"#,
        name, stdout
    )
}

/// Una riga libtest per un test ignorato.
fn ignored_line(name: &str) -> String {
    format!(
        r#"{{"type":"test","event":"ignored","name":"{}","stdout":"","stderr":""}}"#,
        name
    )
}

/// Un evento di suite libtest.
fn suite_line(passed: u32, failed: u32, ignored: u32) -> String {
    format!(
        r#"{{"type":"suite","event":"ok","passed":{},"failed":{},"ignored":{}}}"#,
        passed, failed, ignored
    )
}

#[test]
fn test_parser_serialized_shape_matches_expected() {
    let outcome = TestOutcome {
        passed: 2,
        failed: 1,
        ignored: 1,
        failures: vec![TestFailure {
            name: "test_bar".to_string(),
            message: "panicked at 'assertion failed'".to_string(),
        }],
    };

    let value = serde_json::to_value(&outcome).expect("serializes");
    assert_eq!(value["passed"], 2);
    assert_eq!(value["failed"], 1);
    assert_eq!(value["ignored"], 1);
    assert_eq!(
        value["failures"]
            .as_array()
            .expect("failures is array")
            .len(),
        1
    );
    assert_eq!(value["failures"][0]["name"], "test_bar");
    assert_eq!(
        value["failures"][0]["message"],
        "panicked at 'assertion failed'"
    );
}

#[test]
fn test_parser_all_green_suite() {
    let stream = format!(
        "{}\n{}\n{}\n{}",
        ok_line("test_foo"),
        ok_line("test_bar"),
        ok_line("test_baz"),
        suite_line(3, 0, 0)
    );

    let out = parse_libtest_stream(&stream);
    assert_eq!(out.passed, 3);
    assert_eq!(out.failed, 0);
    assert_eq!(out.ignored, 0);
    assert!(out.failures.is_empty());
}

#[test]
fn test_parser_failure_extracts_name_and_message() {
    let stream = format!(
        "{}\n{}\n{}",
        ok_line("test_foo"),
        failed_line("test_bar", "panicked at 'assertion failed', src/lib.rs:42"),
        suite_line(1, 1, 0)
    );

    let out = parse_libtest_stream(&stream);
    assert_eq!(out.passed, 1);
    assert_eq!(out.failed, 1);
    assert_eq!(out.ignored, 0);
    assert_eq!(out.failures.len(), 1);
    assert_eq!(out.failures[0].name, "test_bar");
    assert!(
        !out.failures[0].message.is_empty(),
        "message must be non-empty"
    );
    assert!(
        out.failures[0].message.contains("panicked"),
        "message must come from stdout excerpt: {}",
        out.failures[0].message
    );
}

#[test]
fn test_parser_ignored_is_counted() {
    let stream = format!(
        "{}\n{}\n{}",
        ok_line("test_foo"),
        ignored_line("test_bar"),
        suite_line(1, 0, 1)
    );

    let out = parse_libtest_stream(&stream);
    assert_eq!(out.passed, 1);
    assert_eq!(out.failed, 0);
    assert_eq!(out.ignored, 1);
    assert!(out.failures.is_empty());
}

#[test]
fn test_parser_non_json_and_blank_lines_are_skipped() {
    let stream = format!(
        "\n   Compiling foo v0.1.0\nnot json at all\n{}\n{}\n{}",
        ok_line("test_foo"),
        failed_line("test_bar", "panicked"),
        suite_line(1, 1, 0)
    );

    let out = parse_libtest_stream(&stream);
    assert_eq!(out.passed, 1);
    assert_eq!(out.failed, 1);
    assert_eq!(out.ignored, 0);
    assert_eq!(out.failures.len(), 1);
}

#[test]
fn test_parser_unknown_event_types_do_not_alter_counts() {
    let stream = format!(
        "{}\n{}\n{}",
        ok_line("test_foo"),
        r#"{"type":"benchmark","event":"ok","name":"bench_foo","ns_iter":100}"#,
        suite_line(1, 0, 0)
    );

    let out = parse_libtest_stream(&stream);
    assert_eq!(out.passed, 1);
    assert_eq!(out.failed, 0);
    assert_eq!(out.ignored, 0);
    assert!(out.failures.is_empty());
}

#[test]
fn test_parser_counts_derive_from_individual_test_events() {
    // La fonte autoritativa dei conteggi sono gli eventi `test` singoli;
    // l'evento `suite` è letto ma non sovrascrive i conteggi derivati.
    let stream = format!(
        "{}\n{}\n{}\n{}",
        ok_line("test_foo"),
        ok_line("test_bar"),
        failed_line("test_baz", "panicked"),
        suite_line(99, 99, 99) // suite dichiara conteggi diversi: ignorati
    );

    let out = parse_libtest_stream(&stream);
    assert_eq!(out.passed, 2);
    assert_eq!(out.failed, 1);
    assert_eq!(out.ignored, 0);
    assert_eq!(out.failures.len(), 1);
    assert_eq!(out.failures[0].name, "test_baz");
}

#[test]
fn test_parser_failure_message_falls_back_to_message_field() {
    // Quando stdout è vuoto, il message della failure deve cadere sul campo
    // `message` dell'evento (mai l'intero dump).
    let stream = format!(
        "{}\n{}",
        ok_line("test_foo"),
        r#"{"type":"test","event":"failed","name":"test_bar","stdout":"","message":"expected true, got false","stderr":""}"#,
    );

    let out = parse_libtest_stream(&stream);
    assert_eq!(out.passed, 1);
    assert_eq!(out.failed, 1);
    assert_eq!(out.failures.len(), 1);
    assert_eq!(out.failures[0].name, "test_bar");
    assert_eq!(
        out.failures[0].message, "expected true, got false",
        "message must fall back to the message field when stdout is empty"
    );
}

#[test]
fn test_parser_empty_stream_is_all_zero() {
    let out = parse_libtest_stream("");
    assert_eq!(out.passed, 0);
    assert_eq!(out.failed, 0);
    assert_eq!(out.ignored, 0);
    assert!(out.failures.is_empty());
}

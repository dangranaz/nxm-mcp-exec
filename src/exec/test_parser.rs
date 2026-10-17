//! Parser for libtest JSON test output.
//!
//! Parser per l'output JSON dei test libtest.
//!
//! `cargo test -- -Z unstable-options --format=json` emette un oggetto JSON
//! per riga. Gli eventi che ci interessano hanno `"type": "test"` con
//! `"event": "ok"|"failed"|"ignored"` e un campo `name`. C'è anche un evento
//! di suite `"type": "suite"` con conteggi aggregati.
//!
//! Trasformiamo lo stream in `TestOutcome { passed, failed, ignored, failures }`.
//! Parser difensivo: righe non-JSON/vuote saltate, campi sconosciuti ignorati,
//! nessun panic sul drift di formato.
//!
//! Il formato è nightly/unstable: questo task si occupa solo del parsing di
//! uno stream dato come input (fixture). La gestione nightly/stabilità è
//! delegata a `exec-03b`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tracing;

/// L'esito completo di un'esecuzione di test.
///
/// The full outcome of a test run.
///
/// # Examples
/// ```
/// use nxm_mcp_exec::exec::test_parser::{TestFailure, TestOutcome};
/// let outcome = TestOutcome {
///     passed: 2,
///     failed: 1,
///     ignored: 0,
///     failures: vec![TestFailure {
///         name: "test_foo".to_string(),
///         message: "panicked at 'assertion failed'".to_string(),
///     }],
/// };
/// assert_eq!(outcome.passed, 2);
/// assert_eq!(outcome.failures.len(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestOutcome {
    /// Numero di test passati.
    pub passed: u32,
    /// Numero di test falliti.
    pub failed: u32,
    /// Numero di test ignorati.
    pub ignored: u32,
    /// Elenco dei fallimenti con nome e messaggio.
    pub failures: Vec<TestFailure>,
}

/// Un singolo fallimento di test.
///
/// A single test failure.
///
/// # Examples
/// ```
/// use nxm_mcp_exec::exec::test_parser::TestFailure;
/// let failure = TestFailure {
///     name: "test_foo".to_string(),
///     message: "panicked at 'assertion failed'".to_string(),
/// };
/// assert_eq!(failure.name, "test_foo");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestFailure {
    /// Nome del test (es. `test_foo`).
    pub name: String,
    /// Messaggio utile estratto dallo stdout dell'evento `failed` (non l'intero dump).
    pub message: String,
}

/// Analizza uno stream JSON libtest in un [`TestOutcome`].
///
/// Parses a libtest JSON stream into a [`TestOutcome`].
///
/// # Examples
/// ```
/// use nxm_mcp_exec::exec::test_parser::parse_libtest_stream;
/// let stream = r#"{"type":"test","event":"ok","name":"test_foo","stdout":"","stderr":""}
/// {"type":"test","event":"failed","name":"test_bar","stdout":"panicked","stderr":""}
/// {"type":"suite","event":"ok","passed":1,"failed":1,"ignored":0}"#;
/// let outcome = parse_libtest_stream(stream);
/// assert_eq!(outcome.passed, 1);
/// assert_eq!(outcome.failed, 1);
/// assert_eq!(outcome.failures[0].name, "test_bar");
/// ```
///
/// Comportamento:
/// - Un oggetto JSON per riga; righe vuote e non-JSON sono saltate.
/// - Solo gli eventi `"type": "test"` con `"event"` in `{ok, failed, ignored}`
///   producono conteggi/failure.
/// - L'evento `"type": "suite"` viene letto ma **non sovrascrive** i conteggi
///   derivati dagli eventi `test` singoli (fonte autoritativa: gli eventi
///   individuali).
/// - Il `message` di una failure è la prima riga non vuota dello `stdout`
///   dell'evento `failed`; se assente, si usa il campo `message`.
pub fn parse_libtest_stream(stream: &str) -> TestOutcome {
    tracing::debug!(
        target: "nxm::exec::test_parser",
        input_len = stream.len(),
        "parsing libtest JSON stream"
    );

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut ignored = 0u32;
    let mut failures = Vec::new();

    for raw in stream.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let event: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let rtype = event.get("type").and_then(Value::as_str).unwrap_or("");
        if rtype != "test" {
            // suite, benchmark, ecc.: ignorati per i conteggi.
            continue;
        }

        let event_kind = event.get("event").and_then(Value::as_str).unwrap_or("");
        let name = event
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        match event_kind {
            "ok" => passed += 1,
            "failed" => {
                failed += 1;
                let message = extract_failure_message(&event);
                failures.push(TestFailure { name, message });
            }
            "ignored" => ignored += 1,
            _ => continue,
        }
    }

    let outcome = TestOutcome {
        passed,
        failed,
        ignored,
        failures,
    };

    tracing::debug!(
        target: "nxm::exec::test_parser",
        passed = outcome.passed,
        failed = outcome.failed,
        ignored = outcome.ignored,
        failures = outcome.failures.len(),
        "libtest JSON stream parsed"
    );

    outcome
}

/// Estrae un messaggio utile da un evento `failed` libtest.
///
/// Regola minima: prima riga non vuota dello `stdout`; se assente, il campo
/// `message`; altrimenti stringa vuota. Non è mai l'intero dump.
fn extract_failure_message(event: &Value) -> String {
    let stdout = event.get("stdout").and_then(Value::as_str).unwrap_or("");
    for line in stdout.lines() {
        let line = line.trim();
        if !line.is_empty() {
            return line.to_string();
        }
    }
    event
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

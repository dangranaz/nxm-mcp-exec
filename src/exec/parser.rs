//! Parser for the `cargo --message-format=json` stream.
//!
//! Parser per lo stream `cargo --message-format=json`.
//!
//! `cargo check --message-format=json` emits one JSON object per line. The
//! objects we care about have `"reason": "compiler-message"` and carry a
//! `message` field shaped like a rustc diagnostic. We extract only the minimal,
//! stable fields an agent needs — never the human-formatted `rendered` blob.
//!
//! `cargo check --message-format=json` emette un oggetto JSON per riga. Gli
//! oggetti che ci interessano hanno `"reason": "compiler-message"` e portano un
//! campo `message` con la forma di una diagnostica rustc. Estraiamo solo i
//! campi minimi e stabili che servono a un agent — mai il testo `rendered`.

use serde::{Deserialize, Serialize};

/// Severity level of a diagnostic, normalised to the two we act on.
///
/// Livello di severità di una diagnostica, normalizzato ai due su cui agiamo.
///
/// rustc emits other levels (`note`, `help`, `failure-note`); those are not
/// standalone problems, so we skip them when building the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
}

/// A single, flattened diagnostic: the minimal shape an agent needs.
///
/// Una singola diagnostica appiattita: la forma minima che serve a un agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// `"error"` or `"warning"`.
    pub level: Level,
    /// Lint/error code such as `E0432` or `unused_variables`, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Primary span file path, when the diagnostic has a primary span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 1-based line of the primary span, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Human-readable message (the short `message`, not the rendered blob).
    pub message: String,
}

/// Counts of errors and warnings across all diagnostics.
///
/// Conteggio di errori e warning su tutte le diagnostiche.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub errors: u32,
    pub warnings: u32,
}

/// The full outcome of a check: the ok flag, diagnostics and summary.
///
/// L'esito completo di un check: il flag ok, le diagnostiche e il riepilogo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOutcome {
    /// `true` when there are no error-level diagnostics (warnings are allowed).
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub summary: Summary,
}

// --- Internal, lenient mirror of the cargo/rustc JSON shape ---------------
//
// We deserialize defensively: unknown fields are ignored and missing optional
// fields default, so a drift in cargo's output never panics the parser.

#[derive(Debug, Deserialize)]
struct CargoLine {
    #[serde(default)]
    reason: String,
    #[serde(default)]
    message: Option<RustcMessage>,
}

#[derive(Debug, Deserialize)]
struct RustcMessage {
    #[serde(default)]
    message: String,
    #[serde(default)]
    level: String,
    #[serde(default)]
    code: Option<RustcCode>,
    #[serde(default)]
    spans: Vec<RustcSpan>,
}

#[derive(Debug, Deserialize)]
struct RustcCode {
    #[serde(default)]
    code: String,
}

#[derive(Debug, Deserialize)]
struct RustcSpan {
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    line_start: Option<u32>,
    #[serde(default)]
    is_primary: bool,
}

/// Parse a full `cargo --message-format=json` stream into a [`CheckOutcome`].
///
/// Analizza l'intero stream `cargo --message-format=json` in un [`CheckOutcome`].
///
/// Behaviour / Comportamento:
/// - One JSON object per line; blank lines and non-JSON lines are skipped so a
///   stray cargo status line never aborts parsing.
/// - Only `reason == "compiler-message"` lines produce diagnostics.
/// - Only `error` and `warning` levels are kept (`note`/`help` are ignored).
/// - `ok == true` iff there are zero error-level diagnostics.
///
/// # Examples
/// ```
/// use nxm_mcp_exec::exec::parser::parse_cargo_stream;
/// let line = r#"{"reason":"compiler-message","message":{"message":"unused","level":"warning","code":{"code":"unused_variables"},"spans":[{"file_name":"src/x.rs","line_start":3,"is_primary":true}]}}"#;
/// let out = parse_cargo_stream(line);
/// assert!(out.ok); // warnings alone keep ok=true
/// assert_eq!(out.summary.warnings, 1);
/// ```
pub fn parse_cargo_stream(stream: &str) -> CheckOutcome {
    let mut diagnostics = Vec::new();
    let mut summary = Summary::default();

    for raw in stream.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        // Skip anything that is not a JSON object (e.g. plain status text).
        let parsed: CargoLine = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if parsed.reason != "compiler-message" {
            continue;
        }
        let Some(msg) = parsed.message else {
            continue;
        };
        let level = match msg.level.as_str() {
            "error" => Level::Error,
            "warning" => Level::Warning,
            // note / help / failure-note / unknown: not a standalone problem.
            _ => continue,
        };

        match level {
            Level::Error => summary.errors += 1,
            Level::Warning => summary.warnings += 1,
        }

        let primary = msg
            .spans
            .iter()
            .find(|s| s.is_primary)
            .or_else(|| msg.spans.first());

        diagnostics.push(Diagnostic {
            level,
            code: msg.code.map(|c| c.code).filter(|c| !c.is_empty()),
            file: primary.and_then(|s| s.file_name.clone()),
            line: primary.and_then(|s| s.line_start),
            message: msg.message,
        });
    }

    CheckOutcome {
        ok: summary.errors == 0,
        diagnostics,
        summary,
    }
}

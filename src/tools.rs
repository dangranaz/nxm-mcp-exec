//! MCP tool implementations: translate MCP arguments into typed runner calls.
//!
//! Implementazioni dei tool MCP: traducono gli argomenti MCP in chiamate
//! tipate verso i runner.
//!
//! Each `tool_*` function parses the free-form `serde_json::Value` arguments,
//! builds a typed args struct and invokes the corresponding runner in
//! [`crate::exec`]. Validation errors surface as readable strings (MCP
//! `isError` results), never as panics.
//!
//! Ogni funzione `tool_*` parsa gli argomenti `serde_json::Value`, costruisce
//! una struct di argomenti tipata e invoca il runner corrispondente in
//! [`crate::exec`]. Gli errori di validazione emergono come stringhe leggibili
//! (risultati MCP `isError`), mai come panic.

use serde_json::Value;

use crate::exec::clippy_runner::{cargo_clippy, ClippyArgs};
use crate::exec::fmt_runner::{cargo_fmt_check, FmtArgs};
use crate::exec::gate_runner::run_gate;
use crate::exec::runner::{cargo_build, cargo_check, CargoBuildArgs, CargoCheckArgs};
use crate::exec::test_runner::{cargo_test, CargoTestArgs};

/// Run the tool `name` with `args`, returning its typed outcome as JSON.
///
/// Esegue il tool `name` con `args` e ritorna l'esito tipato come JSON.
///
/// # Errors
/// Returns a readable error string for an unknown tool name or for invalid
/// arguments; runner failures (spawn, timeout, invalid repo) surface the same
/// way. A compile/lint failure in the target repo is NOT an error here: it is
/// a typed outcome with `ok == false`.
///
/// Ritorna una stringa d'errore leggibile per un nome tool sconosciuto o per
/// argomenti non validi; i fallimenti del runner (spawn, timeout, repo non
/// valida) emergono allo stesso modo. Un fallimento di compilazione/lint nel
/// repo target NON è un errore: è un esito tipato con `ok == false`.
pub fn run_tool(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "cargo_check" => tool_cargo_check(args),
        "cargo_build" => tool_cargo_build(args),
        "cargo_test" => tool_cargo_test(args),
        "cargo_clippy" => tool_cargo_clippy(args),
        "cargo_fmt_check" => tool_cargo_fmt_check(args),
        "run_gate" => tool_run_gate(args),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Run the `cargo_fmt_check` tool and return its typed outcome as JSON.
///
/// Esegue il tool `cargo_fmt_check` e ritorna l'esito tipato come JSON.
///
/// The outcome is `{ok, offending_files}`: `ok == true` when the target repo
/// is already formatted, otherwise `ok == false` with the list of files that
/// `cargo fmt --check` would rewrite. The command is read-only: it never
/// modifies the target files.
///
/// L'esito è `{ok, offending_files}`: `ok == true` quando il repo target è
/// già formattato, altrimenti `ok == false` con l'elenco dei file che
/// `cargo fmt --check` riscriverebbe. Il comando è read-only: non modifica
/// mai i file target.
fn tool_cargo_fmt_check(args: &Value) -> Result<Value, String> {
    let repo_path = require_str(args, "repo_path")?;

    let fmt_args = FmtArgs {
        repo_path: repo_path.into(),
        timeout: None, // default 120s; per-call override is a later micro-task
    };

    let outcome = cargo_fmt_check(&fmt_args).map_err(|e| format!("{e:#}"))?;
    serde_json::to_value(&outcome).map_err(|e| format!("failed to serialize outcome: {e}"))
}

/// Run the `cargo_check` tool and return its typed outcome as JSON.
///
/// Esegue il tool `cargo_check` e ritorna l'esito tipato come JSON.
fn tool_cargo_check(args: &Value) -> Result<Value, String> {
    let repo_path = require_str(args, "repo_path")?;
    let crate_name = args.get("crate").and_then(Value::as_str).map(String::from);
    let workspace = args
        .get("workspace")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let check_args = CargoCheckArgs {
        repo_path: repo_path.into(),
        crate_name,
        workspace,
        timeout: None, // default 120s; per-call override is a later micro-task
    };

    let outcome = cargo_check(&check_args).map_err(|e| format!("{e:#}"))?;
    serde_json::to_value(&outcome).map_err(|e| format!("failed to serialize outcome: {e}"))
}

/// Run the `cargo_build` tool and return its typed outcome as JSON.
///
/// Esegue il tool `cargo_build` e ritorna l'esito tipato come JSON.
fn tool_cargo_build(args: &Value) -> Result<Value, String> {
    let repo_path = require_str(args, "repo_path")?;
    let release = args
        .get("release")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let features = args
        .get("features")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let build_args = CargoBuildArgs {
        repo_path: repo_path.into(),
        release,
        features,
        timeout: None, // default 120s; per-call override is a later micro-task
    };

    let outcome = cargo_build(&build_args).map_err(|e| format!("{e:#}"))?;
    serde_json::to_value(&outcome).map_err(|e| format!("failed to serialize outcome: {e}"))
}

/// Run the `cargo_test` tool and return its typed outcome as JSON.
///
/// Esegue il tool `cargo_test` e ritorna l'esito tipato come JSON.
fn tool_cargo_test(args: &Value) -> Result<Value, String> {
    let repo_path = require_str(args, "repo_path")?;
    let crate_name = args.get("crate").and_then(Value::as_str).map(String::from);
    let filter = args.get("filter").and_then(Value::as_str).map(String::from);

    let test_args = CargoTestArgs {
        repo_path: repo_path.into(),
        crate_name,
        filter,
        timeout: None, // default 120s; per-call override is a later micro-task
    };

    let outcome = cargo_test(&test_args).map_err(|e| format!("{e:#}"))?;
    serde_json::to_value(&outcome).map_err(|e| format!("failed to serialize outcome: {e}"))
}

/// Run the `cargo_clippy` tool and return its typed outcome as JSON.
///
/// Esegue il tool `cargo_clippy` e ritorna l'esito tipato come JSON.
fn tool_cargo_clippy(args: &Value) -> Result<Value, String> {
    let repo_path = require_str(args, "repo_path")?;
    let deny_warnings = args
        .get("deny_warnings")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let clippy_args = ClippyArgs {
        repo_path: repo_path.into(),
        deny_warnings,
        timeout: None, // default 120s; per-call override is a later micro-task
    };

    let outcome = cargo_clippy(&clippy_args).map_err(|e| format!("{e:#}"))?;
    serde_json::to_value(&outcome).map_err(|e| format!("failed to serialize outcome: {e}"))
}

/// Run the `run_gate` tool and return its typed verdict as JSON.
///
/// Esegue il tool `run_gate` e ritorna il verdetto tipato come JSON.
///
/// `gate_name` is optional and defaults to `"default"` (documented order:
/// check → clippy → fmt_check → test). The gate short-circuits at the first
/// failed step: `steps` contains only the executed steps and `failed_step`
/// names the first failure (`null` on GO).
///
/// `gate_name` è opzionale e di default è `"default"` (ordine documentato:
/// check → clippy → fmt_check → test). Il gate fa short-circuit al primo
/// step fallito: `steps` contiene solo gli step eseguiti e `failed_step`
/// nomina il primo fallimento (`null` su GO).
fn tool_run_gate(args: &Value) -> Result<Value, String> {
    let repo_path = require_str(args, "repo_path")?;
    let gate_name = args
        .get("gate_name")
        .and_then(Value::as_str)
        .unwrap_or("default");

    let outcome =
        run_gate(std::path::Path::new(&repo_path), gate_name).map_err(|e| format!("{e:#}"))?;
    serde_json::to_value(&outcome).map_err(|e| format!("failed to serialize outcome: {e}"))
}

/// Require a string argument, returning a readable error when absent.
///
/// Richiede un argomento stringa, ritornando un errore leggibile se assente.
fn require_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing required argument '{key}'"))
}

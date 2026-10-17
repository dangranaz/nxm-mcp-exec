//! `run_gate` orchestration: runs an ordered sequence of EXISTING tools
//! (`cargo_check` → `cargo_clippy` → `cargo_fmt_check` → `cargo_test`) with
//! short-circuit at the first failure and a GO/NO-GO verdict.
//!
//! Orchestrazione di `run_gate`: esegue una sequenza ordinata di tool GIÀ
//! esistenti (`cargo_check` → `cargo_clippy` → `cargo_fmt_check` →
//! `cargo_test`) con short-circuit al primo fallimento e verdetto GO/NO-GO.
//!
//! This module composes the correctness verdicts of the existing runners; it
//! is NOT a CI and does NOT reimplement any check: every step calls the
//! already-tested runner functions. A runner typed error (spawn failure,
//! invalid repo, missing test backend) counts as a step failure and
//! short-circuits, exactly like an `ok == false` outcome.
//!
//! Questo modulo compone i verdetti di correttezza dei runner esistenti; NON
//! è una CI e NON reimplementa alcun check: ogni step richiama le funzioni
//! runner già testate. Un errore tipato del runner (spawn, repo non valida,
//! backend di test assente) conta come fallimento dello step e fa
//! short-circuit, esattamente come un esito `ok == false`.

use std::path::Path;

use anyhow::{bail, Result};
use serde::Serialize;

use super::clippy_runner::{cargo_clippy, ClippyArgs};
use super::fmt_runner::{cargo_fmt_check, FmtArgs};
use super::runner::{cargo_check, CargoCheckArgs};
use super::test_runner::{cargo_test, CargoTestArgs};

use tracing;

/// The default gate: documented, fixed execution order.
///
/// Il gate di default: ordine di esecuzione documentato e fisso.
///
/// Order rationale: cheap correctness first (check), then lints (clippy),
/// then formatting (fmt_check), then the most expensive step (test).
///
/// Ragione dell'ordine: prima la correttezza economica (check), poi le lint
/// (clippy), poi la formattazione (fmt_check), infine lo step più costoso
/// (test).
pub const DEFAULT_GATE: &[&str] = &[
    "cargo_check",
    "cargo_clippy",
    "cargo_fmt_check",
    "cargo_test",
];

/// Resolve a named gate to its ordered step list (pure).
///
/// Risolve un gate nominato nella sua lista ordinata di step (pura).
///
/// Only the documented `default` gate exists today; rich gate configurability
/// is out of scope for this micro-task.
///
/// Oggi esiste solo il gate `default` documentato; la configurabilità ricca
/// dei gate è fuori scope per questo micro-task.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::gate_runner::{gate_steps, DEFAULT_GATE};
/// assert_eq!(gate_steps("default"), Some(DEFAULT_GATE));
/// assert_eq!(gate_steps("bogus"), None);
/// ```
pub fn gate_steps(gate_name: &str) -> Option<&'static [&'static str]> {
    match gate_name {
        "default" => Some(DEFAULT_GATE),
        _ => None,
    }
}

/// Verdict of a gate run: `GO` only when every executed step passed.
///
/// Verdetto di un run del gate: `GO` solo quando ogni step eseguito è passato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum GateVerdict {
    #[serde(rename = "GO")]
    Go,
    #[serde(rename = "NO-GO")]
    NoGo,
}

/// Outcome of a single executed step.
///
/// Esito di un singolo step eseguito.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateStepOutcome {
    /// Step name (the existing tool name, e.g. `cargo_check`).
    ///
    /// Nome dello step (il nome del tool esistente, es. `cargo_check`).
    pub name: String,
    /// `true` when the step passed (`ok == true`, zero test failures).
    ///
    /// `true` quando lo step è passato (`ok == true`, zero test falliti).
    pub ok: bool,
}

/// The full gate outcome: verdict, failed step and the EXECUTED steps.
///
/// L'esito completo del gate: verdetto, step fallito e gli step ESEGUITI.
///
/// Short-circuit policy: `steps` contains ONLY the steps actually executed —
/// the steps after the first failure are absent (they did not run).
///
/// Policy di short-circuit: `steps` contiene SOLO gli step effettivamente
/// eseguiti — gli step successivi al primo fallimento sono assenti (non sono
/// stati eseguiti).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateOutcome {
    /// `"GO"` when every executed step passed, `"NO-GO"` otherwise.
    ///
    /// `"GO"` quando ogni step eseguito è passato, `"NO-GO"` altrimenti.
    pub verdict: GateVerdict,
    /// Name of the first failed step; `null` on `GO`.
    ///
    /// Nome del primo step fallito; `null` su `GO`.
    pub failed_step: Option<String>,
    /// Executed steps, in execution order (short-circuit: later steps absent).
    ///
    /// Step eseguiti, in ordine di esecuzione (short-circuit: i successivi
    /// sono assenti).
    pub steps: Vec<GateStepOutcome>,
}

/// Execute a single gate step by calling the EXISTING runner (no
/// reimplementation), returning its correctness verdict.
///
/// Esegue un singolo step del gate richiamando il runner ESISTENTE (nessuna
/// reimplementazione), ritornando il suo verdetto di correttezza.
///
/// # Errors
/// Propagates the runner's typed errors (invalid repo, spawn failure, missing
/// test backend); [`run_gate`] maps them to a failed step.
///
/// Propaga gli errori tipati del runner (repo non valida, spawn fallito,
/// backend di test assente); [`run_gate`] li mappa a uno step fallito.
fn run_step(name: &str, repo_path: &Path) -> Result<bool> {
    match name {
        "cargo_check" => {
            let args = CargoCheckArgs {
                repo_path: repo_path.to_path_buf(),
                crate_name: None,
                workspace: false,
                timeout: None,
            };
            Ok(cargo_check(&args)?.ok)
        }
        "cargo_clippy" => {
            let args = ClippyArgs {
                repo_path: repo_path.to_path_buf(),
                deny_warnings: false,
                timeout: None,
            };
            Ok(cargo_clippy(&args)?.ok)
        }
        "cargo_fmt_check" => {
            let args = FmtArgs {
                repo_path: repo_path.to_path_buf(),
                timeout: None,
            };
            Ok(cargo_fmt_check(&args)?.ok)
        }
        "cargo_test" => {
            let args = CargoTestArgs {
                repo_path: repo_path.to_path_buf(),
                crate_name: None,
                filter: None,
                timeout: None,
            };
            let outcome = cargo_test(&args)?;
            Ok(outcome.failed == 0)
        }
        other => bail!("unknown gate step `{other}`"),
    }
}

/// Run the named gate in `repo_path`: ordered steps, short-circuit at the
/// first failure, GO/NO-GO verdict.
///
/// Esegue il gate nominato in `repo_path`: step ordinati, short-circuit al
/// primo fallimento, verdetto GO/NO-GO.
///
/// # Errors
/// Unknown `gate_name`, or an invalid `repo_path` (the first step's runner
/// validation fails) — surfaced as a typed error, never a panic.
///
/// `gate_name` sconosciuto, oppure `repo_path` non valida (la validazione del
/// runner del primo step fallisce) — emerge come errore tipato, mai panic.
///
/// # Example
/// ```no_run
/// use nxm_mcp_exec::exec::gate_runner::run_gate;
/// use std::path::Path;
/// let outcome = run_gate(Path::new("."), "default");
/// assert!(outcome.is_ok() || outcome.is_err());
/// ```
pub fn run_gate(repo_path: &Path, gate_name: &str) -> Result<GateOutcome> {
    tracing::debug!(
        target: "nxm::exec",
        repo = %repo_path.display(),
        gate = gate_name,
        "running gate"
    );
    let start = std::time::Instant::now();

    let steps = gate_steps(gate_name)
        .ok_or_else(|| anyhow::anyhow!("unknown gate `{gate_name}` (available: default)"))?;

    let mut executed: Vec<GateStepOutcome> = Vec::new();
    let mut failed_step: Option<String> = None;
    for step in steps {
        let ok = match run_step(step, repo_path) {
            Ok(ok) => ok,
            Err(e) => {
                tracing::warn!(
                    target: "nxm::exec",
                    repo = %repo_path.display(),
                    step,
                    error = %e,
                    "gate step failed with a typed runner error"
                );
                false
            }
        };
        tracing::debug!(
            target: "nxm::exec",
            repo = %repo_path.display(),
            step,
            ok,
            "gate step finished"
        );
        executed.push(GateStepOutcome {
            name: (*step).to_string(),
            ok,
        });
        if !ok {
            failed_step = Some((*step).to_string());
            break;
        }
    }

    let verdict = if failed_step.is_none() {
        GateVerdict::Go
    } else {
        GateVerdict::NoGo
    };

    let elapsed = start.elapsed();
    tracing::debug!(
        target: "nxm::exec",
        repo = %repo_path.display(),
        gate = gate_name,
        elapsed = ?elapsed,
        ?verdict,
        failed_step = ?failed_step,
        "gate finished"
    );

    Ok(GateOutcome {
        verdict,
        failed_step,
        steps: executed,
    })
}

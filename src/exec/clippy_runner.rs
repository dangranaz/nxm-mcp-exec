//! `cargo_clippy` execution: runs `cargo clippy --message-format=json` under
//! the allow-list and reuses [`crate::exec::parser::parse_cargo_stream`] — the
//! SAME structured diagnostics as `cargo_check` / `cargo_build`.
//!
//! Esecuzione di `cargo_clippy`: esegue `cargo clippy --message-format=json`
//! sotto allow-list e riusa [`crate::exec::parser::parse_cargo_stream`] — le
//! STESSE diagnostica strutturata di `cargo_check` / `cargo_build`.
//!
//! The only new logic is the `deny_warnings` policy: when active, warnings
//! count as errors for the `ok` flag. Enforcement is computed **on the parsed
//! summary**, NOT via the `-- -D warnings` clippy flag, so diagnostics keep
//! their true `warning` level (denying via argv would re-emit every lint as an
//! `error`, destroying the severity information the agent needs).
//!
//! L'unica logica nuova è la policy `deny_warnings`: quando attiva, i warning
//! contano come errori per il flag `ok`. L'enforcement è calcolato **sul
//! summary parsato**, NON tramite il flag clippy `-- -D warnings`, così le
//! diagnostiche mantengono il loro vero livello `warning` (negare via argv
//! riemetterebbe ogni lint come `error`, distruggendo l'informazione di
//! severità di cui l'agent ha bisogno).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use super::cargo_runner::{run_cargo, DEFAULT_TIMEOUT};
use super::parser::CheckOutcome;

use tracing;

/// Typed arguments for a `cargo_clippy` invocation.
///
/// Argomenti tipati per un'invocazione di `cargo_clippy`.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::clippy_runner::ClippyArgs;
/// use std::path::PathBuf;
/// let args = ClippyArgs {
///     repo_path: PathBuf::from("."),
///     deny_warnings: true,
///     timeout: None,
/// };
/// assert!(args.deny_warnings);
/// ```
#[derive(Debug, Clone)]
pub struct ClippyArgs {
    /// Target repository directory. Must exist and contain a `Cargo.toml`.
    pub repo_path: PathBuf,
    /// When `true`, warnings also count as errors for the `ok` flag
    /// (`ok == errors == 0 && (!deny_warnings || warnings == 0)`).
    ///
    /// Se `true`, anche i warning contano come errori per il flag `ok`
    /// (`ok == errors == 0 && (!deny_warnings || warnings == 0)`).
    pub deny_warnings: bool,
    /// Optional per-call timeout override; `None` uses [`DEFAULT_TIMEOUT`].
    ///
    /// Override facoltativo per singola chiamata; `None` usa [`DEFAULT_TIMEOUT`].
    pub timeout: Option<Duration>,
}

/// Run `cargo clippy --message-format=json` in `args.repo_path` and return a
/// typed [`CheckOutcome`], applying the `deny_warnings` policy to `ok`.
///
/// Esegue `cargo clippy --message-format=json` in `args.repo_path` e ritorna
/// un [`CheckOutcome`] tipato, applicando la policy `deny_warnings` a `ok`.
///
/// # Errors
/// Same as [`run_cargo`]: a lint or compile error in the target is NOT a
/// runner error; it surfaces as `ok == false` with diagnostics.
///
/// Uguale a [`run_cargo`]: una lint o un errore di compilazione del target NON
/// è un errore del runner; emerge come `ok == false` con diagnostica.
///
/// # Example
/// ```no_run
/// use nxm_mcp_exec::exec::clippy_runner::{cargo_clippy, ClippyArgs};
/// use std::path::PathBuf;
/// let args = ClippyArgs {
///     repo_path: PathBuf::from("."),
///     deny_warnings: false,
///     timeout: None,
/// };
/// // A clean repo yields `ok == true`; lint warnings keep `ok == true`
/// // unless `deny_warnings` is set.
/// let outcome = cargo_clippy(&args);
/// assert!(outcome.is_ok() || outcome.is_err());
/// ```
pub fn cargo_clippy(args: &ClippyArgs) -> Result<CheckOutcome> {
    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        deny_warnings = args.deny_warnings,
        timeout = ?args.timeout,
        "running cargo clippy --message-format=json"
    );
    let start = std::time::Instant::now();

    let timeout = args.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let mut outcome = run_cargo("clippy", &[], &args.repo_path, timeout)?;

    // deny_warnings policy: computed on the summary (see module docs).
    if args.deny_warnings && outcome.summary.warnings > 0 {
        outcome.ok = false;
    }

    let elapsed = start.elapsed();
    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        elapsed = ?elapsed,
        ok = outcome.ok,
        errors = outcome.summary.errors,
        warnings = outcome.summary.warnings,
        "cargo clippy finished"
    );

    Ok(outcome)
}

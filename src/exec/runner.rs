//! Runner: execute allow-listed cargo commands in a target repo directory.
//!
//! Runner: esegue comandi cargo in allow-list nella directory di un repo target.
//!
//! The actual process execution is delegated to [`crate::exec::cargo_runner`];
//! this module exposes the typed wrappers for `cargo_check` and `cargo_build`.
//!
//! L'esecuzione effettiva del processo è delegata a
//! [`crate::exec::cargo_runner`]; questo modulo espone i wrapper tipati per
//! `cargo_check` e `cargo_build`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use tracing;

pub use super::cargo_runner::{
    is_allowed_subcommand, run_cargo, wait_with_timeout, DEFAULT_TIMEOUT,
};
use super::parser::CheckOutcome;

/// Typed arguments for a `cargo_check` invocation.
///
/// Argomenti tipati per un'invocazione di `cargo_check`.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::runner::CargoCheckArgs;
/// use std::path::PathBuf;
/// let args = CargoCheckArgs {
///     repo_path: PathBuf::from("."),
///     crate_name: None,
///     workspace: false,
///     timeout: None,
/// };
/// assert_eq!(args.repo_path, PathBuf::from("."));
/// ```
#[derive(Debug, Clone)]
pub struct CargoCheckArgs {
    /// Target repository directory. Must exist and contain a `Cargo.toml`.
    pub repo_path: PathBuf,
    /// Optional `-p <crate>` package selector.
    pub crate_name: Option<String>,
    /// Optional `--workspace` flag.
    pub workspace: bool,
    /// Optional per-call timeout override; `None` uses [`DEFAULT_TIMEOUT`].
    ///
    /// Override facoltativo per singola chiamata; `None` usa [`DEFAULT_TIMEOUT`].
    pub timeout: Option<Duration>,
}

/// Run `cargo check --message-format=json` in `args.repo_path` and return a
/// typed [`CheckOutcome`].
///
/// Esegue `cargo check --message-format=json` in `args.repo_path` e ritorna un
/// [`CheckOutcome`] tipato.
///
/// # Errors
/// Returns an error if `repo_path` does not exist, is not a directory, has no
/// `Cargo.toml`, or if `cargo` cannot be spawned. A compile *error* in the
/// target is NOT a runner error: it surfaces as `ok == false` with diagnostics.
///
/// Ritorna errore se `repo_path` non esiste, non è una directory, non ha
/// `Cargo.toml`, o se `cargo` non può essere avviato. Un *errore di compilazione*
/// del target NON è un errore del runner: emerge come `ok == false` con diagnostica.
pub fn cargo_check(args: &CargoCheckArgs) -> Result<CheckOutcome> {
    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        workspace = args.workspace,
        crate_name = ?args.crate_name,
        timeout = ?args.timeout,
        "running cargo check --message-format=json"
    );
    let start = std::time::Instant::now();

    let mut extra = Vec::new();
    if args.workspace {
        extra.push("--workspace".to_string());
    }
    if let Some(name) = &args.crate_name {
        // `-p <crate>`: the crate name is passed as a single argv element, so it
        // cannot inject further flags or shell metacharacters.
        extra.push("-p".to_string());
        extra.push(name.clone());
    }
    let timeout = args.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let outcome = run_cargo("check", &extra, &args.repo_path, timeout)?;

    let elapsed = start.elapsed();
    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        elapsed = ?elapsed,
        ok = outcome.ok,
        errors = outcome.summary.errors,
        warnings = outcome.summary.warnings,
        "cargo check finished"
    );

    Ok(outcome)
}

/// Typed arguments for a `cargo_build` invocation.
///
/// Argomenti tipati per un'invocazione di `cargo_build`.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::runner::CargoBuildArgs;
/// use std::path::PathBuf;
/// let args = CargoBuildArgs {
///     repo_path: PathBuf::from("."),
///     release: false,
///     features: vec![],
///     timeout: None,
/// };
/// assert!(!args.release);
/// ```
#[derive(Debug, Clone)]
pub struct CargoBuildArgs {
    /// Target repository directory. Must exist and contain a `Cargo.toml`.
    pub repo_path: PathBuf,
    /// Optional `--release` flag.
    pub release: bool,
    /// Optional features list, passed as `--features <csv>`.
    pub features: Vec<String>,
    /// Optional per-call timeout override; `None` uses [`DEFAULT_TIMEOUT`].
    ///
    /// Override facoltativo per singola chiamata; `None` usa [`DEFAULT_TIMEOUT`].
    pub timeout: Option<Duration>,
}

/// Build the cargo argv for a `cargo_build` invocation (pure, testable).
///
/// `release` → `--release`; `features` → `--features <csv>`. Each element is a
/// single argv element, never string-interpolated.
///
/// Costruisce gli argv di cargo per `cargo_build` (puro, testabile).
/// `release` → `--release`; `features` → `--features <csv>`. Ogni elemento è un
/// singolo elemento argv, mai interpolato in stringa.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::runner::build_args;
/// assert_eq!(
///     build_args(true, &["a".to_string(), "b".to_string()]),
///     vec!["--release".to_string(), "--features".to_string(), "a,b".to_string()]
/// );
/// assert_eq!(build_args(false, &[]), Vec::<String>::new());
/// ```
pub fn build_args(release: bool, features: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    if release {
        args.push("--release".to_string());
    }
    if !features.is_empty() {
        args.push("--features".to_string());
        args.push(features.join(","));
    }
    args
}

/// Run `cargo build --message-format=json` in `args.repo_path` and return a
/// typed [`CheckOutcome`].
///
/// Esegue `cargo build --message-format=json` in `args.repo_path` e ritorna un
/// [`CheckOutcome`] tipato.
///
/// # Errors
/// Same as [`run_cargo`]: a compile error in the target is NOT a runner error;
/// it surfaces as `ok == false` with diagnostics.
///
/// Uguale a [`run_cargo`]: un errore di compilazione del target NON è un
/// errore del runner; emerge come `ok == false` con diagnostica.
pub fn cargo_build(args: &CargoBuildArgs) -> Result<CheckOutcome> {
    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        release = args.release,
        features = ?args.features,
        timeout = ?args.timeout,
        "running cargo build --message-format=json"
    );
    let start = std::time::Instant::now();

    let extra = build_args(args.release, &args.features);
    let timeout = args.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let outcome = run_cargo("build", &extra, &args.repo_path, timeout)?;

    let elapsed = start.elapsed();
    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        elapsed = ?elapsed,
        ok = outcome.ok,
        errors = outcome.summary.errors,
        warnings = outcome.summary.warnings,
        "cargo build finished"
    );

    Ok(outcome)
}

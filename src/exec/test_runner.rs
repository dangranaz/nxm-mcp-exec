//! `cargo_test` execution: runs `cargo test --format=json` (nightly only) and
//! feeds the libtest JSON stream to [`crate::exec::test_parser`].
//!
//! Esecuzione di `cargo_test`: esegue `cargo test --format=json` (solo
//! nightly) e alimenta lo stream JSON di libtest a
//! [`crate::exec::test_parser`].

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};

use super::cargo_runner::{
    cargo_bin, cargo_exec, cargo_is_nightly, is_allowed_subcommand, validate_repo_path, DEFAULT_TIMEOUT,
};
use super::test_parser::{parse_libtest_stream, TestOutcome};

use tracing;

/// Typed arguments for a `cargo_test` invocation.
///
/// Argomenti tipati per un'invocazione di `cargo_test`.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::test_runner::CargoTestArgs;
/// use std::path::PathBuf;
/// let args = CargoTestArgs {
///     repo_path: PathBuf::from("."),
///     crate_name: None,
///     filter: None,
///     timeout: None,
/// };
/// assert_eq!(args.repo_path, PathBuf::from("."));
/// ```
#[derive(Debug, Clone)]
pub struct CargoTestArgs {
    /// Target repository directory. Must exist and contain a `Cargo.toml`.
    pub repo_path: PathBuf,
    /// Optional `-p <crate>` package selector.
    pub crate_name: Option<String>,
    /// Optional test filter passed as a single argv element.
    pub filter: Option<String>,
    /// Optional per-call timeout override; `None` uses [`DEFAULT_TIMEOUT`].
    ///
    /// Override facoltativo per singola chiamata; `None` usa [`DEFAULT_TIMEOUT`].
    pub timeout: Option<Duration>,
}

/// Run `cargo test --format=json` (nightly only) in `args.repo_path` and feed
/// the libtest JSON stream to [`parse_libtest_stream`], returning a
/// [`TestOutcome`].
///
/// Esegue `cargo test --format=json` (solo nightly) in `args.repo_path` e
/// alimenta lo stream JSON di libtest a [`parse_libtest_stream`], ritornando
/// un [`TestOutcome`].
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::test_runner::CargoTestArgs;
/// use std::path::PathBuf;
/// let args = CargoTestArgs {
///     repo_path: PathBuf::from("."),
///     crate_name: None,
///     filter: None,
///     timeout: None,
/// };
/// // Requires a nightly toolchain; on stable, `cargo_test(&args)` returns a
/// // typed error instead of panicking.
/// assert_eq!(args.repo_path, PathBuf::from("."));
/// ```
///
/// # Format stability / Stabilità del formato
/// libtest `--format=json` is unstable and requires a nightly toolchain.
/// Strategy: detect the toolchain via [`cargo_is_nightly`]; if not nightly,
/// return a typed error (graceful skip, never a panic, never a hang, never
/// parsed human text). If nightly but `--format=json` is rejected by cargo,
/// the non-zero exit status is surfaced as a typed error too — never a fake
/// "all passed" outcome.
///
/// libtest `--format=json` è instabile e richiede una toolchain nightly.
/// Strategia: rilevare la toolchain via [`cargo_is_nightly`]; se non nightly,
/// ritornare un errore tipato (skip grazioso, mai un panic, mai un hang, mai
/// testo umano parsato). Se nightly ma `--format=json` viene rifiutato da
/// cargo, lo status di uscita non-zero viene surfacciato come errore tipato —
/// mai un falso "tutti passati".
pub fn cargo_test(args: &CargoTestArgs) -> Result<TestOutcome> {
    if !is_allowed_subcommand("test") {
        bail!("subcommand `test` is not allow-listed");
    }
    if !cargo_is_nightly() {
        bail!(
            "test JSON output requires a nightly toolchain (use `cargo +nightly`) \
             or cargo-nextest; the current toolchain is stable and cannot emit libtest JSON"
        );
    }
    validate_repo_path(&args.repo_path)?;

    // `-p <crate>` is a cargo-level flag: it must precede `--` so cargo (not
    // libtest) consumes it as a package selector.
    let mut cargo_args = Vec::new();
    if let Some(name) = &args.crate_name {
        cargo_args.push("-p".to_string());
        cargo_args.push(name.clone());
    }

    // `-- --format=json` is passed to the libtest runner (after `--`).
    let mut libtest_args = Vec::new();
    libtest_args.push("--format=json".to_string());
    if let Some(filter) = &args.filter {
        libtest_args.push(filter.clone());
    }

    let timeout = args.timeout.unwrap_or(DEFAULT_TIMEOUT);

    // `-Z unstable-options` is a cargo-level flag and must precede the
    // subcommand.
    let mut cmd = Command::new(cargo_bin());
    cmd.current_dir(&args.repo_path)
        .arg("-Z")
        .arg("unstable-options")
        .arg("test");
    for arg in &cargo_args {
        cmd.arg(arg);
    }
    cmd.arg("--");
    for arg in &libtest_args {
        cmd.arg(arg);
    }

    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        cargo_args = ?cargo_args,
        libtest_args = ?libtest_args,
        timeout = ?timeout,
        "running cargo test --format=json (nightly)"
    );

    let (stdout_str, _status) = cargo_exec(&mut cmd, timeout)?;

    // A non-zero exit status from cargo is NOT by itself an error: cargo exits
    // with status 1 when tests fail, but still emits the libtest JSON stream
    // describing those failures. We therefore parse stdout regardless of the
    // exit status — a fake "all passed" outcome is never returned.
    //
    // If the toolchain cannot emit JSON (stable, or nightly rejecting
    // `--format=json`), stdout is empty and we surface a typed error instead
    // of parsing human text or returning a fake outcome.
    if stdout_str.trim().is_empty() {
        bail!(
            "libtest JSON output is unavailable on this toolchain: cargo \
             emitted no stdout (nightly + -Z unstable-options required, or \
             cargo-nextest)"
        );
    }

    let outcome = parse_libtest_stream(&stdout_str);

    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        passed = outcome.passed,
        failed = outcome.failed,
        ignored = outcome.ignored,
        failures = outcome.failures.len(),
        "cargo test finished"
    );

    Ok(outcome)
}

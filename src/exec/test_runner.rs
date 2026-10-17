//! `cargo_test` execution: runs tests under the allow-list and returns a
//! [`TestOutcome`], using `cargo-nextest` when available (stable, machine-readable)
//! and falling back to the nightly libtest JSON path from exec-03b.
//!
//! Esecuzione di `cargo_test`: esegue i test sotto allow-list e ritorna un
//! [`TestOutcome`], usando `cargo-nextest` quando disponibile (stable, output
//! machine-readable) e ricadendo sul path nightly libtest JSON di exec-03b.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};

use super::cargo_runner::{
    cargo_bin, cargo_exec, cargo_is_nightly, is_allowed_subcommand, validate_repo_path,
    DEFAULT_TIMEOUT,
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

/// Detect whether the `cargo-nextest` binary is available on `PATH`.
///
/// Rileva se il binario `cargo-nextest` è disponibile su `PATH`.
///
/// Returns `true` when `cargo nextest --version` succeeds and reports
/// `nextest`; `false` otherwise.
///
/// Ritorna `true` quando `cargo nextest --version` ha successo e segnala
/// `nextest`; `false` altrimenti.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::test_runner::cargo_nextest_available;
/// // Result depends on whether cargo-nextest is installed; no assertion on the value.
/// let _ = cargo_nextest_available();
/// ```
pub fn cargo_nextest_available() -> bool {
    let output = Command::new("cargo")
        .arg("nextest")
        .arg("--version")
        .output();
    let available = match output {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            o.status.success() && s.contains("nextest")
        }
        Err(_) => false,
    };
    tracing::debug!(target: "nxm::exec::test_runner", cargo_nextest_available = available, "detected cargo-nextest");
    available
}

/// Run `cargo test` in `args.repo_path` and return a [`TestOutcome`].
///
/// Esegue `cargo test` in `args.repo_path` e ritorna un [`TestOutcome`].
///
/// # Backend selection / Selezione del backend
/// 1. If `cargo-nextest` is available, use `cargo nextest run --message-format
///    libtest-json` (machine-readable libtest JSON on stable, experimental gate
///    `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1` in the nextest env; see
///    [`crate::exec::nextest_runner`]). The libtest-json schema is the SAME
///    stream [`parse_libtest_stream`] already handles, so it is reused.
/// 2. Else, if the toolchain is nightly, use `cargo test -Z unstable-options
///    -- --format=json` and map via [`parse_libtest_stream`] (exec-03b path).
/// 3. Else, return a typed error (graceful skip: no panic, no hang, no parsed
///    human text).
///
/// 1. Se `cargo-nextest` è disponibile, usa `cargo nextest run --message-format
///    libtest-json` (libtest JSON machine-readable su stable, gate sperimentale
///    `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1` nell'env di nextest; vedi
///    [`crate::exec::nextest_runner`]). Lo schema libtest-json è lo STESSO
///    stream che [`parse_libtest_stream`] gestisce già, quindi viene riusato.
/// 2. Altrimenti, se la toolchain è nightly, usa `cargo test -Z unstable-options
///    -- --format=json` e mappa via [`parse_libtest_stream`] (path exec-03b).
/// 3. Altrimenti, ritorna un errore tipato (skip grazioso: no panic, no hang,
///    no testo umano parsato).
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
/// // On a machine without nextest or nightly, `cargo_test(&args)` returns a
/// // typed error instead of panicking.
/// assert_eq!(args.repo_path, PathBuf::from("."));
/// ```
pub fn cargo_test(args: &CargoTestArgs) -> Result<TestOutcome> {
    tracing::debug!(
        target: "nxm::exec::test_runner",
        repo = %args.repo_path.display(),
        filter = ?args.filter,
        "running cargo_test"
    );
    if !is_allowed_subcommand("test") {
        bail!("subcommand `test` is not allow-listed");
    }
    validate_repo_path(&args.repo_path)?;

    if cargo_nextest_available() {
        return super::nextest_runner::run_nextest(args);
    }

    if cargo_is_nightly() {
        return cargo_test_via_libtest_json(args);
    }

    bail!(
        "no test JSON backend available: cargo-nextest is not installed and \
         the toolchain is not nightly (cannot emit libtest JSON). \
         Install cargo-nextest or use `cargo +nightly`."
    );
}

/// Run `cargo test` via nightly libtest JSON (exec-03b path).
///
/// Esegue `cargo test` via nightly libtest JSON (path exec-03b).
fn cargo_test_via_libtest_json(args: &CargoTestArgs) -> Result<TestOutcome> {
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
        "cargo test finished (nightly libtest JSON)"
    );

    Ok(outcome)
}

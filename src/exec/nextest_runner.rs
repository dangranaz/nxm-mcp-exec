//! `cargo-nextest` backend for [`crate::exec::test_runner::cargo_test`].
//!
//! Backend `cargo-nextest` per [`crate::exec::test_runner::cargo_test`].
//!
//! The command is built through a **pure function** (`nextest_command_spec`)
//! that turns typed args into a command spec (program + argv + env), so the
//! invocation itself is unit-testable without spawning nextest. The real
//! execution reuses the shared, allow-listed `cargo` execution and maps the
//! machine-readable stream to a [`TestOutcome`].
//!
//! Il comando è costruito tramite una **funzione pura** (`nextest_command_spec`)
//! che trasforma gli argomenti tipati in una specifica di comando (programma +
//! argv + env), così l'invocazione è unit-testabile senza spawnare nextest.
//! L'esecuzione reale riusa l'esecuzione `cargo` condivisa in allow-list e mappa
//! lo stream machine-readable su un [`TestOutcome`].

use std::process::Command;

use anyhow::{bail, Result};

use super::cargo_runner::{cargo_bin, cargo_exec, DEFAULT_TIMEOUT};
use super::test_parser::{parse_libtest_stream, TestOutcome};
use super::test_runner::CargoTestArgs;

use tracing;

/// Specification of a concrete process invocation (pure data, never spawned).
///
/// Specifica concreta di un'invocazione di processo (dati puri, mai spawnati).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestCommandSpec {
    /// Program name to spawn (the `cargo` binary).
    pub program: String,
    /// Full argv, including the `nextest` subcommand chain.
    pub args: Vec<String>,
    /// Extra environment variables to inject in the child process.
    pub env: Vec<(String, String)>,
}

/// Build the pure spec of the `cargo nextest run` invocation.
///
/// Costruisce la specifica pura dell'invocazione `cargo nextest run`.
///
/// The invocation is `cargo nextest run --message-format libtest-json`
/// (`--message-format` is a nextest flag, emitted BEFORE any positional
/// filter), with the experimental gate `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`
/// injected in the child env.
///
/// L'invocazione è `cargo nextest run --message-format libtest-json`
/// (`--message-format` è un flag di nextest, emesso PRIMA di ogni filtro
/// posizionale), con il gate sperimentale `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`
/// iniettato nell'env del processo child.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::nextest_runner::nextest_command_spec;
/// use nxm_mcp_exec::exec::test_runner::CargoTestArgs;
/// use std::path::PathBuf;
/// let args = CargoTestArgs { repo_path: PathBuf::from("."), crate_name: None, filter: None, timeout: None };
/// let spec = nextest_command_spec(&args).expect("spec");
/// assert_eq!(spec.args[0], "nextest");
/// ```
pub fn nextest_command_spec(args: &CargoTestArgs) -> Result<NextestCommandSpec> {
    // Full argv after the `cargo` program: the `nextest` subcommand chain.
    let mut nextest_args = vec!["nextest".to_string(), "run".to_string()];

    // `-p <crate>` is a cargo-level flag: it precedes `run`'s own flags.
    if let Some(name) = &args.crate_name {
        nextest_args.push("-p".to_string());
        nextest_args.push(name.clone());
    }
    // `--message-format` is a nextest-level flag (NOT a test-binary flag): it
    // precedes any positional filter and there is no `--` separator at all.
    // `--format=json` does not exist in nextest (`cargo -- --format=json` would
    // be swallowed by the test binaries, leaving stdout empty).
    nextest_args.push("--message-format".to_string());
    nextest_args.push("libtest-json".to_string());
    if let Some(filter) = &args.filter {
        nextest_args.push(filter.clone());
    }

    // The `libtest-json` message format is experimental in nextest: without
    // this gate the process refuses to emit the JSON stream.
    let env = vec![(
        "NEXTEST_EXPERIMENTAL_LIBTEST_JSON".to_string(),
        "1".to_string(),
    )];

    Ok(NextestCommandSpec {
        program: cargo_bin(),
        args: nextest_args,
        env,
    })
}

/// Run `cargo test` via `cargo-nextest` and map the machine-readable stream.
///
/// Esegue `cargo test` via `cargo-nextest` e mappa lo stream machine-readable.
///
/// # Errors
/// Returns a typed error when nextest emits no machine-readable stdout
/// (never a fake "all passed" outcome, never parsed human text).
///
/// Ritorna un errore tipato quando nextest non emette stdout machine-readable
/// (mai un falso "tutto passato", mai testo umano parsato).
pub fn run_nextest(args: &CargoTestArgs) -> Result<TestOutcome> {
    let spec = nextest_command_spec(args)?;

    let timeout = args.timeout.unwrap_or(DEFAULT_TIMEOUT);

    let mut cmd = Command::new(&spec.program);
    cmd.current_dir(&args.repo_path);
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    for arg in &spec.args {
        cmd.arg(arg);
    }

    tracing::debug!(
        target: "nxm::exec::nextest_runner",
        repo = %args.repo_path.display(),
        args = ?spec.args,
        env = ?spec.env,
        timeout = ?timeout,
        "running nextest (spec)"
    );

    let (stdout_str, _status) = cargo_exec(&mut cmd, timeout)?;

    // A non-zero exit status from nextest is NOT by itself an error: nextest
    // exits 1 when tests fail but still emits the JSON stream describing those
    // failures. Empty stdout, instead, means no machine-readable stream.
    if stdout_str.trim().is_empty() {
        bail!("nextest JSON output is unavailable: cargo-nextest emitted no stdout");
    }

    let outcome = parse_libtest_stream(&stdout_str);

    tracing::debug!(
        target: "nxm::exec::nextest_runner",
        repo = %args.repo_path.display(),
        passed = outcome.passed,
        failed = outcome.failed,
        ignored = outcome.ignored,
        failures = outcome.failures.len(),
        "cargo nextest finished"
    );

    Ok(outcome)
}

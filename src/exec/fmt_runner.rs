//! `cargo_fmt_check` execution: runs `cargo fmt -- --check -l` under the
//! allow-list and maps exit code + file list to `{ok, offending_files}`.
//!
//! Esecuzione di `cargo_fmt_check`: esegue `cargo fmt -- --check -l`
//! sotto allow-list e mappa exit code + elenco file in `{ok, offending_files}`.
//!
//! `cargo fmt --check` is read-only: it never rewrites the target files.
//! `ok` is derived from the exit code (0 = already formatted);
//! `offending_files` is the list of files rustfmt reports (one per line via
//! `-l`). The diff content is never parsed: only file names + exit code.
//!
//! `cargo fmt --check` è read-only: non riscrive mai i file target.
//! `ok` deriva dall'exit code (0 = già formattato);
//! `offending_files` è l'elenco dei file segnalati da rustfmt (uno per riga
//! tramite `-l`). Il contenuto del diff non viene mai parsato: solo nomi file
//! + exit code.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::cargo_runner::{
    cargo_bin, cargo_exec, is_allowed_subcommand, validate_repo_path, DEFAULT_TIMEOUT,
};

use tracing;

/// Typed arguments for a `cargo_fmt_check` invocation.
///
/// Argomenti tipati per un'invocazione di `cargo_fmt_check`.
#[derive(Debug, Clone)]
pub struct FmtArgs {
    /// Target repository directory. Must exist and contain a `Cargo.toml`.
    pub repo_path: PathBuf,
    /// Optional per-call timeout override; `None` uses [`DEFAULT_TIMEOUT`].
    ///
    /// Override facoltativo per singola chiamata; `None` usa [`DEFAULT_TIMEOUT`].
    pub timeout: Option<Duration>,
}

/// Typed outcome of a `cargo fmt --check` invocation.
///
/// Esito tipato di un'invocazione di `cargo fmt --check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FmtOutcome {
    /// `true` when the target repo is already formatted (exit code 0).
    ///
    /// `true` quando il repo target è già formattato (exit code 0).
    pub ok: bool,
    /// Files rustfmt reports as not formatted (one per line from `-l`).
    ///
    /// File che rustfmt segnala come non formattati (uno per riga da `-l`).
    pub offending_files: Vec<String>,
}

/// Pure mapping from `cargo fmt --check` output to [`FmtOutcome`].
///
/// Mappatura pura dall'output di `cargo fmt --check` a [`FmtOutcome`].
///
/// `ok` comes from the exit code; `offending_files` are the non-empty lines
/// of stdout. The diff content is never parsed.
///
/// `ok` deriva dall'exit code; `offending_files` sono le righe non vuote
/// di stdout. Il contenuto del diff non viene mai parsato.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::fmt_runner::parse_fmt_output;
/// let clean = parse_fmt_output(true, "");
/// assert!(clean.ok);
/// assert!(clean.offending_files.is_empty());
/// let dirty = parse_fmt_output(false, "src/a.rs\nsrc/b.rs\n");
/// assert!(!dirty.ok);
/// assert_eq!(dirty.offending_files, vec!["src/a.rs", "src/b.rs"]);
/// ```
pub fn parse_fmt_output(exit_success: bool, stdout: &str) -> FmtOutcome {
    let offending_files: Vec<String> = stdout
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    FmtOutcome {
        ok: exit_success,
        offending_files,
    }
}

/// Run `cargo fmt -- --check -l` in `args.repo_path` and return a
/// [`FmtOutcome`].
///
/// Esegue `cargo fmt -- --check -l` in `args.repo_path` e ritorna un
/// [`FmtOutcome`].
///
/// # Errors
/// Same as [`cargo_exec`]: `repo_path` must be a usable cargo repo and `cargo`
/// must be spawnable. A non-zero exit from `fmt --check` is NOT a runner
/// error: it surfaces as `ok == false` with the offending files listed.
///
/// Uguale a [`cargo_exec`]: `repo_path` deve essere un repo cargo utilizzabile
/// e `cargo` deve essere avviabile. Un exit non zero di `fmt --check` NON è
/// un errore del runner: emerge come `ok == false` con i file offending.
pub fn cargo_fmt_check(args: &FmtArgs) -> Result<FmtOutcome> {
    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        timeout = ?args.timeout,
        "running cargo fmt -- --check -l"
    );
    let start = std::time::Instant::now();

    if !is_allowed_subcommand("fmt") {
        anyhow::bail!("subcommand `fmt` is not allow-listed");
    }
    validate_repo_path(&args.repo_path)?;

    let mut cmd = std::process::Command::new(cargo_bin());
    cmd.current_dir(&args.repo_path)
        .arg("fmt")
        .arg("--")
        .arg("--check")
        .arg("-l");

    let timeout = args.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let (stdout, status) = cargo_exec(&mut cmd, timeout)?;

    let outcome = parse_fmt_output(status.success(), &stdout);

    let elapsed = start.elapsed();
    tracing::debug!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        elapsed = ?elapsed,
        ok = outcome.ok,
        files = outcome.offending_files.len(),
        "cargo fmt --check finished"
    );

    Ok(outcome)
}

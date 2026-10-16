//! Runner: execute allow-listed cargo commands in a target repo directory.
//!
//! Runner: esegue comandi cargo in allow-list nella directory di un repo target.
//!
//! Security (non-negotiable) / Sicurezza (non negoziabile):
//! - Only the allow-listed program `cargo` with an allow-listed subcommand
//!   (`check` or `build`) is ever spawned. There is NO arbitrary shell:
//!   arguments are built from typed structs, never from free-form strings.
//! - The command runs in the caller-provided `repo_path` and is read-only on
//!   the source (`cargo check` does not modify sources; `cargo build` writes
//!   only to `target/`, which is expected and inside the repo).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::parser::{parse_cargo_stream, CheckOutcome};

/// The only program this server is ever allowed to spawn.
///
/// L'unico programma che questo server può eseguire.
const ALLOWED_PROGRAM: &str = "cargo";

/// Default timeout for a cargo invocation.
///
/// Timeout di default per un'invocazione di cargo.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Returns `true` only for the allow-listed cargo subcommands.
///
/// Ritorna `true` solo per i sottocomandi cargo in allow-list.
pub fn is_allowed_subcommand(subcommand: &str) -> bool {
    matches!(subcommand, "check" | "build")
}

/// Typed arguments for a `cargo_check` invocation.
///
/// Argomenti tipati per un'invocazione di `cargo_check`.
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

/// Typed arguments for a `cargo_build` invocation.
///
/// Argomenti tipati per un'invocazione di `cargo_build`.
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

/// Run an allow-listed cargo subcommand (`check` or `build`) with `extra_args`
/// in `repo_path`, returning a typed [`CheckOutcome`].
///
/// Esegue un sottocomando cargo in allow-list (`check` o `build`) con
/// `extra_args` in `repo_path`, ritornando un [`CheckOutcome`] tipato.
///
/// # Errors
/// Returns an error if the subcommand is not allow-listed, `repo_path` is not a
/// usable cargo repo, or `cargo` cannot be spawned. A compile *error* in the
/// target is NOT a runner error: it surfaces as `ok == false` with diagnostics.
///
/// Ritorna errore se il sottocomando non è in allow-list, `repo_path` non è un
/// repo cargo utilizzabile, o `cargo` non può essere avviato. Un *errore di
/// compilazione* del target NON è un errore del runner: emerge come
/// `ok == false` con diagnostica.
pub fn run_cargo(
    subcommand: &str,
    extra_args: &[String],
    repo_path: &Path,
    timeout: Duration,
) -> Result<CheckOutcome> {
    if !is_allowed_subcommand(subcommand) {
        bail!("subcommand `{subcommand}` is not allow-listed (only `check` and `build`)");
    }
    validate_repo_path(repo_path)?;

    let mut cmd = Command::new(ALLOWED_PROGRAM);
    cmd.current_dir(repo_path)
        .arg(subcommand)
        .arg("--message-format=json");
    for arg in extra_args {
        cmd.arg(arg);
    }

    tracing::info!(
        target: "nxm::exec",
        repo = %repo_path.display(),
        subcommand = subcommand,
        args = ?extra_args,
        timeout = ?timeout,
        "running cargo {subcommand} --message-format=json"
    );

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn `cargo {subcommand}`"))?;

    // Pipe the output on reader threads so a large stdout/stderr cannot
    // deadlock the child while we wait with a timeout.
    let mut stdout = child
        .stdout
        .take()
        .context(format!("cargo {subcommand} spawned without piped stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .context(format!("cargo {subcommand} spawned without piped stderr"))?;
    let stdout_thread = thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let _stderr_thread = thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let status = wait_with_timeout(&mut child, timeout)?;

    let stdout_str = stdout_thread.join().unwrap_or_default();

    let outcome = parse_cargo_stream(&stdout_str);

    tracing::info!(
        target: "nxm::exec",
        repo = %repo_path.display(),
        subcommand = subcommand,
        success = status.success(),
        ok = outcome.ok,
        errors = outcome.summary.errors,
        warnings = outcome.summary.warnings,
        "cargo {subcommand} finished"
    );

    Ok(outcome)
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
    run_cargo("check", &extra, &args.repo_path, timeout)
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
    let extra = build_args(args.release, &args.features);
    let timeout = args.timeout.unwrap_or(DEFAULT_TIMEOUT);
    run_cargo("build", &extra, &args.repo_path, timeout)
}

/// Wait for `child` to exit, killing it if `timeout` elapses first.
///
/// Aspetta che `child` termini, uccidendolo se `timeout` scade prima.
///
/// Returns `Ok(status)` when the child exits on its own, or a typed `Err`
/// (containing "timed out") when it had to be killed. Never panics and never
/// hangs: the caller always regains control within `timeout` plus one poll
/// interval.
///
/// Restituisce `Ok(status)` se il child esce da solo, oppure un `Err` tipato
/// (contenente "timed out") se è stato ucciso. Non panica mai e non appende
/// mai: il caller riottene il controllo entro `timeout` più un intervallo di
/// polling.
pub fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Result<ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("process timed out after {:?}", timeout);
                }
                thread::sleep(Duration::from_millis(200));
            }
            Err(e) => bail!("failed to wait for child process: {e}"),
        }
    }
}

/// Validate that `repo_path` is a usable cargo repository directory.
///
/// Valida che `repo_path` sia una directory di repo cargo utilizzabile.
fn validate_repo_path(repo_path: &Path) -> Result<()> {
    if !repo_path.exists() {
        bail!("repo_path does not exist: {}", repo_path.display());
    }
    if !repo_path.is_dir() {
        bail!("repo_path is not a directory: {}", repo_path.display());
    }
    if !repo_path.join("Cargo.toml").is_file() {
        bail!(
            "repo_path has no Cargo.toml (not a cargo repo): {}",
            repo_path.display()
        );
    }
    Ok(())
}

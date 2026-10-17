//! Generic cargo command execution shared by `cargo_check`, `cargo_build`,
//! `cargo_test` and `cargo_clippy`.
//!
//! Esecuzione generica di comandi cargo condivisa da `cargo_check`,
//! `cargo_build`, `cargo_test` e `cargo_clippy`.
//!
//! Security (non-negotiable) / Sicurezza (non negoziabile):
//! - Only the allow-listed program `cargo` with an allow-listed subcommand
//!   (`check`, `build`, `test`, `nextest` or `clippy`) is ever spawned. There is
//!   NO arbitrary shell: arguments are built from typed structs, never from
//!   free-form strings.
//! - The command runs in the caller-provided `repo_path`.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::parser::parse_cargo_stream;

use tracing;

/// Default timeout for a cargo invocation.
///
/// Timeout di default per un'invocazione di cargo.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Returns the cargo binary to use, honouring the `NXM_CARGO` environment
/// variable (useful for tests that need to point at a nightly toolchain while
/// the default `cargo` on `PATH` is stable). Defaults to `"cargo"`.
///
/// Ritorna il binario cargo da usare, rispettando la variabile d'ambiente
/// `NXM_CARGO` (utile per i test che devono puntare a una toolchain nightly
/// mentre il `cargo` di default su `PATH` è stable). Default: `"cargo"`.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::cargo_runner::cargo_bin;
/// let bin = cargo_bin();
/// assert!(!bin.is_empty());
/// ```
pub fn cargo_bin() -> String {
    let bin = std::env::var("NXM_CARGO").unwrap_or_else(|_| "cargo".to_string());
    tracing::debug!(target: "nxm::exec::cargo_runner", cargo_bin = %bin, "resolved cargo binary");
    bin
}

/// Returns `true` only for the allow-listed cargo subcommands.
///
/// Ritorna `true` solo per i sottocomandi cargo in allow-list.
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::cargo_runner::is_allowed_subcommand;
/// assert!(is_allowed_subcommand("check"));
/// assert!(is_allowed_subcommand("build"));
/// assert!(is_allowed_subcommand("test"));
/// assert!(is_allowed_subcommand("nextest"));
/// assert!(is_allowed_subcommand("clippy"));
/// assert!(!is_allowed_subcommand("run"));
/// ```
pub fn is_allowed_subcommand(subcommand: &str) -> bool {
    let allowed = matches!(
        subcommand,
        "check" | "build" | "test" | "nextest" | "clippy"
    );
    tracing::debug!(target: "nxm::exec::cargo_runner", subcommand, allowed, "checked allow-listed subcommand");
    allowed
}

/// Detect whether the cargo toolchain is nightly, which is required to emit
/// libtest JSON output (`cargo test -Z unstable-options --format=json`).
///
/// Rileva se la toolchain cargo è nightly, necessario per emettere l'output
/// JSON di libtest (`cargo test -Z unstable-options --format=json`).
///
/// # Example
/// ```
/// use nxm_mcp_exec::exec::cargo_runner::cargo_is_nightly;
/// // Result depends on the installed toolchain; no assertion on the value.
/// let _ = cargo_is_nightly();
/// ```
pub fn cargo_is_nightly() -> bool {
    let output = Command::new(cargo_bin()).arg("--version").output();
    let is_nightly = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("nightly"),
        Err(_) => false,
    };
    tracing::debug!(target: "nxm::exec::cargo_runner", is_nightly, "detected cargo toolchain");
    is_nightly
}

/// Execute an allow-listed cargo subcommand with `extra_args` in `repo_path`,
/// capturing stdout as a string and the child exit status.
///
/// Esegue un sottocomando cargo in allow-list con `extra_args` in
/// `repo_path`, catturando stdout come stringa e lo status di uscita del child.
///
/// The stdout is piped on a reader thread so a large output cannot deadlock
/// the child while we wait with a timeout.
///
/// Lo stdout è pipato su un thread reader così un output grande non può
/// causare deadlock sul child mentre aspettiamo con un timeout.
pub(crate) fn cargo_exec(cmd: &mut Command, timeout: Duration) -> Result<(String, ExitStatus)> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn cargo command")?;

    let mut stdout = child
        .stdout
        .take()
        .context("cargo spawned without piped stdout")?;
    let mut stderr = child
        .stderr
        .take()
        .context("cargo spawned without piped stderr")?;
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

    Ok((stdout_str, status))
}

/// Wait for `child` to exit, killing it if `timeout` elapses first.
///
/// Aspetta che `child` termini, uccidendolo se `timeout` scade prima.
///
/// Returns `Ok(status)` when the child exits on its own, or a typed `Err`
/// (containing "timed out") when it had to be killed. Never panics and never
/// hangs.
///
/// Restituisce `Ok(status)` se il child esce da solo, oppure un `Err` tipato
/// (contenente "timed out") se è stato ucciso. Non panica mai e non appende
/// mai.
/// Wait for `child` to exit, killing it if `timeout` elapses first.
///
/// Aspetta che `child` termini, uccidendolo se `timeout` scade prima.
///
/// Returns `Ok(status)` when the child exits on its own, or a typed `Err`
/// (containing "timed out") when it had to be killed. Never panics and never
/// hangs.
///
/// Restituisce `Ok(status)` se il child esce da solo, oppure un `Err` tipato
/// (contenente "timed out") se è stato ucciso. Non panica mai e non appende
/// mai.
///
/// # Example
/// ```
/// use std::process::Command;
/// use nxm_mcp_exec::exec::cargo_runner::wait_with_timeout;
/// use std::time::Duration;
/// let mut child = Command::new("true").spawn().expect("spawn true");
/// let status = wait_with_timeout(&mut child, Duration::from_secs(5)).expect("wait");
/// assert!(status.success());
/// ```
pub fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Result<ExitStatus> {
    tracing::debug!(
        target: "nxm::exec::cargo_runner",
        timeout = ?timeout,
        "waiting for child process"
    );
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let elapsed = start.elapsed();
                tracing::debug!(
                    target: "nxm::exec::cargo_runner",
                    elapsed = ?elapsed,
                    success = status.success(),
                    "child process exited"
                );
                return Ok(status);
            }
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
pub(crate) fn validate_repo_path(repo_path: &Path) -> Result<()> {
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

/// Run an allow-listed cargo subcommand (`check` or `build`) with `extra_args`
/// in `repo_path`, returning a typed [`CheckOutcome`].
///
/// Esegue un sottocomando cargo in allow-list (`check` o `build`) con
/// `extra_args` in `repo_path`, ritornando un [`CheckOutcome`] tipato.
///
/// # Example
/// ```no_run
/// use nxm_mcp_exec::exec::cargo_runner::run_cargo;
/// use std::path::Path;
/// let outcome = run_cargo("check", &[], Path::new("."), std::time::Duration::from_secs(120));
/// assert!(outcome.is_ok() || outcome.is_err());
/// ```
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
) -> Result<crate::exec::parser::CheckOutcome> {
    tracing::debug!(
        target: "nxm::exec::cargo_runner",
        repo = %repo_path.display(),
        subcommand,
        args = ?extra_args,
        timeout = ?timeout,
        "running cargo subcommand"
    );
    let start = Instant::now();
    if !is_allowed_subcommand(subcommand) {
        bail!(
            "subcommand `{subcommand}` is not allow-listed (only `check`, `build`, \
             `test`, `nextest` and `clippy`)"
        );
    }
    validate_repo_path(repo_path)?;

    let mut cmd = Command::new(cargo_bin());
    cmd.current_dir(repo_path)
        .arg(subcommand)
        .arg("--message-format=json");
    for arg in extra_args {
        cmd.arg(arg);
    }

    // For `check`/`build` a non-zero exit (e.g. compile errors) is NOT a runner
    // error: stdout still carries the JSON diagnostics we parse.
    let (stdout_str, _status) = cargo_exec(&mut cmd, timeout)?;
    let outcome = parse_cargo_stream(&stdout_str);

    let elapsed = start.elapsed();
    tracing::debug!(
        target: "nxm::exec::cargo_runner",
        repo = %repo_path.display(),
        subcommand,
        elapsed = ?elapsed,
        ok = outcome.ok,
        "cargo subcommand finished"
    );

    Ok(outcome)
}

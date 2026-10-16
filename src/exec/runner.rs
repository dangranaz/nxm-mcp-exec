//! Runner: execute allow-listed cargo commands in a target repo directory.
//!
//! Runner: esegue comandi cargo in allow-list nella directory di un repo target.
//!
//! Security (non-negotiable) / Sicurezza (non negoziabile):
//! - Only the single allow-listed program `cargo` with the fixed subcommand
//!   `check` is ever spawned. There is NO arbitrary shell: arguments are built
//!   from a typed struct, never from free-form strings.
//! - The command runs in the caller-provided `repo_path` and is read-only on
//!   the source (`cargo check` does not modify sources).
//!
//! - Viene eseguito solo il programma in allow-list `cargo` con il sottocomando
//!   fisso `check`. NIENTE shell arbitraria: gli argomenti nascono da uno struct
//!   tipato, mai da stringhe libere.
//! - Il comando gira nella `repo_path` fornita dal chiamante ed è read-only sul
//!   sorgente (`cargo check` non modifica i file).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::parser::{parse_cargo_stream, CheckOutcome};

/// The only program this server is ever allowed to spawn, in this task.
///
/// L'unico programma che questo server può eseguire, in questo task.
const ALLOWED_PROGRAM: &str = "cargo";

/// The only subcommand allowed by this tool.
///
/// L'unico sottocomando consentito da questo tool.
const ALLOWED_SUBCOMMAND: &str = "check";

/// Default timeout for a `cargo check` invocation.
///
/// Timeout di default per un'invocazione di `cargo check`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

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
    validate_repo_path(&args.repo_path)?;

    let mut cmd = Command::new(ALLOWED_PROGRAM);
    cmd.current_dir(&args.repo_path)
        .arg(ALLOWED_SUBCOMMAND)
        .arg("--message-format=json");

    if args.workspace {
        cmd.arg("--workspace");
    }
    if let Some(name) = &args.crate_name {
        // `-p <crate>`: the crate name is passed as a single argv element, so it
        // cannot inject further flags or shell metacharacters.
        cmd.arg("-p").arg(name);
    }

    tracing::info!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        workspace = args.workspace,
        crate_name = ?args.crate_name,
        timeout = ?args.timeout,
        "running cargo check --message-format=json"
    );

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn `{ALLOWED_PROGRAM} {ALLOWED_SUBCOMMAND}`"))?;

    // Pipe the output on reader threads so a large stdout/stderr cannot
    // deadlock the child while we wait with a timeout.
    let mut stdout = child
        .stdout
        .take()
        .context("cargo check spawned without piped stdout")?;
    let mut stderr = child
        .stderr
        .take()
        .context("cargo check spawned without piped stderr")?;
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

    let timeout = args.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let status = wait_with_timeout(&mut child, timeout)?;

    let stdout_str = stdout_thread.join().unwrap_or_default();

    let outcome = parse_cargo_stream(&stdout_str);

    tracing::info!(
        target: "nxm::exec",
        repo = %args.repo_path.display(),
        success = status.success(),
        ok = outcome.ok,
        errors = outcome.summary.errors,
        warnings = outcome.summary.warnings,
        "cargo check finished"
    );

    Ok(outcome)
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

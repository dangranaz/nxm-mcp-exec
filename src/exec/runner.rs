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

use std::path::{Path, PathBuf};
use std::process::Command;

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
        "running cargo check --message-format=json"
    );

    let output = cmd
        .output()
        .with_context(|| format!("failed to spawn `{ALLOWED_PROGRAM} {ALLOWED_SUBCOMMAND}`"))?;

    // cargo writes the JSON stream to stdout; stderr holds progress/status text.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let outcome = parse_cargo_stream(&stdout);

    tracing::info!(
        target: "nxm::exec",
        ok = outcome.ok,
        errors = outcome.summary.errors,
        warnings = outcome.summary.warnings,
        "cargo check finished"
    );

    Ok(outcome)
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

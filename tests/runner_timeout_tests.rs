//! Tests for the sync timeout on `cargo check`.
//!
//! Test per il timeout sincrono su `cargo check`.
//!
//! Design decision (documented here): the runner is hard-coded to spawn only
//! `cargo check`, so a "slow cargo" fixture is fragile and repo-dependent. The
//! wait-with-timeout logic is therefore extracted into the pure, public
//! `runner::wait_with_timeout` and tested against a controlled system process
//! (`sleep`). `cargo_check` itself is then tested for the default timeout value
//! and for the timeout path (a zero timeout deterministically kills a freshly
//! spawned `cargo`, which needs ~20 ms to start and cannot exit on the first
//! poll).

use std::process::Command;
use std::time::Duration;

use nxm_mcp_exec::exec::runner::{wait_with_timeout, CargoCheckArgs, DEFAULT_TIMEOUT};

#[test]
fn default_timeout_is_120_seconds() {
    assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(120));
}

#[test]
fn wait_with_timeout_kills_process_exceeding_timeout() {
    // `sleep 30` runs far beyond the 50 ms timeout: it must be killed and
    // reported as a typed timeout error, not hang or panic. The 50 ms override
    // proves a non-default timeout is actually applied.
    let mut child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("`sleep` must be available on this system");
    let result = wait_with_timeout(&mut child, Duration::from_millis(50));

    assert!(result.is_err(), "expected a timeout error, got {result:?}");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("timed out"),
        "error must mention the timeout: {err}"
    );
    // The child must be dead, not left orphaned.
    assert!(
        child.try_wait().unwrap().is_some(),
        "child must be killed by the timeout"
    );
}

#[test]
fn wait_with_timeout_returns_ok_for_process_under_timeout() {
    // `true` exits immediately, well within the 5 s timeout.
    let mut child = Command::new("true")
        .spawn()
        .expect("`true` must be available on this system");
    let result = wait_with_timeout(&mut child, Duration::from_secs(5));

    assert!(result.is_ok(), "expected Ok, got {result:?}");
    assert!(result.unwrap().success());
}

#[test]
fn cargo_check_zero_timeout_returns_timed_out_error() {
    // A zero timeout cannot elapse between `spawn` and the first poll, so a
    // freshly spawned `cargo` (which needs ~20 ms to start) is still running
    // and gets killed: deterministic exercise of the typed timeout error.
    let dir = tempfile::TempDir::new().expect("temp dir");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let args = CargoCheckArgs {
        repo_path: dir.path().to_path_buf(),
        crate_name: None,
        workspace: false,
        timeout: Some(Duration::ZERO),
    };

    let result = nxm_mcp_exec::exec::runner::cargo_check(&args);
    assert!(
        result.is_err(),
        "expected a timeout error for a zero timeout, got {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("timed out"),
        "error must mention the timeout: {err}"
    );
}

#[test]
fn cargo_check_default_timeout_succeeds_on_valid_repo() {
    // Regression: a valid repo with the default (120 s) timeout completes
    // normally and returns a typed outcome.
    let dir = tempfile::TempDir::new().expect("temp dir");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let args = CargoCheckArgs {
        repo_path: dir.path().to_path_buf(),
        crate_name: None,
        workspace: false,
        timeout: None, // default 120 s
    };

    let outcome = nxm_mcp_exec::exec::runner::cargo_check(&args).expect("cargo check succeeds");
    assert!(outcome.ok);
    assert_eq!(outcome.summary.errors, 0);
    assert!(outcome.diagnostics.is_empty());
}

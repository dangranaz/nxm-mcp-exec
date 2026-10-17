# Changelog

All notable changes to `nxm-mcp-exec` are documented here. Format loosely based
on [Keep a Changelog](https://keepachangelog.com/); versions follow SemVer.

## [Unreleased]

### Added
- **Tool `cargo_clippy`** (`work/exec-04-cargo-clippy`).
  - `clippy` added to the allow-list alongside `check`, `build`, `test` and
    `nextest`; `tools/list` advertises `cargo_clippy` with schema
    `{repo_path(req), deny_warnings?}`.
  - New `exec::clippy_runner::cargo_clippy` runs `cargo clippy
    --message-format=json` (argv-only, no shell) and reuses
    `exec::parser::parse_cargo_stream` — same
    `{ok, diagnostics:[{level,code,file,line,message}], summary}` schema, with
    clippy lint names in `code` (e.g. `clippy::assertions_on_constants`).
  - `deny_warnings` (default false): warnings count as errors for the `ok`
    flag. Enforcement is computed on the parsed summary, NOT via
    `-- -D warnings`, so diagnostics keep their true `warning` level (denying
    via argv would re-emit every lint as `error`, destroying the severity
    information the agent needs). Documented in the tool description.
  - Handler split (≤300-lines rule): tool implementations (argument parsing
    + invocation) moved from `src/handler.rs` to new `src/tools.rs`
    (`handler::dispatch` routes via `tools::run_tool`); `tools/list` schema
    stays in `handler.rs` (the public tool contract).
  - Tests: `tests/clippy_handler_tests.rs` (tool advertisement + schema,
    integration on a real lint fixture, `deny_warnings` flip with mutation
    check, clean repo) + allow-list coverage in `tests/handler_tests.rs`.
- **Scaffold + `cargo_check` tracer-bullet** (`work/exec-01-scaffold-cargo-check`).
- **Tool `cargo_build`** (`work/exec-02-cargo-build`).
  - New `exec::runner::run_cargo` shared by `cargo_check` and `cargo_build`;
    `cargo_check` behaviour unchanged.
  - `cargo_build` reuses `exec::parser::parse_cargo_stream` (no new parser) and
    returns the same `{ok, diagnostics:[{level,code,file,line,message}], summary}`
    schema.
  - `release` → `--release`; `features` → `--features <csv>` (single argv
    elements, never string-interpolated).
  - `build` added to the allow-list alongside `check`; non-allow-listed
    subcommands are rejected.
  - `tools/list` advertises `cargo_build` with schema
    `{repo_path(req), release?, features?}`.
  - Tests in `tests/handler_tests.rs`: tool advertisement + schema, integration
    via `handler::dispatch` (clean repo → `ok:true`; broken repo → `ok:false`
    + error diagnostic), `build_args` argv construction, allow-list rejection.
- **Pure parser `exec::test_parser` for libtest JSON output** (`work/exec-03a-libtest-parser`).

- **Tool `cargo_test`** (`work/exec-03b-cargo-test`).
  - `test` added to the allow-list alongside `check` and `build`; `is_allowed_subcommand` covers all three.
  - New `exec::runner::cargo_runner` module with shared process execution (`cargo_exec`, `wait_with_timeout`, `validate_repo_path`, `cargo_is_nightly`); `exec::runner` kept to `cargo_check`/`cargo_build` wrappers (all files ≤300 lines).
  - `cargo_test` runs `cargo -Z unstable-options test -- --format=json` (nightly only) and feeds the stream to `exec::test_parser::parse_libtest_stream`, returning `{passed, failed, ignored, failures:[{name,message}]}`.
  - `crate` → `-p <crate>`; `filter` → test filter argument (single argv, never interpolated).
  - Format stability: `cargo_is_nightly()` detects the toolchain; on non-nightly, `cargo_test` returns a typed, readable error (graceful skip, no panic, no hang, no human-text parsing). `NXM_CARGO` env var overrides the cargo binary (testability hook).
  - `tools/list` advertises `cargo_test` with schema `{repo_path(req), crate?, filter?}`.
  - Tests in `tests/handler_tests.rs`: tool advertisement + schema, graceful skip on stable toolchain, integration tests (green/red/filter, ignored when nightly JSON format is unavailable).
  - Public types `TestOutcome { passed, failed, ignored, failures }` and
    `TestFailure { name, message }`, both serializable.
  - `parse_libtest_stream` converts a libtest JSON stream into a `TestOutcome`.
  - Defensive: non-JSON/blank lines skipped, unknown event types ignored, no
    panic on format drift.
  - Failure `message` = first non-empty line of the event's `stdout` (never the
    whole dump), falling back to the `message` field.
  - Counts derive from individual `test` events; the `suite` event is read but
    does not override them.
  - No process execution, no runner/handler wiring (that is `exec-03b`).
  - Tests in `tests/test_parser_tests.rs`: serialized shape, all-green suite,
    failure extraction, ignored counting, noise robustness, count consistency,
    empty stream.
- **`cargo-nextest` backend for `cargo_test`** (`work/exec-03c-cargo-test-nextest`).
  - `nextest` added to the allow-list alongside `check`, `build` and `test`.
  - Backend selection strategy: `cargo-nextest` if available (stable,
    `libtest-json` machine-readable stream); else nightly libtest JSON
    (exec-03b path); else typed graceful skip error (no panic, no hang, no
    human text).
  - **Fix (exec-03c-fix, 2026-10-17): real nextest invocation.** The previous
    invocation (`cargo nextest run -- --format=json`, flag passed AFTER `--`) was
    wrong and produced an empty stdout even with nextest installed; `--format=json`
    does not exist in nextest. The correct invocation is
    `cargo nextest run --message-format libtest-json` (a nextest-level flag,
    BEFORE any positional filter) with the mandatory experimental gate
    `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1` set in the nextest process env. The
    command is now built by the pure function
    `exec::nextest_runner::nextest_command_spec` (argv+env, unit-tested without
    spawning) and executed by `exec::nextest_runner::run_nextest`.
  - **Parser de-duplication.** Empirically verified against real nextest
    0.9.148 output: `--message-format libtest-json` emits the SAME libtest
    schema (`event: "ok"|"failed"|"ignored"`, FQCN names
    `<crate>::<lib>$<test>`, failure message in the `stdout` field). The
    redundant `parse_nextest_stream` (built on an invented
    `event:"passed"` schema) was removed; consumption goes through the existing
    `parse_libtest_stream`. The old static fixture was invented and the new one
    is captured verbatim from real output.
  - **Integration tests run REAL nextest (no skip):** green/red/filter tests in
    `tests/nextest_tests.rs` require cargo-nextest installed
    (`cargo install cargo-nextest --locked`, not a `Cargo.toml` dependency) and
    fail — instead of silently skipping — when the tool is present but the
    invocation is wrong. `cargo_nextest_must_be_available_in_this_environment`
    pins the environmental prerequisite. The graceful "no backend available"
    skip (exec-03b) is preserved and simulated by restricting `PATH`.
  - `cargo_test` parses stdout regardless of exit status (nextest exits non-zero on
    test failures but still emits JSON); empty stdout → typed error.
  - `tools/list` advertises `cargo_test` with schema
    `{repo_path(req), crate?, filter?}`.

### Changed
- **`exec::runner` — sync timeout on `cargo check`** (`work/exec-01b-cargo-check-review-fixes`).
  - `Command::output()` replaced by `spawn` + `wait_with_timeout` so a stuck `cargo`
    cannot hang the server. Default timeout 120s, overridable per call via
    `CargoCheckArgs.timeout`; on expiry the process is killed and a typed error is
    returned (no panic, no hang).
- **`exec::parser` — diagnostic field serialization policy**.
  - `code`, `file` and `line` are now ALWAYS emitted in `Diagnostic`, with `null`
    when absent, for a stable, predictable contract toward agents. Schema
    `{ok, diagnostics:[{level,code,file,line,message}], summary}` unchanged.
- **`handler` / README** — `tools/list` description and README document the
  null-fields policy and the default 120s timeout.
  - Standalone Cargo crate `nxm-mcp-exec` (lib + bin), no parent workspace.
  - `exec::parser` — converts the `cargo --message-format=json` stream into a
    typed `CheckOutcome { ok, diagnostics:[{level,code,file,line,message}], summary }`.
    Defensive deserialization: unknown fields ignored, non-JSON/blank lines
    skipped, only `error`/`warning` levels kept, `ok == (errors == 0)`.
  - `exec::runner` — executes `cargo check --message-format=json` in a target
    `repo_path` under a one-command allow-list, read-only on the source; no
    arbitrary shell.
  - `protocol` + `handler` + `main` — hand-rolled JSON-RPC 2.0 over stdio
    (`initialize`, `tools/list`, `tools/call → cargo_check`), mirroring the
    sibling servers `nxm-mcp-webtomd` / `nxm-mcp-session`.
  - Tests in `tests/` (never inline in `src/`): parser fixtures
    (error → `ok:false`, warning-only → `ok:true`, mixed, noise, blank/garbage,
    empty, note/help skipped) and handler contract (tools/list advertises
    `cargo_check`, notifications ignored, unknown method, missing/invalid
    `repo_path` → tool error).

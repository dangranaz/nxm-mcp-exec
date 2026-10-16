# Changelog

All notable changes to `nxm-mcp-exec` are documented here. Format loosely based
on [Keep a Changelog](https://keepachangelog.com/); versions follow SemVer.

## [Unreleased]

### Added
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

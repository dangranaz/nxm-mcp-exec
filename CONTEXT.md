# CONTEXT — nxm-mcp-exec

Fast orientation for a coding agent touching this repo.

## What this is

An MCP server that turns Rust toolchain runs into **structured diagnostics** for
agents. The cardinal value is the typed verdict
(`{ok, diagnostics, summary}`), never raw log text.

Scope grows micro-task by micro-task:
`cargo_check` (this one) → `cargo_build` → `cargo_test` → `cargo_clippy` +
`cargo_fmt_check` → `run_gate` (GO/NO-GO, short-circuit at first failure).

## Non-negotiable decisions

- **Structured diagnostics only.** Parse `cargo --message-format=json`, never
  the human-rendered text (fragile: ANSI, truncation, format drift).
- **Allow-list, no arbitrary shell.** Only fixed cargo subcommands run, in the
  caller's `repo_path`, read-only on the source. Exposing a generic shell would
  recreate the raw-shell risk the allow-list exists to prevent.
- **Independent git repo.** `servers-mcp/` is a container, not a repo. This repo
  has its own `.git`; do not add it to any parent Cargo workspace
  (`servers-mcp/RULES.md`).
- **Hand-rolled JSON-RPC over stdio**, like `nxm-mcp-webtomd` / `nxm-mcp-session`.
  HTTP stateless is a later task.
- **Sync-first.** `cargo check` is fast; run it synchronously. An async
  start→poll→result model is only for later, long-running commands.

## Layout

- `src/exec/parser.rs` — JSON stream → `CheckOutcome` (the core, test-first).
- `src/exec/runner.rs` — spawn `cargo check` under the allow-list.
- `src/protocol.rs` — minimal JSON-RPC 2.0 types.
- `src/handler.rs` — `initialize` / `tools/list` / `tools/call` + tool contract.
- `src/main.rs` — stdio read/dispatch/write loop.
- `tests/` — parser + handler tests (RULES: tests never inline in `src/`).

## Standards (inherited)

`servers-mcp/RULES.md` + `nxm-docs/RULES.md`: files ≤300 lines, tests only in
`tests/`, no `unwrap`/`expect` in production (`anyhow::Result` + `?`/`.context`),
`tracing` logging, `///` docs with ITA/ENG on public API, keep `CHANGELOG.md`
updated, Conventional Commits, work on `work/<nn>-<slug>` branches.

## Composition (toolbelt)

`run_gate` will produce correctness GO/NO-GO verdicts that compose with
`nxm-decision` (`noul "mergeable?"`) → `nxm-git` (safe merge) → `nxm-session`
(handoff). `run_gate` may later call a bench server as one step.

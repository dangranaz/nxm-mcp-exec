# nxm-mcp-exec

A small MCP server that runs **allow-listed** Rust toolchain commands in a
target repository and returns **structured, typed diagnostics** — not raw log
text. A coding agent receives
`{ok:false, diagnostics:[{level,code,file,line,message}], summary}` instead of
hundreds of lines of human-formatted output: less context burned, zero
log-parsing bugs.

Un piccolo server MCP che esegue comandi della toolchain Rust **in allow-list**
nella directory di un repo target e ritorna **diagnostica strutturata e
tipata** invece del log grezzo.

> This is **not a CI**. It is a diagnostics surface for agents.

## Status

Tracer-bullet: two working tools, **`cargo_check`** and **`cargo_build`**, over
**stdio** JSON-RPC. Build/test/clippy/fmt and a GO/NO-GO `run_gate` are later
micro-tasks. HTTP transport is also a later step.

## Tool: `cargo_check`

Runs `cargo check --message-format=json` in `repo_path` and parses the JSON
stream into typed diagnostics.

```jsonc
cargo_check {
  "repo_path": "string",   // REQUIRED: target repo dir (must contain Cargo.toml)
  "crate":     "string",   // optional: -p <crate>
  "workspace": true         // optional: --workspace
}
// →
{
  "ok": false,              // true iff there are no error-level diagnostics
  "diagnostics": [
    { "level": "error", "code": "E0432", "file": "src/main.rs", "line": 12, "message": "unresolved import `foo`" },
    { "level": "warning", "code": null, "file": null, "line": null, "message": "unused variable `x`" }
  ],
  "summary": { "errors": 1, "warnings": 1 }
}
```

`cargo check` is synchronous with a configurable timeout (default 120s). A stuck
`cargo` process is killed and reported as a typed error — it never hangs the server.

Warnings alone keep `ok: true`.

## Tool: `cargo_build`

Runs `cargo build --message-format=json` in `repo_path` and parses the JSON
stream into the same typed diagnostics as `cargo_check`.

```jsonc
cargo_build {
  "repo_path": "string",   // REQUIRED: target repo dir (must contain Cargo.toml)
  "release":  false,        // optional: --release
  "features": []            // optional: --features <csv>
}
// →
{
  "ok": false,
  "diagnostics": [ ... ],
  "summary": { "errors": 1, "warnings": 0 }
}
```

`cargo build` is synchronous with the same configurable timeout (default 120s).
`release` is passed as `--release`; `features` as `--features <csv>`. Each
argument is a single argv element, never string-interpolated.

Build writes only to `target/` (expected and inside the repo), so it is
read-only in spirit.

## Security

- Only the allow-listed program `cargo` with subcommands `check` or `build` is
  ever spawned. There is **no arbitrary shell**: arguments are built from typed
  structs.
- The command runs in the caller-provided `repo_path`. `cargo check` is
  read-only on the source; `cargo build` writes only to `target/`.

## Transport

JSON-RPC 2.0 over **stdio** (one JSON object per line). Methods: `initialize`,
`tools/list`, `tools/call`.

## Build & test

```bash
cargo build
cargo test
```

## License

Licensed under either of Apache-2.0 or MIT at your option.

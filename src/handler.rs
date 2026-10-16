//! MCP request handling: `initialize`, `tools/list`, `tools/call`.
//!
//! Gestione delle richieste MCP: `initialize`, `tools/list`, `tools/call`.
//!
//! The tool *interface* (names, input schema) lives here and is the public
//! contract of this server. The actual work is delegated to [`crate::exec`].
//!
//! L'*interfaccia* dei tool (nomi, schema di input) vive qui ed è il contratto
//! pubblico di questo server. Il lavoro effettivo è delegato a [`crate::exec`].

use serde_json::{json, Value};

use crate::exec::runner::{cargo_build, cargo_check, CargoBuildArgs, CargoCheckArgs};
use crate::protocol::{codes, JsonRpcRequest, JsonRpcResponse};

/// MCP protocol version this server implements.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Dispatch a single JSON-RPC request to the right handler.
///
/// Smista una singola richiesta JSON-RPC verso il gestore corretto.
///
/// Returns `None` for notifications (requests without an `id`), which must not
/// produce a response per JSON-RPC.
pub fn dispatch(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    // Notifications carry no id and expect no reply (e.g. `notifications/initialized`);
    // the `?` returns `None` for them so they are never answered.
    let id = Some(req.id.as_ref()?.clone());

    let resp = match req.method.as_str() {
        "initialize" => JsonRpcResponse::ok(id, initialize_result()),
        "tools/list" => JsonRpcResponse::ok(id, tools_list_result()),
        "tools/call" => handle_tools_call(id, &req.params),
        other => JsonRpcResponse::error(
            id,
            codes::METHOD_NOT_FOUND,
            format!("method not found: {other}"),
        ),
    };
    Some(resp)
}

/// Build the `initialize` result advertising our capabilities.
fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "nxm-mcp-exec",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

/// The public tool contract. This is exactly what an agent sees via `tools/list`.
///
/// Il contratto pubblico dei tool. È esattamente ciò che un agent vede via
/// `tools/list`.
pub fn tools_list_result() -> Value {
    json!({
        "tools": [
            {
                "name": "cargo_check",
                "description": "Run `cargo check --message-format=json` in a target Rust repo and return \
    structured, typed diagnostics instead of raw log text: {ok, diagnostics:[{level,code,file,line,message}], summary}. \
    `code`, `file` and `line` are always present (`null` when absent) for a stable contract. \
    The command is sync with a configurable timeout (default 120s); a stuck `cargo` is killed \
    and reported as a typed error. Read-only on the source; only the allow-listed `cargo check` \
    command is ever executed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": {
                            "type": "string",
                            "description": "Target repo directory (MUST exist and contain a Cargo.toml)."
                        },
                        "crate": {
                            "type": "string",
                            "description": "Optional package selector, passed as `-p <crate>`."
                        },
                        "workspace": {
                            "type": "boolean",
                            "description": "Optional: check the whole workspace (`--workspace`)."
                        }
                    },
                    "required": ["repo_path"]
                }
            },
            {
                "name": "cargo_build",
                "description": "Run `cargo build --message-format=json` in a target Rust repo and return \
    structured, typed diagnostics instead of raw log text: {ok, diagnostics:[{level,code,file,line,message}], summary}. \
    `code`, `file` and `line` are always present (`null` when absent) for a stable contract. \
    `release` passes `--release`; `features` passes `--features <csv>`. Read-only in spirit \
    (build writes only to `target/`); only the allow-listed `cargo build` command is ever executed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": {
                            "type": "string",
                            "description": "Target repo directory (MUST exist and contain a Cargo.toml)."
                        },
                        "release": {
                            "type": "boolean",
                            "description": "Optional: build in release mode (`--release`)."
                        },
                        "features": {
                            "type": "array",
                            "description": "Optional: features to enable, passed as `--features <csv>`.",
                            "items": { "type": "string" }
                        }
                    },
                    "required": ["repo_path"]
                }
            }
        ]
    })
}

/// Handle `tools/call`: validate the tool name and arguments, then run it.
fn handle_tools_call(id: Option<Value>, params: &Value) -> JsonRpcResponse {
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n,
        None => {
            return JsonRpcResponse::error(id, codes::INVALID_PARAMS, "missing tool 'name'");
        }
    };
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let result = match name {
        "cargo_check" => tool_cargo_check(&args),
        "cargo_build" => tool_cargo_build(&args),
        other => Err(format!("unknown tool: {other}")),
    };

    match result {
        Ok(outcome) => JsonRpcResponse::ok(id, tool_json_result(&outcome)),
        Err(msg) => JsonRpcResponse::ok(id, tool_error_result(&msg)),
    }
}

/// Run the `cargo_check` tool and return its typed outcome as JSON.
///
/// Esegue il tool `cargo_check` e ritorna l'esito tipato come JSON.
fn tool_cargo_check(args: &Value) -> Result<Value, String> {
    let repo_path = require_str(args, "repo_path")?;
    let crate_name = args.get("crate").and_then(Value::as_str).map(String::from);
    let workspace = args
        .get("workspace")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let check_args = CargoCheckArgs {
        repo_path: repo_path.into(),
        crate_name,
        workspace,
        timeout: None, // default 120s; per-call override is a later micro-task
    };

    let outcome = cargo_check(&check_args).map_err(|e| format!("{e:#}"))?;
    serde_json::to_value(&outcome).map_err(|e| format!("failed to serialize outcome: {e}"))
}

/// Run the `cargo_build` tool and return its typed outcome as JSON.
///
/// Esegue il tool `cargo_build` e ritorna l'esito tipato come JSON.
fn tool_cargo_build(args: &Value) -> Result<Value, String> {
    let repo_path = require_str(args, "repo_path")?;
    let release = args
        .get("release")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let features = args
        .get("features")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let build_args = CargoBuildArgs {
        repo_path: repo_path.into(),
        release,
        features,
        timeout: None, // default 120s; per-call override is a later micro-task
    };

    let outcome = cargo_build(&build_args).map_err(|e| format!("{e:#}"))?;
    serde_json::to_value(&outcome).map_err(|e| format!("failed to serialize outcome: {e}"))
}

/// Require a string argument, returning a readable error when absent.
fn require_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing required argument '{key}'"))
}

/// MCP `tools/call` success payload: structured JSON in a text content block.
///
/// The outcome is serialized as pretty JSON so a human reading the transcript
/// can see it, while remaining valid for programmatic re-parsing.
fn tool_json_result(outcome: &Value) -> Value {
    let text = serde_json::to_string_pretty(outcome).unwrap_or_else(|_| outcome.to_string());
    json!({
        "content": [ { "type": "text", "text": text } ],
        "structuredContent": outcome,
        "isError": false
    })
}

/// MCP `tools/call` error payload (isError = true, still a valid result).
fn tool_error_result(message: &str) -> Value {
    json!({
        "content": [ { "type": "text", "text": message } ],
        "isError": true
    })
}

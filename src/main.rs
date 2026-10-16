//! nxm-mcp-exec binary entry point.
//!
//! Transport: JSON-RPC 2.0 over stdio (one JSON object per line), like the
//! sibling servers `nxm-mcp-webtomd` / `nxm-mcp-session`. HTTP transport is a
//! later micro-task; this binary only speaks stdio.
//!
//! Trasporto: JSON-RPC 2.0 su stdio (un oggetto JSON per riga), come i fratelli
//! `nxm-mcp-webtomd` / `nxm-mcp-session`. Il trasporto HTTP è un micro-task
//! successivo; questo binario parla solo stdio.

use std::io::{self, BufRead, Write};

use nxm_mcp_exec::handler;
use nxm_mcp_exec::protocol::{codes, JsonRpcRequest, JsonRpcResponse};

fn main() {
    // Logging to stderr (file logging via nxm-shared is a later alignment step).
    // Non-fatal if the subscriber is already set.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nxm_mcp_exec=info")),
        )
        .with_writer(io::stderr)
        .try_init();

    run_stdio();
}

/// Read JSON-RPC requests line by line from stdin and write responses to stdout.
///
/// Legge le richieste JSON-RPC riga per riga da stdin e scrive le risposte su
/// stdout.
fn run_stdio() {
    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_response(
                    &stdout,
                    &JsonRpcResponse::error(None, codes::PARSE_ERROR, format!("parse error: {e}")),
                );
                continue;
            }
        };

        // Notifications return None and must not be answered.
        if let Some(response) = handler::dispatch(&request) {
            write_response(&stdout, &response);
        }
    }
}

/// Serialize and write a single response as one line on stdout.
fn write_response(stdout: &io::Stdout, response: &JsonRpcResponse) {
    match serde_json::to_string(response) {
        Ok(s) => {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{s}");
            let _ = out.flush();
        }
        Err(e) => eprintln!("failed to serialize response: {e}"),
    }
}

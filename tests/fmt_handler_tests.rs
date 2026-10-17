//! Tests for the `cargo_fmt_check` tool contract and semantics (MCP surface).
//! TDD: red → green, one slice at a time.
//!
//! Test per il contratto e la semantica del tool `cargo_fmt_check`
//! (superficie MCP). TDD: rosso → verde, una slice alla volta.
//!
//! Coverage / Copertura:
//! - Parser puro `fmt output → {ok, offending_files}` su fixture di output.
//! - `tools/list` espone `cargo_fmt_check` `{repo_path(req)}`.
//! - Call reale su repo formattato → `ok=true`.
//! - Call reale su repo non formattato → `ok=false` + file elencato.
//! - Read-only: dopo la call il file mal-formattato NON è modificato.

use nxm_mcp_exec::exec::fmt_runner::parse_fmt_output;
use nxm_mcp_exec::handler::{dispatch, tools_list_result};
use nxm_mcp_exec::protocol::JsonRpcRequest;
use serde_json::json;
use tempfile::TempDir;

fn request(
    id: Option<serde_json::Value>,
    method: &str,
    params: serde_json::Value,
) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: Some("2.0".into()),
        id,
        method: method.into(),
        params,
    }
}

fn write_cargo_repo(dir: &TempDir, main_src: &str) {
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    std::fs::write(dir.path().join("src").join("main.rs"), main_src).expect("write main.rs");
}

fn cargo_fmt_check_request(repo_path: &str) -> JsonRpcRequest {
    request(
        Some(json!(7)),
        "tools/call",
        json!({ "name": "cargo_fmt_check", "arguments": { "repo_path": repo_path } }),
    )
}

// --- Parser puro: fmt output → {ok, offending_files} (test-first) ---

#[test]
fn fmt_parser_clean_output_is_ok() {
    let out = parse_fmt_output(true, "");
    assert!(out.ok);
    assert!(out.offending_files.is_empty());
}

#[test]
fn fmt_parser_two_offending_files() {
    let out = parse_fmt_output(false, "a.rs\nb.rs\n");
    assert!(!out.ok);
    assert_eq!(
        out.offending_files,
        vec!["a.rs".to_string(), "b.rs".to_string()]
    );
}

// --- tools/list espone cargo_fmt_check ---

#[test]
fn tools_list_advertises_cargo_fmt_check() {
    let binding = tools_list_result();
    let tools = binding["tools"].as_array().expect("tools is an array");
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    assert!(
        names.contains(&"cargo_fmt_check"),
        "missing cargo_fmt_check in tools/list: {names:?}"
    );
    let tool = tools
        .iter()
        .find(|t| t["name"] == "cargo_fmt_check")
        .expect("cargo_fmt_check tool entry");
    let props = tool["inputSchema"]["properties"]
        .as_object()
        .expect("inputSchema.properties is an object");
    assert!(props.contains_key("repo_path"));
    let required = tool["inputSchema"]["required"]
        .as_array()
        .expect("inputSchema.required is an array");
    assert!(
        required.iter().any(|v| v == "repo_path"),
        "repo_path must be required"
    );
}

// --- Call reale su repo formattato → ok=true ---

#[test]
fn cargo_fmt_check_formatted_repo_ok_true() {
    let dir = TempDir::new().expect("temp dir");
    write_cargo_repo(&dir, "fn main() {}\n");

    let req = cargo_fmt_check_request(dir.path().to_str().expect("path is utf8"));
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["ok"], json!(true));
    assert!(
        sc["offending_files"]
            .as_array()
            .expect("offending_files is an array")
            .is_empty(),
        "offending_files must be empty for a formatted repo"
    );
}

// --- Call reale su repo non formattato → ok=false + file elencato ---

#[test]
fn cargo_fmt_check_unformatted_repo_ok_false() {
    let dir = TempDir::new().expect("temp dir");
    write_cargo_repo(&dir, "fn main(){\n   \n}\n");
    let main_rs = dir.path().join("src").join("main.rs");
    let before = std::fs::read(&main_rs).expect("read fixture before call");

    let req = cargo_fmt_check_request(dir.path().to_str().expect("path is utf8"));
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["ok"], json!(false));
    let files: Vec<String> = sc["offending_files"]
        .as_array()
        .expect("offending_files is an array")
        .iter()
        .map(|v| v.as_str().expect("offending file is a string").to_string())
        .collect();
    assert!(!files.is_empty(), "expected at least one offending file");
    assert!(
        files.iter().any(|f| f.ends_with("src/main.rs")),
        "offending files must include src/main.rs, got {files:?}"
    );

    // Read-only: il file mal-formattato non deve essere modificato da --check.
    let after = std::fs::read(&main_rs).expect("read fixture after call");
    assert_eq!(before, after, "cargo fmt --check must be read-only");
}

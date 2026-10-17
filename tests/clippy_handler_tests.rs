//! Tests for the `cargo_clippy` tool contract and semantics (MCP surface).
//!
//! Test per il contratto e la semantica del tool `cargo_clippy` (superficie MCP).
//!
//! Coverage / Copertura:
//! - `tools/list` advertises `cargo_clippy` with schema `{repo_path(req), deny_warnings?}`.
//! - Integration on a real fixture repo: a clippy lint yields structured
//!   diagnostics carrying `file`/`line`/`code` (the clippy lint name), reusing
//!   the shared compiler-message parser.
//! - `deny_warnings` semantics (central requirement): on a warnings-only
//!   fixture, `ok` flips from `true` to `false` while diagnostics keep their
//!   true `warning` level.
//! - Clean repo: `ok == true`, `diagnostics == []`.
//!
//! - `tools/list` espone `cargo_clippy` con schema `{repo_path(req), deny_warnings?}`.
//! - Integrazione su fixture reale: una lint clippy produce diagnostica
//!   strutturata con `file`/`line`/`code` (nome della lint), riusando il parser
//!   condiviso dei compiler-message.
//! - Semantica `deny_warnings` (requisito centrale): su una fixture con soli
//!   warning, `ok` passa da `true` a `false` mentre le diagnostiche mantengono
//!   il loro vero livello `warning`.
//! - Repo pulita: `ok == true`, `diagnostics == []`.

use nxm_mcp_exec::handler::{dispatch, tools_list_result};
use nxm_mcp_exec::protocol::JsonRpcRequest;
use serde_json::{json, Value};
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

fn cargo_clippy_request(repo_path: &str, deny_warnings: Option<bool>) -> JsonRpcRequest {
    let mut arguments = json!({ "repo_path": repo_path });
    if let Some(deny) = deny_warnings {
        arguments["deny_warnings"] = json!(deny);
    }
    request(Some(json!(3)), "tools/call", json!({ "name": "cargo_clippy", "arguments": arguments }))
}

fn write_clippy_lint_repo(dir: &TempDir) {
    // `assert!(true)` triggers clippy::assertions_on_constants (warning-level,
    // no rustc warning, no compile error): a warnings-only fixture.
    write_cargo_repo(&dir, "fn main() {\n    assert!(true);\n}\n");
}

#[test]
fn cargo_clippy_lint_produces_structured_diagnostics() {
    let dir = TempDir::new().expect("temp dir");
    write_clippy_lint_repo(&dir);

    let req = cargo_clippy_request(dir.path().to_str().expect("path is utf8"), None);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    // Warnings-only fixture and deny_warnings defaults to false: ok stays true.
    assert_eq!(sc["ok"], json!(true));
    let diagnostics = sc["diagnostics"]
        .as_array()
        .expect("diagnostics is an array");
    assert!(!diagnostics.is_empty(), "expected at least one clippy lint");
    let d = &diagnostics[0];
    assert_eq!(d["level"], "warning");
    let code = d["code"].as_str().expect("lint code is present");
    assert!(
        code.contains("clippy::"),
        "code must carry the clippy lint name, got: {code}"
    );
    assert!(d["file"].as_str().is_some(), "diagnostic must have a file");
    assert!(d["line"].is_number(), "diagnostic must have a line");
}

#[test]
fn cargo_clippy_deny_warnings_treats_warnings_as_errors() {
    let dir = TempDir::new().expect("temp dir");
    write_clippy_lint_repo(&dir);

    let req = cargo_clippy_request(dir.path().to_str().expect("path is utf8"), Some(true));
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    // Central requirement: same warnings-only fixture must flip ok to false.
    assert_eq!(sc["ok"], json!(false));
    // Diagnostics keep the TRUE warning level (enforcement is on the summary).
    assert_eq!(sc["diagnostics"][0]["level"], "warning");
}

#[test]
fn cargo_clippy_clean_repo_is_ok_with_no_diagnostics() {
    let dir = TempDir::new().expect("temp dir");
    write_cargo_repo(&dir, "fn main() {}\n");

    let req = cargo_clippy_request(dir.path().to_str().expect("path is utf8"), Some(true));
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["ok"], json!(true));
    assert_eq!(
        sc["diagnostics"]
            .as_array()
            .expect("diagnostics is an array")
            .len(),
        0
    );
}

#[test]
fn tools_list_advertises_cargo_clippy() {
    let v = tools_list_result();
    let names: Vec<&str> = v["tools"]
        .as_array()
        .expect("tools is an array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"cargo_clippy"));

    let clippy = v["tools"]
        .as_array()
        .expect("tools is an array")
        .iter()
        .find(|t| t["name"] == "cargo_clippy")
        .expect("cargo_clippy tool is advertised");
    let props = clippy["inputSchema"]["properties"]
        .as_object()
        .expect("inputSchema properties is an object");
    assert!(props.contains_key("repo_path"));
    assert!(props.contains_key("deny_warnings"));
    let required = clippy["inputSchema"]["required"]
        .as_array()
        .expect("required is an array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<&str>>();
    assert!(required.contains(&"repo_path"));
}

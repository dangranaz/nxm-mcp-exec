//! Tests for the MCP handler dispatch and tool contract.
//!
//! Test per il dispatch del handler MCP e per il contratto dei tool.

use nxm_mcp_exec::handler::{dispatch, tools_list_result};
use nxm_mcp_exec::protocol::{codes, JsonRpcRequest};
use serde_json::json;

fn request(id: Option<serde_json::Value>, method: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: Some("2.0".into()),
        id,
        method: method.into(),
        params,
    }
}

#[test]
fn tools_list_advertises_cargo_check() {
    let v = tools_list_result();
    let names: Vec<&str> = v["tools"]
        .as_array()
        .expect("tools is an array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"cargo_check"));
}

#[test]
fn notification_produces_no_response() {
    let req = request(None, "notifications/initialized", json!({}));
    assert!(dispatch(&req).is_none());
}

#[test]
fn unknown_method_is_method_not_found() {
    let req = request(Some(json!(1)), "does/not/exist", json!({}));
    let resp = dispatch(&req).expect("request gets a response");
    let err = resp.error.expect("error response");
    assert_eq!(err.code, codes::METHOD_NOT_FOUND);
}

#[test]
fn initialize_reports_server_name() {
    let req = request(Some(json!(1)), "initialize", json!({}));
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("result");
    assert_eq!(result["serverInfo"]["name"], json!("nxm-mcp-exec"));
}

#[test]
fn cargo_check_missing_repo_path_is_tool_error() {
    let req = request(
        Some(json!(1)),
        "tools/call",
        json!({ "name": "cargo_check", "arguments": {} }),
    );
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(true));
}

#[test]
fn cargo_check_nonexistent_repo_is_tool_error() {
    let req = request(
        Some(json!(1)),
        "tools/call",
        json!({ "name": "cargo_check", "arguments": { "repo_path": "/no/such/dir/xyzzy" } }),
    );
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(true));
}

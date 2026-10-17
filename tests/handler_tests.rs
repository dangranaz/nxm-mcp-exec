//! Tests for the MCP handler dispatch and tool contract.
//!
//! Test per il dispatch del handler MCP e per il contratto dei tool.

use nxm_mcp_exec::exec::runner::{build_args, is_allowed_subcommand, run_cargo, DEFAULT_TIMEOUT};
use nxm_mcp_exec::handler::{dispatch, tools_list_result};
use nxm_mcp_exec::protocol::{codes, JsonRpcRequest};
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
fn tools_list_describes_null_fields_policy_and_timeout() {
    let v = tools_list_result();
    let desc = v["tools"][0]["description"]
        .as_str()
        .expect("description is a string");
    assert!(
        desc.contains("null"),
        "description must document the null-fields policy: {desc}"
    );
    assert!(
        desc.contains("120s"),
        "description must document the default timeout: {desc}"
    );
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

#[test]
fn tools_list_advertises_cargo_build() {
    let v = tools_list_result();
    let names: Vec<&str> = v["tools"]
        .as_array()
        .expect("tools is an array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"cargo_build"));

    let build = v["tools"]
        .as_array()
        .expect("tools is an array")
        .iter()
        .find(|t| t["name"] == "cargo_build")
        .expect("cargo_build tool is advertised");
    let props = build["inputSchema"]["properties"]
        .as_object()
        .expect("inputSchema properties is an object");
    assert!(props.contains_key("repo_path"));
    assert!(props.contains_key("release"));
    assert!(props.contains_key("features"));
    let required = build["inputSchema"]["required"]
        .as_array()
        .expect("required is an array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<&str>>();
    assert!(required.contains(&"repo_path"));
}

fn cargo_build_request(repo_path: &str, release: bool, features: Vec<&str>) -> JsonRpcRequest {
    request(
        Some(json!(2)),
        "tools/call",
        json!({
            "name": "cargo_build",
            "arguments": {
                "repo_path": repo_path,
                "release": release,
                "features": features
            }
        }),
    )
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

#[test]
fn cargo_build_clean_repo_is_ok() {
    let dir = TempDir::new().expect("temp dir");
    write_cargo_repo(&dir, "fn main() {}\n");

    let req = cargo_build_request(dir.path().to_str().expect("path is utf8"), false, vec![]);
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
fn cargo_build_broken_repo_is_error_with_diagnostics() {
    let dir = TempDir::new().expect("temp dir");
    write_cargo_repo(&dir, "fn main() {\n    let x: i32 = \"not a number\";\n}\n");

    let req = cargo_build_request(dir.path().to_str().expect("path is utf8"), true, vec![]);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["ok"], json!(false));
    let diagnostics = sc["diagnostics"].as_array().expect("diagnostics is an array");
    assert!(!diagnostics.is_empty(), "expected at least one diagnostic");
    let d = &diagnostics[0];
    assert_eq!(d["level"], "error");
    assert!(d["file"].as_str().is_some(), "diagnostic must have a file");
    assert!(d["line"].is_number(), "diagnostic must have a line");
}

#[test]
fn build_args_translates_release_and_features() {
    assert_eq!(
        build_args(true, &["a".to_string(), "b".to_string()]),
        vec![
            "--release".to_string(),
            "--features".to_string(),
            "a,b".to_string()
        ]
    );
    assert_eq!(build_args(false, &[]), Vec::<String>::new());
    assert_eq!(
        build_args(false, &["one".to_string()]),
        vec!["--features".to_string(), "one".to_string()]
    );
}

#[test]
fn run_cargo_rejects_non_allowlisted_subcommand() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let result = run_cargo("run", &[], dir.path(), DEFAULT_TIMEOUT);
    assert!(
        result.is_err(),
        "non-allow-listed subcommand must be rejected"
    );
    assert!(
        result.unwrap_err().to_string().contains("not allow-listed"),
        "error must mention the allow-list"
    );
    assert!(is_allowed_subcommand("check"));
    assert!(is_allowed_subcommand("build"));
    assert!(!is_allowed_subcommand("run"));
    assert!(!is_allowed_subcommand("clippy"));
}

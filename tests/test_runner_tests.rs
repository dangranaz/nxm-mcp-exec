//! Tests for `cargo_test` tool wiring and `exec::test_runner`.
//!
//! Test per il cablaggio del tool `cargo_test` e per `exec::test_runner`.
//!
//! I test di integrazione green/red/filter via nextest skipparono esplicitamente
//! (con messaggio chiaro) quando `cargo-nextest` non è installato, invece di
//! essere marcati `#[ignore]` incondizionato.

use std::env;

use nxm_mcp_exec::exec::test_runner::cargo_nextest_available;
use nxm_mcp_exec::handler::{dispatch, tools_list_result};
use nxm_mcp_exec::protocol::JsonRpcRequest;
use serde_json::{json, Value};
use tempfile::TempDir;

/// Writes a minimal cargo repo with a lib containing the given test code.
///
/// Scrive un repo cargo minimo con una lib contenente il codice di test dato.
fn write_test_repo(dir: &TempDir, lib_src: &str) {
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    std::fs::write(dir.path().join("src").join("lib.rs"), lib_src).expect("write lib.rs");
}

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

fn cargo_test_request(
    repo_path: &str,
    crate_name: Option<&str>,
    filter: Option<&str>,
) -> JsonRpcRequest {
    let mut args = serde_json::json!({ "repo_path": repo_path });
    if let Some(c) = crate_name {
        args.as_object_mut()
            .expect("args is object")
            .insert("crate".to_string(), json!(c));
    }
    if let Some(f) = filter {
        args.as_object_mut()
            .expect("args is object")
            .insert("filter".to_string(), json!(f));
    }
    request(
        Some(json!(3)),
        "tools/call",
        json!({ "name": "cargo_test", "arguments": args }),
    )
}

#[test]
fn tools_list_advertises_cargo_test() {
    let v = tools_list_result();
    let names: Vec<&str> = v["tools"]
        .as_array()
        .expect("tools is an array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"cargo_test"));

    let test_tool = v["tools"]
        .as_array()
        .expect("tools is an array")
        .iter()
        .find(|t| t["name"] == "cargo_test")
        .expect("cargo_test tool is advertised");
    let props = test_tool["inputSchema"]["properties"]
        .as_object()
        .expect("inputSchema properties is an object");
    assert!(props.contains_key("repo_path"));
    assert!(props.contains_key("crate"));
    assert!(props.contains_key("filter"));
    let required = test_tool["inputSchema"]["required"]
        .as_array()
        .expect("required is an array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<&str>>();
    assert!(required.contains(&"repo_path"));
}

#[test]
fn cargo_test_graceful_skip_when_no_backend() {
    // Quando nessun backend (nextest né nightly JSON) è disponibile, cargo_test
    // deve restituire un errore tipato e leggibile invece di panificare, appendere
    // o parsare testo umano.
    env::remove_var("NXM_CARGO");

    let dir = TempDir::new().expect("temp dir");
    write_test_repo(&dir, "#[test]\nfn it_works() { assert_eq!(2 + 2, 4); }\n");

    let req = cargo_test_request(dir.path().to_str().expect("path is utf8"), None, None);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(true));
    let msg = result["content"][0]["text"]
        .as_str()
        .expect("content text is a string");
    assert!(
        msg.contains("backend") || msg.contains("nextest") || msg.contains("nightly"),
        "error message must mention the missing backend: {msg}"
    );
}

#[test]
fn cargo_test_graceful_skip_when_nextest_absent() {
    // Se nextest è assente, il test di integrazione deve skipparsi esplicitamente
    // con un messaggio chiaro, NON fallire e NON essere #[ignore] incondizionato.
    if cargo_nextest_available() {
        eprintln!("skip: cargo-nextest is available; integration test would run");
        return;
    }
    eprintln!("skip: cargo-nextest not available; nextest integration test explicitly skipped");
}

#[test]
fn nextest_output_mapping_to_test_outcome() {
    // Mappatura output nextest machine-readable → TestOutcome (fixture statica,
    // non richiede nextest installato).
    use nxm_mcp_exec::exec::test_parser::parse_nextest_stream;

    let stream = r#"{"type":"test","event":"passed","name":"test_foo","kind":"test","duration_ms":10}
{"type":"test","event":"failed","name":"test_bar","kind":"test","duration_ms":5,"message":"panicked at 'assertion failed', src/lib.rs:42"}
{"type":"test","event":"ignored","name":"test_baz","kind":"test","reason":"..."}
{"type":"run","event":"started","name":"demo","kind":"run"}
{"type":"not-json-at-all"}"#;

    let outcome = parse_nextest_stream(stream);
    assert_eq!(outcome.passed, 1);
    assert_eq!(outcome.failed, 1);
    assert_eq!(outcome.ignored, 1);
    assert_eq!(outcome.failures.len(), 1);
    assert_eq!(outcome.failures[0].name, "test_bar");
    assert_eq!(
        outcome.failures[0].message,
        "panicked at 'assertion failed', src/lib.rs:42"
    );
}

#[test]
fn cargo_test_green_repo_via_nextest() {
    // Integrazione green su stable via nextest. Skip esplicito se nextest
    // non è disponibile (non #[ignore] incondizionato).
    if !cargo_nextest_available() {
        eprintln!("skip: cargo-nextest not available; green integration test explicitly skipped");
        return;
    }

    env::set_var("NXM_CARGO", "cargo");

    let dir = TempDir::new().expect("temp dir");
    write_test_repo(
        &dir,
        "#[test]\nfn it_works() { assert_eq!(2 + 2, 4); }\n#[test]\nfn another_ok() { assert!(true); }\n",
    );

    let req = cargo_test_request(dir.path().to_str().expect("path is utf8"), None, None);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["failed"], json!(0));
    assert_eq!(sc["passed"], json!(2));
    assert_eq!(
        sc["failures"].as_array().expect("failures is array").len(),
        0
    );

    env::remove_var("NXM_CARGO");
}

#[test]
fn cargo_test_red_repo_via_nextest() {
    // Integrazione red su stable via nextest. Skip esplicito se nextest
    // non è disponibile.
    if !cargo_nextest_available() {
        eprintln!("skip: cargo-nextest not available; red integration test explicitly skipped");
        return;
    }

    env::set_var("NXM_CARGO", "cargo");

    let dir = TempDir::new().expect("temp dir");
    write_test_repo(
        &dir,
        "#[test]\nfn it_fails() { assert_eq!(1, 2); }\n#[test]\nfn it_works() { assert!(true); }\n",
    );

    let req = cargo_test_request(dir.path().to_str().expect("path is utf8"), None, None);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert!(sc["failed"].as_u64().expect("failed is a number") >= 1);
    let failures = sc["failures"].as_array().expect("failures is array");
    assert!(!failures.is_empty(), "expected at least one failure");
    let failure_names: Vec<String> = failures
        .iter()
        .filter_map(|f| f["name"].as_str().map(String::from))
        .collect();
    assert!(
        failure_names.iter().any(|n| n == "it_fails"),
        "failures must contain the failing test name: {failure_names:?}"
    );

    env::remove_var("NXM_CARGO");
}

#[test]
fn cargo_test_filter_applied_via_nextest() {
    // Filtro applicato via nextest. Skip esplicito se nextest non è disponibile.
    if !cargo_nextest_available() {
        eprintln!("skip: cargo-nextest not available; filter integration test explicitly skipped");
        return;
    }

    env::set_var("NXM_CARGO", "cargo");

    let dir = TempDir::new().expect("temp dir");
    write_test_repo(
        &dir,
        "#[test]\nfn alpha_test() { assert!(true); }\n#[test]\nfn beta_test() { assert!(true); }\n",
    );

    let req = cargo_test_request(
        dir.path().to_str().expect("path is utf8"),
        None,
        Some("alpha"),
    );
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["failed"], json!(0));
    // Con il filtro `alpha`, solo `alpha_test` dovrebbe eseguirsi e passare.
    assert_eq!(sc["passed"], json!(1));
    assert_eq!(
        sc["failures"].as_array().expect("failures is array").len(),
        0
    );

    env::remove_var("NXM_CARGO");
}

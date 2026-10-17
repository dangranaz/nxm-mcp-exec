//! Tests for the `run_gate` tool contract and semantics (MCP surface).
//! TDD: red → green, one slice at a time.
//!
//! Test per il contratto e la semantica del tool `run_gate` (superficie MCP).
//! TDD: rosso → verde, una slice alla volta.
//!
//! Coverage / Copertura:
//! - `tools/list` espone `run_gate` `{repo_path(req), gate_name?}`.
//! - GO su fixture pulita: `verdict:"GO"`, tutti gli step `ok:true`,
//!   `failed_step` assente, ordine = ordine dichiarato del gate di default.
//! - NO-GO + short-circuit al primo fallimento (requisito centrale): fixture
//!   con errore di compilazione → si ferma al primo step; fixture con step
//!   intermedio fallito (fmt) → gli step successivi NON risultano eseguiti.
//! - Gate sconosciuto → errore tipato (isError).

use nxm_mcp_exec::exec::gate_runner::{gate_steps, DEFAULT_GATE};
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

fn run_gate_request(repo_path: &str, gate_name: Option<&str>) -> JsonRpcRequest {
    let mut arguments = json!({ "repo_path": repo_path });
    if let Some(gate) = gate_name {
        arguments["gate_name"] = json!(gate);
    }
    request(
        Some(json!(9)),
        "tools/call",
        json!({ "name": "run_gate", "arguments": arguments }),
    )
}

fn step_names(sc: &Value) -> Vec<String> {
    sc["steps"]
        .as_array()
        .expect("steps is an array")
        .iter()
        .map(|s| {
            s["name"]
                .as_str()
                .expect("step name is a string")
                .to_string()
        })
        .collect()
}

// --- Definizione del gate (pura): default noto, nomi sconosciuti rifiutati ---

#[test]
fn gate_steps_default_is_defined_and_unknown_gate_is_none() {
    assert_eq!(gate_steps("default"), Some(DEFAULT_GATE));
    assert_eq!(gate_steps("bogus"), None);
    assert!(
        DEFAULT_GATE.contains(&"cargo_check"),
        "default gate must include cargo_check"
    );
}

// --- tools/list espone run_gate ---

#[test]
fn tools_list_advertises_run_gate() {
    let binding = tools_list_result();
    let tools = binding["tools"].as_array().expect("tools is an array");
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    assert!(
        names.contains(&"run_gate"),
        "missing run_gate in tools/list: {names:?}"
    );
    let tool = tools
        .iter()
        .find(|t| t["name"] == "run_gate")
        .expect("run_gate tool entry");
    let props = tool["inputSchema"]["properties"]
        .as_object()
        .expect("inputSchema.properties is an object");
    assert!(props.contains_key("repo_path"));
    assert!(props.contains_key("gate_name"));
    let required = tool["inputSchema"]["required"]
        .as_array()
        .expect("inputSchema.required is an array");
    assert!(
        required.iter().any(|v| v == "repo_path"),
        "repo_path must be required"
    );
}

// --- GO su fixture pulita, ordine = ordine dichiarato ---

#[test]
fn run_gate_clean_repo_is_go_in_declared_order() {
    let dir = TempDir::new().expect("temp dir");
    // Fixture pulita CON un test che passa: il gate esegue anche cargo_test
    // (nextest esce 4 = "no tests to run" su un repo senza test).
    write_cargo_repo(
        &dir,
        "fn main() {}\n\n#[test]\nfn sanity() {\n    assert_eq!(1 + 1, 2);\n}\n",
    );

    let req = run_gate_request(dir.path().to_str().expect("path is utf8"), None);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["verdict"], json!("GO"));
    assert!(
        sc["failed_step"].is_null(),
        "failed_step must be absent (null) on GO"
    );
    let steps = sc["steps"].as_array().expect("steps is an array");
    assert_eq!(
        step_names(sc),
        DEFAULT_GATE
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        "executed steps must match the declared default gate order"
    );
    for s in steps {
        assert_eq!(s["ok"], json!(true), "every step must be ok on GO: {s}");
    }
}

// --- NO-GO + short-circuit al primo fallimento (requisito centrale) ---

#[test]
fn run_gate_compile_error_short_circuits_at_first_step() {
    let dir = TempDir::new().expect("temp dir");
    write_cargo_repo(&dir, "fn main() {\n    let x: u32 = \"not a u32\";\n}\n");

    let req = run_gate_request(dir.path().to_str().expect("path is utf8"), None);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["verdict"], json!("NO-GO"));
    assert_eq!(sc["failed_step"], json!("cargo_check"));
    // Short-circuit: SOLO il primo step è stato eseguito. Se il gate non
    // fermasse la sequenza, gli step successivi comparirebbero qui.
    let names = step_names(sc);
    assert_eq!(
        names,
        vec!["cargo_check".to_string()],
        "only the first (failed) step must be executed: {names:?}"
    );
    let steps = sc["steps"].as_array().expect("steps is an array");
    assert_eq!(steps[0]["ok"], json!(false));
}

#[test]
fn run_gate_intermediate_failure_stops_before_later_steps() {
    // Compila, clippy-clean, ma NON formattato: il gate deve fermarsi a
    // cargo_fmt_check e NON eseguire cargo_test.
    let dir = TempDir::new().expect("temp dir");
    write_cargo_repo(&dir, "fn main(){\n   \n}\n");

    let req = run_gate_request(dir.path().to_str().expect("path is utf8"), None);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(false));
    let sc = &result["structuredContent"];
    assert_eq!(sc["verdict"], json!("NO-GO"));
    assert_eq!(sc["failed_step"], json!("cargo_fmt_check"));
    let names = step_names(sc);
    assert_eq!(
        names,
        vec![
            "cargo_check".to_string(),
            "cargo_clippy".to_string(),
            "cargo_fmt_check".to_string()
        ],
        "steps after the failed one must not be executed: {names:?}"
    );
    assert!(
        !names.contains(&"cargo_test".to_string()),
        "cargo_test must NOT run after a failed step (short-circuit)"
    );
}

// --- Gate sconosciuto → errore tipato ---

#[test]
fn run_gate_unknown_gate_name_is_typed_error() {
    let dir = TempDir::new().expect("temp dir");
    write_cargo_repo(&dir, "fn main() {}\n");

    let req = run_gate_request(dir.path().to_str().expect("path is utf8"), Some("bogus"));
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    assert_eq!(result["isError"], json!(true));
}

//! Tests for `cargo_test` tool wiring and graceful behavior in the handler.
//!
//! Test per il cablaggio del tool `cargo_test` e per il comportamento grazioso
//! nell'handler.
//!
//! I test d'integrazione green/red/filter via nextest ESEGUONO nextest reale
//! e vivono in `tests/nextest_tests.rs` (con il prerequisito ambientale
//! obbligatorio di cargo-nextest). Qui resta lo skip grazioso per il caso
//! "nessun backend disponibile", che preserva exec-03b.
//!
//! I test che mutano le variabili d'ambiente (NXM_CARGO, PATH) prendono il
//! lock condiviso: i test Rust girano in thread paralleli e l'env è
//! per-processo.

use std::env;
use std::sync::{Mutex, MutexGuard};

use nxm_mcp_exec::handler::{dispatch, tools_list_result};
use nxm_mcp_exec::protocol::JsonRpcRequest;
use serde_json::{json, Value};
use tempfile::TempDir;

/// Le variabili di ambiente sono per-processo: i test che le mutano (NXM_CARGO,
/// PATH) devono essere sequenziali rispetto ai loro pari. I test Rust girano in
/// thread paralleli, quindi ogni test che tocca l'env prende questo lock.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Hold the env mutex for the whole scope of a test touching env vars.
fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().expect("env lock available")
}

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
    // o parsare testo umano. Poiché nell'ambiente delegato cargo-nextest è
    // obbligatorio, la "assenza di backend" viene SIMULATA restringendo PATH
    // (nessun `cargo` spawna) e ripristinando l'ambiente a fine test.
    let _guard = env_guard();

    env::remove_var("NXM_CARGO");

    let old_path = env::var("PATH").ok();
    env::set_var("PATH", "/usr/bin:/bin");

    let dir = TempDir::new().expect("temp dir");
    write_test_repo(&dir, "#[test]\nfn it_works() { assert_eq!(2 + 2, 4); }\n");

    let req = cargo_test_request(dir.path().to_str().expect("path is utf8"), None, None);
    let resp = dispatch(&req).expect("response");
    let result = resp.result.expect("tool result");
    let err_text = result["content"][0]["text"]
        .as_str()
        .expect("content text is a string")
        .to_string();

    // Ripristino l'ambiente PRIMA delle asserzioni, per non inquinare i pari.
    match old_path {
        Some(p) => env::set_var("PATH", p),
        None => env::remove_var("PATH"),
    }
    drop(_guard);

    assert_eq!(result["isError"], json!(true));
    assert!(
        err_text.contains("backend")
            || err_text.contains("nextest")
            || err_text.contains("nightly"),
        "error message must mention the missing backend: {err_text}"
    );
}

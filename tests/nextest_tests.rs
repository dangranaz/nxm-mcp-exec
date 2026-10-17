//! Tests for the cargo-nextest backend of `cargo_test` (`exec::nextest_runner`).
//!
//! Test per il backend cargo-nextest di `cargo_test` (`exec::nextest_runner`).
//!
//! I test d'integrazione green/red/filter ESEGUONO nextest reale (divieto di
//! skip): `cargo-nextest` è un prerequisito ambientale obbligatorio
//! (`cargo_nextest_must_be_available_in_this_environment`). Lo schema dello
//! stream `libtest-json` è il formato libtest, verificato su OUTPUT REALE di
//! nextest 0.9.148; la fixture qui sotto è presa verbatim da quella cattura.
//!
//! I test che mutano le variabili d'ambiente (NXM_CARGO, PATH) prendono il
//! lock condiviso: i test Rust girano in thread paralleli e l'env è
//! per-processo.

use std::env;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use nxm_mcp_exec::exec::test_runner::cargo_nextest_available;
use nxm_mcp_exec::handler::dispatch;
use nxm_mcp_exec::protocol::JsonRpcRequest;
use serde_json::json;
use tempfile::TempDir;

/// Le variabili di ambiente sono per-processo: i test che le mutano devono
/// essere sequenziali rispetto ai loro pari. I test Rust girano in thread
/// paralleli, quindi ogni test che tocca l'env prende questo lock.
static ENV_LOCK: Mutex<()> = Mutex::new(());

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
    JsonRpcRequest {
        jsonrpc: Some("2.0".into()),
        id: Some(json!(3)),
        method: "tools/call".into(),
        params: json!({ "name": "cargo_test", "arguments": args }),
    }
}

#[test]
fn cargo_nextest_must_be_available_in_this_environment() {
    // I test d'integrazione nextest DEVONO eseguire nextest reale (divieto di
    // skip): l'ambiente delegato è obbligato ad avere cargo-nextest installato.
    // Se questo test fallisce, installare: `cargo install cargo-nextest --locked`.
    let _guard = env_guard();
    assert!(
        cargo_nextest_available(),
        "cargo-nextest not available: integration tests must run real nextest \
         (install with `cargo install cargo-nextest --locked`)"
    );
}

#[test]
fn nextest_command_spec_uses_libtest_json_flag_before_double_dash() {
    // L'invocazione nextest corretta (bug exec-03c, confermato contro
    // https://nexte.st/docs/machine-readable/libtest-json/):
    // - `--message-format libtest-json` è un flag DI nextest e va PRIMA della
    //   prima parte posizionale (il filtro):
    //   `cargo nextest run --message-format libtest-json`.
    // - `--format=json` NON esiste in nextest e non deve comparire.
    // - Il gate sperimentale `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1` DEVE essere
    //   nell'env del processo nextest.
    use nxm_mcp_exec::exec::nextest_runner::nextest_command_spec;
    use nxm_mcp_exec::exec::test_runner::CargoTestArgs;

    let args = CargoTestArgs {
        repo_path: PathBuf::from("."),
        crate_name: Some("demo".to_string()),
        filter: Some("alpha".to_string()),
        timeout: None,
    };

    let spec = nextest_command_spec(&args).expect("spec is buildable");

    assert!(spec.args.starts_with(&["nextest".to_string(), "run".to_string()]));
    let fmt_pos = spec
        .args
        .iter()
        .position(|a| a == "--message-format")
        .expect("`--message-format` flag must be present in the nextest argv");
    assert!(
        fmt_pos > 1,
        "`--message-format` must be a nextest-level flag (after `nextest run`): {:?}",
        spec.args
    );
    assert_eq!(
        spec.args[fmt_pos + 1], "libtest-json",
        "`--message-format` must take `libtest-json` as its value: {:?}",
        spec.args
    );
    assert!(
        !spec.args.contains(&"--".to_string()),
        "the nextest invocation must not use a `--` separator: {:?}",
        spec.args
    );
    assert!(
        !spec.args.contains(&"--format=json".to_string()),
        "`--format=json` does not exist in nextest and must not appear: {:?}",
        spec.args
    );
    assert!(
        spec.args.contains(&"-p".to_string()) && spec.args.contains(&"demo".to_string()),
        "`-p <crate>` must be present when crate is set: {:?}",
        spec.args
    );
    let filter_pos = spec
        .args
        .iter()
        .position(|a| a == "alpha")
        .expect("the filter must be in the argv");
    assert!(
        filter_pos > fmt_pos + 1,
        "the filter must be a positional argument AFTER the `--message-format` value: {:?}",
        spec.args
    );

    let gate = spec
        .env
        .iter()
        .find(|(k, _)| k == "NEXTEST_EXPERIMENTAL_LIBTEST_JSON")
        .expect("experimental libtest-json gate must be set in the nextest env");
    assert_eq!(gate.1, "1");
}

#[test]
fn libtest_json_real_nextest_output_mapping_to_test_outcome() {
    // Mappatura output nextest machine-readable → TestOutcome. La fixture è
    // presa VERBATIM dall'OUTPUT REALE di nextest 0.9.148
    // (`NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run
    // --message-format libtest-json`, su un repo con 2 test verdi, 1 rosso,
    // un #[ignore]): lo schema è il formato libtest
    // (`event: "ok"|"failed"|"ignored"`), quindi il consumo avviene tramite
    // `parse_libtest_stream` — il parser nextest dedicato era ridondante ed è
    // stato rimosso.
    use nxm_mcp_exec::exec::test_parser::parse_libtest_stream;

    let stream = r#"{"type":"suite","event":"started","test_count":5}
{"type":"test","event":"started","name":"demo::demo$ignored_skipped"}
{"type":"test","event":"started","name":"demo::demo$red_test"}
{"type":"test","event":"started","name":"demo::demo$green_test"}
{"type":"test","event":"started","name":"demo::demo$alpha_extra"}
{"type":"test","event":"ignored","name":"demo::demo$ignored_skipped"}
{"type":"test","event":"ok","name":"demo::demo$alpha_extra","exec_time":0.010005083}
{"type":"test","event":"ok","name":"demo::demo$ignored_test","exec_time":0.009974167}
{"type":"test","event":"ok","name":"demo::demo$green_test","exec_time":0.010009}
{"type":"test","event":"failed","name":"demo::demo$red_test","exec_time":0.0104735,"stdout":"\nthread 'red_test' (48410655) panicked at src/lib.rs:4:17:\nassertion `left == right` failed\n  left: 1\n right: 2\nnote: run with `RUST_BACKTRACE=1` to display a backtrace\n"}
{"type":"suite","event":"failed","passed":3,"failed":1,"ignored":1,"measured":0,"filtered_out":3,"exec_time":0.0104735}"#;

    let outcome = parse_libtest_stream(stream);
    assert_eq!(outcome.passed, 3);
    assert_eq!(outcome.failed, 1);
    assert_eq!(outcome.ignored, 1);
    assert_eq!(outcome.failures.len(), 1);
    // Nome reale FQCN di nextest: `<crate>::<lib>$<test>`.
    assert_eq!(outcome.failures[0].name, "demo::demo$red_test");
    assert_eq!(
        outcome.failures[0].message,
        "thread 'red_test' (48410655) panicked at src/lib.rs:4:17:"
    );
}

#[test]
fn cargo_test_green_repo_via_nextest() {
    // Integrazione green su stable via nextest. NESSUN skip: nextest DEVE
    // essere presente nell'ambiente delegato e deve ESSERE ESEGUITO davvero.
    let _guard = env_guard();
    assert!(cargo_nextest_available(), "nextest must be installed");

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
    // Integrazione red su stable via nextest. NESSUN skip: nextest DEVE essere
    // presente e deve ESSERE ESEGUITO davvero. In produzione il nome è FQCN
    // (`<crate>::<lib>$<test>`, verificato sull'output reale di nextest), quindi
    // l'asserzione è sul suffisso del nome del test fallito.
    let _guard = env_guard();
    assert!(cargo_nextest_available(), "nextest must be installed");

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
        failure_names.iter().any(|n| n.ends_with("it_fails")),
        "failures must contain the failing test (FQCN suffix `it_fails`): {failure_names:?}"
    );

    env::remove_var("NXM_CARGO");
}

#[test]
fn cargo_test_filter_applied_via_nextest() {
    // Filtro applicato via nextest. NESSUN skip: nextest DEVE essere presente e
    // deve ESSERE ESEGUITO davvero. Nextest (filtro posizionale reale) matcha
    // per substring sul nome completo: solo `alpha_test` deve eseguirsi.
    let _guard = env_guard();
    assert!(cargo_nextest_available(), "nextest must be installed");

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

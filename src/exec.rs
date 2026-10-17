//! Execution layer: run allow-listed cargo commands and turn their JSON
//! output into typed, structured diagnostics.
//!
//! Livello di esecuzione: esegue comandi cargo in allow-list e trasforma il
//! loro output JSON in diagnostica strutturata e tipata.
//!
//! The cardinal value of this server is **structured diagnostics**: an agent
//! receives `{ok, diagnostics:[{level,code,file,line,message}], summary}`
//! instead of hundreds of lines of human-formatted log text.
//!
//! Il valore cardine di questo server è la **diagnostica strutturata**: un
//! agent riceve `{ok, diagnostics:[...], summary}` invece di centinaia di
//! righe di log in formato umano.

pub mod cargo_runner;
pub mod clippy_runner;
pub mod fmt_runner;
pub mod gate_runner;
pub mod nextest_runner;
pub mod parser;
pub mod runner;
pub mod test_parser;
pub mod test_runner;

pub use cargo_runner::{is_allowed_subcommand, run_cargo, wait_with_timeout, DEFAULT_TIMEOUT};
pub use clippy_runner::{cargo_clippy, ClippyArgs};
pub use fmt_runner::{cargo_fmt_check, parse_fmt_output, FmtArgs, FmtOutcome};
pub use gate_runner::{
    gate_steps, run_gate, GateOutcome, GateStepOutcome, GateVerdict, DEFAULT_GATE,
};
pub use nextest_runner::{nextest_command_spec, run_nextest, NextestCommandSpec};
pub use parser::{parse_cargo_stream, CheckOutcome, Diagnostic, Summary};
pub use runner::{cargo_check, CargoCheckArgs};
pub use test_parser::{parse_libtest_stream, TestFailure, TestOutcome};
pub use test_runner::{cargo_test, CargoTestArgs};

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

pub mod parser;
pub mod runner;

pub use parser::{parse_cargo_stream, CheckOutcome, Diagnostic, Summary};
pub use runner::{cargo_check, CargoCheckArgs};

//! nxm-mcp-exec — an MCP server that runs allow-listed Rust toolchain commands
//! and returns structured, typed diagnostics for coding agents.
//!
//! nxm-mcp-exec — un server MCP che esegue comandi della toolchain Rust in
//! allow-list e ritorna diagnostica strutturata e tipata per gli agent di coding.
//!
//! The library crate exposes the inner modules so integration tests in
//! `tests/` can exercise the parser and handler directly (RULES.md: tests live
//! in `tests/`, never inline in `src/`).

pub mod exec;
pub mod handler;
pub mod protocol;
pub mod tools;

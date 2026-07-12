//! The shared store: all SQLite-backed domain logic, free of any Tauri or
//! audio dependency, so both the desktop app and the MCP sidecar can call it.

pub mod databases;
pub mod documents;
pub mod ingest;
pub mod knowledge;
pub mod pages;
pub mod search;

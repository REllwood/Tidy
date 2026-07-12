//! appflower-core — the shared, UI-agnostic heart of AppFlower.
//!
//! It owns the SQLite schema/migrations, the error type, the local-LLM client,
//! and the whole `store` domain layer (pages, documents, databases, knowledge,
//! search, ingest). The Tauri desktop app and the MCP sidecar both link this
//! crate and call the exact same `store::…::core` functions, so there is one
//! source of truth for behavior.

pub mod db;
pub mod error;
pub mod llm;
pub mod store;
pub mod vault;

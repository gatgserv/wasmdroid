//! Runtime-neutral engine contracts and durable storage. No Wasmer dependencies.
pub mod api;
pub mod backup;
pub mod catalog;
pub mod catalog_assets;
pub mod catalog_install;
pub mod catalog_media;
pub mod catalog_updates;
pub mod compiled_cache;
pub mod data;
pub mod driver;
pub mod installer;
pub mod local_import;
mod locking;
pub mod logs;
pub mod network;
pub mod operation;
pub mod process_stats;
pub mod registry;
pub mod session;
pub mod stacks;
pub mod state;
pub mod storage_probe;
pub mod storage_usage;
pub mod store;
pub mod terminal;
pub mod volumes;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("unsupported capability: {0}")]
    Unsupported(String),
    #[error("network request failed: {0}")]
    Network(String),
    #[error("registry operation failed: {0}")]
    Registry(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("invalid state transition: {0}")]
    InvalidTransition(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

//! Runtime-neutral engine contracts and durable storage. No Wasmer dependencies.
pub mod api;
pub mod catalog;
pub mod driver;
pub mod installer;
pub mod registry;
pub mod state;
pub mod store;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Debug, Error)]
pub enum EngineError {
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

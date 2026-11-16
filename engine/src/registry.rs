//! Runtime-neutral registry boundary. Network implementations are a later Stage 0 story.
use crate::{Result, api::Artifact};
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPackage {
    pub reference: crate::api::ArtifactReference,
    pub publisher: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPage {
    pub packages: Vec<RegistryPackage>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedArtifact {
    pub artifact: Artifact,
    pub download_url: String,
}

pub trait Registry: Send + Sync {
    fn search(&self, query: &str, cursor: Option<&str>, limit: u32) -> Result<SearchPage>;
    /// Resolve mutable references to immutable digest and version metadata.
    fn resolve(&self, reference: &crate::api::ArtifactReference) -> Result<ResolvedArtifact>;
    /// Downloaders must enforce HTTPS/redirect policy and timeouts. The engine
    /// additionally verifies digest and bounds streamed bytes in ContentStore.
    fn download(&self, artifact: &ResolvedArtifact) -> Result<Box<dyn Read + Send>>;
}

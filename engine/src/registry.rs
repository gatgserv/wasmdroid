//! Runtime-neutral registry boundary. Only the Wasmer adapter knows GraphQL semantics.
pub(crate) mod http;
pub mod wasmer;
use crate::{Result, api::Artifact};
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPackage {
    pub reference: crate::api::ArtifactReference,
    pub publisher: String,
    pub description: String,
    /// Timestamp for the exact release, in RFC 3339 when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_at: Option<String>,
    /// WebC version returned for this release when requesting a v3 distribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webc_version: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPage {
    pub packages: Vec<RegistryPackage>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ResolvedArtifact {
    pub artifact: Artifact,
    /// Temporary capability: keep in memory; never persist or log this URL.
    pub download_url: String,
    #[serde(default)]
    pub dependencies: Vec<PackageRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRequirement {
    pub reference: crate::api::ArtifactReference,
    pub version_requirement: String,
}

impl std::fmt::Debug for ResolvedArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedArtifact")
            .field("artifact", &self.artifact)
            .field("download_url", &"<redacted>")
            .field("dependencies", &self.dependencies)
            .finish()
    }
}

pub trait Registry: Send + Sync {
    fn search(&self, query: &str, cursor: Option<&str>, limit: u32) -> Result<SearchPage>;
    /// Resolve mutable references to immutable digest and version metadata.
    fn resolve(&self, reference: &crate::api::ArtifactReference) -> Result<ResolvedArtifact>;
    fn resolve_requirement(&self, requirement: &PackageRequirement) -> Result<ResolvedArtifact>;
    /// Downloaders must enforce HTTPS/redirect policy and timeouts. The engine
    /// additionally verifies digest and bounds streamed bytes in ContentStore.
    fn download(&self, artifact: &ResolvedArtifact) -> Result<Box<dyn Read + Send>>;
}

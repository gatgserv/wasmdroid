use crate::{
    EngineError, Result,
    api::{Artifact, ArtifactReference},
    registry::{Registry, ResolvedArtifact},
    state::StateDb,
    store::ContentStore,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

pub const DEPENDENCIES_LABEL: &str = "dev.wasmdroid.dependency-images";

#[derive(Debug, Clone, Copy)]
pub struct InstallLimits {
    pub max_blob_bytes: u64,
    pub max_graph_bytes: u64,
    pub max_packages: usize,
    pub max_depth: usize,
}
impl Default for InstallLimits {
    fn default() -> Self {
        Self {
            max_blob_bytes: 512 * 1024 * 1024,
            max_graph_bytes: 1024 * 1024 * 1024,
            max_packages: 32,
            max_depth: 16,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullPhase {
    Resolving,
    Downloading,
    Verifying,
    Installed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullProgress {
    pub reference: ArtifactReference,
    pub phase: PullPhase,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledGraph {
    pub root: Artifact,
    pub images: Vec<Artifact>,
}

pub struct Installer<'a> {
    pub registry: &'a dyn Registry,
    pub store: &'a ContentStore,
    pub state: &'a StateDb,
    pub limits: InstallLimits,
}

impl Installer<'_> {
    pub fn pull(
        &self,
        reference: &ArtifactReference,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<InstalledGraph> {
        let resolved = self.registry.resolve(reference)?;
        let root_ref = resolved.artifact.reference.clone();
        let mut graph = BTreeMap::new();
        self.visit(resolved, &mut graph, &mut BTreeSet::new(), 0, progress)?;
        let images: Vec<_> = graph.into_values().collect();
        self.state.put_artifacts(&images)?;
        let root = images
            .iter()
            .find(|a| a.reference == root_ref)
            .expect("resolved root is in graph")
            .clone();
        progress(PullProgress {
            reference: root_ref,
            phase: PullPhase::Installed,
            bytes_downloaded: root.size_bytes,
            total_bytes: root.size_bytes,
        });
        Ok(InstalledGraph { root, images })
    }

    fn visit(
        &self,
        mut resolved: ResolvedArtifact,
        graph: &mut BTreeMap<String, Artifact>,
        visiting: &mut BTreeSet<String>,
        depth: usize,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<()> {
        let key = resolved.artifact.reference.as_str().to_owned();
        if visiting.contains(&key) {
            return Err(EngineError::Invalid(
                "cyclic package dependency graph".into(),
            ));
        }
        if let Some(existing) = graph.get(&key) {
            if existing.digest != resolved.artifact.digest {
                return Err(EngineError::Conflict(
                    "dependency digest changed during resolution".into(),
                ));
            }
            return Ok(());
        }
        if depth > self.limits.max_depth || graph.len() + visiting.len() >= self.limits.max_packages
        {
            return Err(EngineError::Invalid(
                "package dependency graph exceeds limit".into(),
            ));
        }
        if resolved.artifact.size_bytes > self.limits.max_blob_bytes {
            return Err(EngineError::Invalid(
                "package exceeds download limit".into(),
            ));
        }
        visiting.insert(key.clone());
        progress(event(&resolved.artifact, PullPhase::Resolving, 0));
        let mut dependencies = Vec::new();
        for requirement in &resolved.dependencies {
            let dependency = self.registry.resolve_requirement(requirement)?;
            dependencies.push(dependency.artifact.reference.clone());
            self.visit(dependency, graph, visiting, depth + 1, progress)?;
        }
        let graph_bytes = graph
            .values()
            .try_fold(resolved.artifact.size_bytes, |total, a| {
                total.checked_add(a.size_bytes)
            })
            .ok_or_else(|| EngineError::Invalid("package graph size overflow".into()))?;
        if graph_bytes > self.limits.max_graph_bytes {
            return Err(EngineError::Invalid(
                "package graph exceeds download limit".into(),
            ));
        }
        if self
            .store
            .verify(&resolved.artifact.digest, resolved.artifact.size_bytes)
            .ok()
            != Some(resolved.artifact.size_bytes)
        {
            let reader = self.registry.download(&resolved)?;
            progress(event(&resolved.artifact, PullPhase::Downloading, 0));
            let reader = ProgressReader {
                reader,
                artifact: &resolved.artifact,
                downloaded: 0,
                progress,
            };
            let size = self.store.ingest(
                reader,
                &resolved.artifact.digest,
                resolved.artifact.size_bytes,
            )?;
            if size != resolved.artifact.size_bytes {
                return Err(EngineError::Registry(
                    "downloaded package is truncated".into(),
                ));
            }
        }
        progress(event(
            &resolved.artifact,
            PullPhase::Verifying,
            resolved.artifact.size_bytes,
        ));
        resolved.artifact.labels.insert(
            DEPENDENCIES_LABEL.into(),
            serde_json::to_string(&dependencies)?,
        );
        visiting.remove(&key);
        graph.insert(key, resolved.artifact);
        Ok(())
    }
}

fn event(artifact: &Artifact, phase: PullPhase, bytes_downloaded: u64) -> PullProgress {
    PullProgress {
        reference: artifact.reference.clone(),
        phase,
        bytes_downloaded,
        total_bytes: artifact.size_bytes,
    }
}

struct ProgressReader<'a> {
    reader: Box<dyn Read + Send>,
    artifact: &'a Artifact,
    downloaded: u64,
    progress: &'a mut dyn FnMut(PullProgress),
}
impl Read for ProgressReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.reader.read(buffer)?;
        self.downloaded += count as u64;
        (self.progress)(event(
            self.artifact,
            PullPhase::Downloading,
            self.downloaded,
        ));
        Ok(count)
    }
}

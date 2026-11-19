use crate::{
    EngineError, Result,
    api::{Artifact, ArtifactReference},
    operation::{OperationControl, OperationGuard, OperationState},
    registry::{Registry, ResolvedArtifact},
    state::StateDb,
    store::{ContentStore, Digest, InstallBudget, StoreWriter},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    sync::Arc,
    time::{Duration, Instant},
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

struct Work<'a> {
    graph: BTreeMap<String, Artifact>,
    root_pins: BTreeMap<String, Digest>,
    visiting: BTreeSet<String>,
    control: &'a OperationControl,
    writer: StoreWriter,
    budget: InstallBudget,
    progress: &'a mut dyn FnMut(PullProgress),
}
impl Installer<'_> {
    pub fn pull(
        &self,
        reference: &ArtifactReference,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<InstalledGraph> {
        self.pull_controlled(
            reference,
            &Arc::new(OperationControl::default()),
            self.default_budget(),
            progress,
        )
    }
    pub fn pull_resolved(
        &self,
        resolved: ResolvedArtifact,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<InstalledGraph> {
        self.pull_resolved_controlled(
            resolved,
            &Arc::new(OperationControl::default()),
            self.default_budget(),
            progress,
        )
    }
    fn default_budget(&self) -> InstallBudget {
        InstallBudget {
            max_new_bytes: self.limits.max_graph_bytes,
            max_store_bytes: None,
        }
    }
    /// The registry transport must share this token to cancel pending I/O;
    /// arbitrary third-party blocking Registry implementations are cooperative.
    pub fn pull_controlled(
        &self,
        reference: &ArtifactReference,
        control: &Arc<OperationControl>,
        budget: InstallBudget,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<InstalledGraph> {
        let mut operation = control.claim()?;
        self.pull_in_operation(reference, None, &mut operation, budget, progress)
    }
    pub fn pull_expected_controlled(
        &self,
        reference: &ArtifactReference,
        expected: &crate::store::Digest,
        control: &Arc<OperationControl>,
        budget: InstallBudget,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<InstalledGraph> {
        let mut operation = control.claim()?;
        self.pull_in_operation(reference, Some(expected), &mut operation, budget, progress)
    }
    /// A caller may claim before validating catalog/input. Only its guard may
    /// finalize this operation; a failed second token claim cannot alter it.
    pub fn pull_in_operation(
        &self,
        reference: &ArtifactReference,
        expected: Option<&crate::store::Digest>,
        operation: &mut OperationGuard,
        budget: InstallBudget,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<InstalledGraph> {
        let control = operation.control();
        check_active(control)?;
        let resolved = self.registry.resolve(reference);
        control.check()?;
        let resolved = resolved?;
        if let Some(expected) = expected
            && resolved.artifact.digest != *expected
        {
            return Err(EngineError::DigestMismatch {
                expected: expected.as_str().into(),
                actual: resolved.artifact.digest.as_str().into(),
            });
        }
        self.install(resolved, control, budget, progress)
    }
    /// Resolve and pin every requested root before downloading any blob, then
    /// publish their union in one transaction. Results preserve request order;
    /// each graph's images contains the same deduplicated union of all roots.
    /// Duplicate requested references are rejected, even with identical pins.
    /// The registry transport must share the operation's cancellation token.
    pub fn pull_many_expected_in_operation(
        &self,
        roots: &[(ArtifactReference, Digest)],
        operation: &mut OperationGuard,
        budget: InstallBudget,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<Vec<InstalledGraph>> {
        let control = operation.control();
        check_active(control)?;
        if roots.is_empty() || roots.len() > 8 {
            return Err(EngineError::Invalid(
                "installation requires one to eight package roots".into(),
            ));
        }
        let mut requested = BTreeMap::new();
        for (reference, digest) in roots {
            if let Some(previous) = requested.insert(reference.as_str(), digest) {
                return Err(if previous == digest {
                    EngineError::Invalid("duplicate installation root".into())
                } else {
                    EngineError::Conflict("conflicting installation root pins".into())
                });
            }
        }
        let mut resolved_roots = Vec::with_capacity(roots.len());
        let mut canonical = BTreeMap::new();
        for (reference, expected) in roots {
            control.check()?;
            let resolved = self.registry.resolve(reference);
            control.check()?;
            let resolved = resolved?;
            if resolved.artifact.reference != *reference {
                return Err(EngineError::Conflict(
                    "resolved installation root reference differs from request".into(),
                ));
            }
            if resolved.artifact.digest != *expected {
                return Err(EngineError::DigestMismatch {
                    expected: expected.as_str().into(),
                    actual: resolved.artifact.digest.as_str().into(),
                });
            }
            if let Some(previous) = canonical.insert(
                resolved.artifact.reference.as_str().to_owned(),
                expected.clone(),
            ) {
                return Err(if previous == *expected {
                    EngineError::Invalid("duplicate resolved installation root".into())
                } else {
                    EngineError::Conflict("conflicting resolved installation root pins".into())
                });
            }
            resolved_roots.push(resolved);
        }
        self.install_many(resolved_roots, control, budget, progress)
    }
    pub fn pull_resolved_controlled(
        &self,
        resolved: ResolvedArtifact,
        control: &Arc<OperationControl>,
        budget: InstallBudget,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<InstalledGraph> {
        let _operation = control.claim()?;
        self.install(resolved, control, budget, progress)
    }
    fn install(
        &self,
        resolved: ResolvedArtifact,
        control: &OperationControl,
        budget: InstallBudget,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<InstalledGraph> {
        Ok(self
            .install_many(vec![resolved], control, budget, progress)?
            .remove(0))
    }
    fn install_many(
        &self,
        resolved: Vec<ResolvedArtifact>,
        control: &OperationControl,
        budget: InstallBudget,
        progress: &mut dyn FnMut(PullProgress),
    ) -> Result<Vec<InstalledGraph>> {
        check_active(control)?;
        // Shared maintenance lease protects the complete graph until publication.
        // Exclusive writer admission bounds all staging paths without blocking
        // existing service reads or silently evicting installed packages.
        let _lease = self.store.installation_lease()?;
        let root_refs: Vec<_> = resolved
            .iter()
            .map(|root| root.artifact.reference.clone())
            .collect();
        let root_pins = resolved
            .iter()
            .map(|root| {
                (
                    root.artifact.reference.as_str().to_owned(),
                    root.artifact.digest.clone(),
                )
            })
            .collect();
        let mut work = Work {
            graph: BTreeMap::new(),
            root_pins,
            visiting: BTreeSet::new(),
            control,
            writer: self.store.writer()?,
            budget,
            progress,
        };
        for root in resolved {
            self.visit(root, 0, &mut work)?;
        }
        let images: Vec<_> = work.graph.into_values().collect();
        control.begin_publication()?;
        self.state.put_artifacts(&images)?;
        control.committed();
        let mut graphs = Vec::with_capacity(root_refs.len());
        for root_ref in root_refs {
            let root = images
                .iter()
                .find(|image| image.reference == root_ref)
                .expect("resolved root is in graph")
                .clone();
            (work.progress)(event(&root, PullPhase::Installed, root.size_bytes));
            graphs.push(InstalledGraph {
                root,
                images: images.clone(),
            });
        }
        Ok(graphs)
    }
    fn visit(
        &self,
        mut resolved: ResolvedArtifact,
        depth: usize,
        work: &mut Work<'_>,
    ) -> Result<()> {
        work.control.check()?;
        let key = resolved.artifact.reference.as_str().to_owned();
        if let Some(expected) = work.root_pins.get(&key)
            && *expected != resolved.artifact.digest
        {
            return Err(EngineError::Conflict(
                "dependency conflicts with installation root pin".into(),
            ));
        }
        if work.visiting.contains(&key) {
            return Err(EngineError::Invalid(
                "cyclic package dependency graph".into(),
            ));
        }
        if let Some(existing) = work.graph.get(&key) {
            if existing.digest != resolved.artifact.digest {
                return Err(EngineError::Conflict(
                    "dependency digest changed during resolution".into(),
                ));
            }
            return Ok(());
        }
        if depth > self.limits.max_depth
            || work.graph.len() + work.visiting.len() >= self.limits.max_packages
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
        work.visiting.insert(key.clone());
        (work.progress)(event(&resolved.artifact, PullPhase::Resolving, 0));
        let mut dependencies = Vec::new();
        for requirement in &resolved.dependencies {
            work.control.check()?;
            let dependency = self.registry.resolve_requirement(requirement);
            work.control.check()?;
            let dependency = dependency?;
            dependencies.push(dependency.artifact.reference.clone());
            self.visit(dependency, depth + 1, work)?;
        }
        let graph_bytes = work
            .graph
            .values()
            .try_fold(resolved.artifact.size_bytes, |total, image| {
                total.checked_add(image.size_bytes)
            })
            .ok_or_else(|| EngineError::Invalid("package graph size overflow".into()))?;
        if graph_bytes > self.limits.max_graph_bytes {
            return Err(EngineError::Invalid(
                "package graph exceeds download limit".into(),
            ));
        }
        let cached = match self.store.verify_controlled(
            &resolved.artifact.digest,
            resolved.artifact.size_bytes,
            work.control,
        ) {
            Ok(size) => size == resolved.artifact.size_bytes,
            Err(EngineError::Cancelled) => return Err(EngineError::Cancelled),
            Err(_) => false,
        };
        if !cached {
            work.control.check()?;
            self.store.check_write_admission(
                &work.writer,
                work.budget,
                resolved.artifact.size_bytes,
                work.control,
            )?;
            let reader = self.registry.download(&resolved);
            work.control.check()?;
            let reader = reader?;
            (work.progress)(event(&resolved.artifact, PullPhase::Downloading, 0));
            let reader = ProgressReader {
                reader,
                artifact: &resolved.artifact,
                downloaded: 0,
                progress: work.progress,
                control: work.control,
                last: Instant::now(),
            };
            let size = self.store.ingest_controlled(
                reader,
                &resolved.artifact.digest,
                resolved.artifact.size_bytes,
                &mut work.writer,
                work.budget,
                work.control,
            )?;
            if size != resolved.artifact.size_bytes {
                return Err(EngineError::Registry(
                    "downloaded package is truncated".into(),
                ));
            }
        }
        work.control.check()?;
        (work.progress)(event(
            &resolved.artifact,
            PullPhase::Verifying,
            resolved.artifact.size_bytes,
        ));
        resolved.artifact.labels.insert(
            DEPENDENCIES_LABEL.into(),
            serde_json::to_string(&dependencies)?,
        );
        work.visiting.remove(&key);
        work.graph.insert(key, resolved.artifact);
        Ok(())
    }
}

fn check_active(control: &OperationControl) -> Result<()> {
    control.check()?;
    if control.state() != OperationState::Active {
        return Err(EngineError::Conflict("operation cannot install".into()));
    }
    Ok(())
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
    control: &'a OperationControl,
    last: Instant,
}
impl Read for ProgressReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.control.check().map_err(std::io::Error::other)?;
        let count = self.reader.read(buffer)?;
        self.control.check().map_err(std::io::Error::other)?;
        self.downloaded = self.downloaded.saturating_add(count as u64);
        if self.last.elapsed() >= Duration::from_millis(100)
            || self.downloaded == self.artifact.size_bytes
        {
            (self.progress)(event(
                self.artifact,
                PullPhase::Downloading,
                self.downloaded,
            ));
            self.last = Instant::now();
        }
        Ok(count)
    }
}

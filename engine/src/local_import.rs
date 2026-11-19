//! Local bytes are capabilities, not host paths or registry publisher identities.
use crate::{
    EngineError, Result,
    api::{Artifact, ArtifactKind, ArtifactReference},
    installer::DEPENDENCIES_LABEL,
    operation::{OperationControl, OperationGuard},
    session::EngineSession,
    state::StateDb,
    store::{ContentStore, Digest, InstallBudget, StoreLease},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
    sync::Arc,
};

pub const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;
pub const ORIGIN_LABEL: &str = "dev.wasmdroid.local.origin";
pub const PINS_LABEL: &str = "dev.wasmdroid.local.dependencies";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Dependency {
    Named {
        reference: ArtifactReference,
        version_requirement: String,
    },
    Sha256 {
        digest: Digest,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inspection {
    pub kind: ArtifactKind,
    pub commands: Vec<String>,
    pub entrypoint: Option<String>,
    pub dependencies: Vec<Dependency>,
}
/// Implemented only by the runtime driver. The file is sealed app-owned staging;
/// implementations must not execute it or open manifest-selected host resources.
pub trait Inspector {
    fn inspect(&self, file: &File, control: &OperationControl) -> Result<Inspection>;
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DependencyPin {
    pub reference: ArtifactReference,
    pub digest: Digest,
    pub metadata: Digest,
}
#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    pub schema_version: u32,
    pub generation: String,
    pub artifact: Artifact,
    pub requirements: Vec<Dependency>,
    pub pins: Vec<DependencyPin>,
    pub missing: Vec<Dependency>,
    pub token: Digest,
}
pub fn reference(digest: &Digest) -> Result<ArtifactReference> {
    ArtifactReference::parse(format!("local.wasmdroid/sha256/{}", &digest.as_str()[7..]))
}
fn fingerprint(artifact: &Artifact) -> Result<Digest> {
    Ok(Digest::of(&serde_json::to_vec(artifact)?))
}
fn invalid(reason: &str) -> EngineError {
    EngineError::Invalid(reason.into())
}
fn pin(artifact: &Artifact) -> Result<DependencyPin> {
    Ok(DependencyPin {
        reference: artifact.reference.clone(),
        digest: artifact.digest.clone(),
        metadata: fingerprint(artifact)?,
    })
}
fn inspect_contract(value: &Inspection) -> Result<()> {
    if !matches!(value.kind, ArtifactKind::Webc | ArtifactKind::Wasm)
        || value.commands.is_empty()
        || value.commands.len() > 256
        || value.dependencies.len() > 32
    {
        return Err(invalid(
            "local package exceeds supported command/dependency bounds",
        ));
    }
    let mut names = BTreeSet::new();
    for name in &value.commands {
        if name.is_empty()
            || name.len() > 128
            || name.chars().any(char::is_control)
            || !names.insert(name)
        {
            return Err(invalid("invalid local command metadata"));
        }
    }
    if value
        .entrypoint
        .as_ref()
        .is_some_and(|entry| !names.contains(entry))
    {
        return Err(invalid("local entrypoint is not a command"));
    }
    if value.kind == ArtifactKind::Wasm
        && (!value.dependencies.is_empty() || value.commands != ["main"])
    {
        return Err(invalid("invalid core Wasm command descriptor"));
    }
    Ok(())
}
pub fn graph(state: &StateDb, root: &Artifact) -> Result<Vec<Artifact>> {
    if root.reference != reference(&root.digest)?
        || root.size_bytes > MAX_IMPORT_BYTES
        || root.labels.get(ORIGIN_LABEL).map(String::as_str) != Some("local_import")
        || root.driver != "wasmer-local"
        || !matches!(root.kind, ArtifactKind::Wasm | ArtifactKind::Webc)
    {
        return Err(invalid("invalid local artifact identity"));
    }
    let raw = root
        .labels
        .get(PINS_LABEL)
        .ok_or_else(|| invalid("local dependency lock missing"))?;
    if raw.len() > 32 * 1024 {
        return Err(invalid("local dependency lock exceeds limit"));
    }
    let pins: Vec<DependencyPin> = serde_json::from_str(raw)?;
    if pins.len() > 31 || (root.kind == ArtifactKind::Wasm && !pins.is_empty()) {
        return Err(invalid("invalid local dependency lock"));
    }
    let mut result = vec![root.clone()];
    let mut seen = BTreeSet::from([root.reference.as_str().to_owned()]);
    let mut total = root.size_bytes;
    for expected in pins {
        if !seen.insert(expected.reference.as_str().to_owned()) {
            return Err(invalid("duplicate local dependency lock"));
        }
        let actual = state.artifact(&expected.reference)?;
        if actual.kind != ArtifactKind::Webc
            || actual.driver != "wasmer-local"
            || pin(&actual)? != expected
        {
            return Err(EngineError::Conflict(
                "installed local dependency lock changed; reimport after review".into(),
            ));
        }
        total = total
            .checked_add(actual.size_bytes)
            .ok_or_else(|| invalid("local graph size overflow"))?;
        if total > 1024 * 1024 * 1024 {
            return Err(invalid("local graph exceeds byte limit"));
        }
        result.push(actual);
    }
    Ok(result)
}
fn resolve(
    state: &StateDb,
    requirements: &[Dependency],
    control: &OperationControl,
) -> Result<(Vec<DependencyPin>, Vec<Dependency>)> {
    let available: Vec<_> = state
        .artifacts()?
        .into_iter()
        .filter(|image| image.kind == ArtifactKind::Webc && image.driver == "wasmer-local")
        .collect();
    let mut selected = BTreeMap::new();
    let mut missing = Vec::new();
    for requirement in requirements {
        control.check()?;
        let chosen = match requirement {
            Dependency::Sha256 { digest } => available.iter().find(|image| image.digest == *digest),
            Dependency::Named {
                reference,
                version_requirement,
            } => {
                let wanted = reference
                    .as_str()
                    .strip_prefix("wasmer.io/")
                    .ok_or_else(|| invalid("local dependency registry is unsupported"))?;
                if wanted.contains('@') || wanted.len() > 256 || version_requirement.len() > 256 {
                    return Err(invalid("invalid local dependency requirement"));
                }
                let range = semver::VersionReq::parse(version_requirement)
                    .map_err(|_| invalid("invalid local dependency version range"))?;
                available
                    .iter()
                    .filter_map(|image| {
                        let (name, version) = image
                            .reference
                            .as_str()
                            .strip_prefix("wasmer.io/")?
                            .rsplit_once('@')?;
                        let version = semver::Version::parse(version).ok()?;
                        (name == wanted && range.matches(&version)).then_some((version, image))
                    })
                    .max_by(|left, right| left.0.cmp(&right.0))
                    .map(|(_, image)| image)
            }
        };
        if let Some(chosen) = chosen {
            collect(chosen, 0, &available, &mut selected, control)?;
        } else {
            missing.push(requirement.clone());
        }
    }
    Ok((selected.values().map(pin).collect::<Result<_>>()?, missing))
}
fn collect(
    image: &Artifact,
    depth: usize,
    available: &[Artifact],
    selected: &mut BTreeMap<String, Artifact>,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    if selected.contains_key(image.reference.as_str()) {
        return Ok(());
    }
    if depth > 16 || selected.len() >= 31 {
        return Err(invalid("local dependency graph exceeds limit"));
    }
    selected.insert(image.reference.as_str().into(), image.clone());
    if let Some(raw) = image.labels.get(DEPENDENCIES_LABEL) {
        if raw.len() > 32 * 1024 {
            return Err(invalid("installed dependency metadata exceeds limit"));
        }
        let dependencies: Vec<ArtifactReference> = serde_json::from_str(raw)?;
        if dependencies.len() > 31 {
            return Err(invalid("installed dependency graph exceeds limit"));
        }
        for dependency in dependencies {
            let found = available
                .iter()
                .find(|item| item.reference == dependency)
                .ok_or_else(|| {
                    EngineError::NotFound("installed dependency closure is incomplete".into())
                })?;
            collect(found, depth + 1, available, selected, control)?;
        }
    }
    Ok(())
}

pub struct LocalImport {
    session: Option<EngineSession>,
    temporary: Option<tempfile::NamedTempFile>,
    lease: Option<StoreLease>,
    operation: Option<OperationGuard>,
    control: Arc<OperationControl>,
    budget: InstallBudget,
    expected: Option<u64>,
    total: u64,
    hash: Sha256,
    sealed: bool,
    generation: String,
    preview: Option<ImportPreview>,
}
impl Drop for LocalImport {
    fn drop(&mut self) {
        // Keep ownership through temporary/lease cleanup even when the FFI
        // capability is dropped after failure rather than explicitly aborted.
        self.release();
    }
}
impl LocalImport {
    pub fn begin(
        session: EngineSession,
        budget: InstallBudget,
        expected: Option<u64>,
        control: Arc<OperationControl>,
    ) -> Result<Self> {
        let operation = control.claim()?;
        if expected.is_some_and(|size| size > MAX_IMPORT_BYTES || size > budget.max_new_bytes) {
            return Err(invalid("local import exceeds space or file limit"));
        }
        let root = session.root().join("imports");
        match std::fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Err(error) = std::fs::create_dir(&root)
                    && error.kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(error.into());
                }
                let metadata = std::fs::symlink_metadata(&root)?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(invalid("invalid private import staging directory"));
                }
            }
            _ => return Err(invalid("invalid private import staging directory")),
        }
        let store = ContentStore::open(session.root().join("store"))?;
        let lease = store.installation_lease()?;
        let temporary = tempfile::Builder::new()
            .prefix("wasmdroid-import-")
            .tempfile_in(root)?;
        use ring::rand::SecureRandom;
        let mut nonce = [0u8; 16];
        ring::rand::SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| invalid("import identity unavailable"))?;
        Ok(Self {
            session: Some(session),
            temporary: Some(temporary),
            lease: Some(lease),
            operation: Some(operation),
            control,
            budget,
            expected,
            total: 0,
            hash: Sha256::new(),
            sealed: false,
            generation: hex::encode(nonce),
            preview: None,
        })
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<u64> {
        self.control.check()?;
        if self.sealed || self.session.is_none() {
            return Err(invalid("local import is sealed or closed"));
        }
        if bytes.len() > 64 * 1024 {
            return Err(invalid("import chunk exceeds limit"));
        }
        let next = self
            .total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("import size overflow"))?;
        if next > MAX_IMPORT_BYTES
            || next > self.budget.max_new_bytes
            || self.expected.is_some_and(|size| next > size)
        {
            return Err(invalid("local import exceeds size or space limit"));
        }
        let result = self
            .temporary
            .as_mut()
            .ok_or_else(|| invalid("local import is closed"))?
            .write_all(bytes);
        if let Err(error) = result {
            // A failed write_all may already have appended a prefix. Never
            // resume hashing a different byte stream under the same review.
            self.release();
            return Err(error.into());
        }
        self.hash.update(bytes);
        self.total = next;
        Ok(next)
    }
    pub fn preview(&mut self, inspector: &dyn Inspector) -> Result<&ImportPreview> {
        self.control.check()?;
        if self.expected.is_some_and(|size| size != self.total) || self.total == 0 {
            return Err(invalid("local import is empty or truncated"));
        }
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| invalid("local import is closed"))?;
        let file = self
            .temporary
            .as_ref()
            .ok_or_else(|| invalid("local import is closed"))?
            .as_file();
        file.sync_all()?;
        if file.metadata()?.len() != self.total {
            return Err(invalid("private import staging length changed"));
        }
        self.sealed = true;
        let value = inspector.inspect(file, &self.control)?;
        inspect_contract(&value)?;
        self.control.check()?;
        let digest = Digest::parse(format!(
            "sha256:{}",
            hex::encode(self.hash.clone().finalize())
        ))?;
        let (pins, missing) = resolve(session.state(), &value.dependencies, &self.control)?;
        let mut labels = BTreeMap::from([
            (ORIGIN_LABEL.into(), "local_import".into()),
            (PINS_LABEL.into(), serde_json::to_string(&pins)?),
            (
                DEPENDENCIES_LABEL.into(),
                serde_json::to_string(&pins.iter().map(|pin| &pin.reference).collect::<Vec<_>>())?,
            ),
        ]);
        if let Some(entry) = value.entrypoint {
            labels.insert("dev.wasmdroid.entrypoint".into(), entry);
        }
        let artifact = Artifact {
            reference: reference(&digest)?,
            digest,
            kind: value.kind,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: value.commands,
            size_bytes: self.total,
            labels,
        };
        let token = Digest::of(&serde_json::to_vec(&(
            &self.generation,
            &artifact,
            &pins,
            &missing,
        ))?);
        self.preview = Some(ImportPreview {
            schema_version: 1,
            generation: self.generation.clone(),
            artifact,
            requirements: value.dependencies,
            pins,
            missing,
            token,
        });
        Ok(self.preview.as_ref().expect("preview assigned"))
    }
    pub fn commit(&mut self, token: &str) -> Result<Artifact> {
        self.control.check()?;
        let preview = self
            .preview
            .as_ref()
            .ok_or_else(|| invalid("review the local import first"))?;
        if preview.token.as_str() != token {
            return Err(EngineError::Conflict("local import preview changed".into()));
        }
        if !preview.missing.is_empty() {
            return Err(EngineError::NotFound(
                "install missing dependencies and review the import again".into(),
            ));
        }
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| invalid("local import is closed"))?;
        let artifact = preview.artifact.clone();
        let store = ContentStore::open(session.root().join("store"))?;
        let mut writer = store.writer()?;
        graph(session.state(), &artifact)?;
        match session.state().artifact(&artifact.reference) {
            Ok(existing) if fingerprint(&existing)? != fingerprint(&artifact)? => {
                return Err(EngineError::Conflict(
                    "local content already has another dependency lock; remove it before reimport"
                        .into(),
                ));
            }
            Ok(_) | Err(EngineError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
        if store
            .verify_controlled(&artifact.digest, self.total, &self.control)
            .ok()
            != Some(self.total)
        {
            self.control.check()?;
            let input = self
                .temporary
                .as_ref()
                .ok_or_else(|| invalid("local import is closed"))?
                .reopen()?;
            store.ingest_controlled(
                input,
                &artifact.digest,
                self.total,
                &mut writer,
                InstallBudget {
                    max_new_bytes: self.budget.max_new_bytes - self.total,
                    max_store_bytes: self.budget.max_store_bytes,
                },
                &self.control,
            )?;
        }
        self.control.begin_publication()?;
        let result = session.state().put_artifact(&artifact);
        if result.is_ok() {
            self.control.committed();
        }
        self.release();
        result?;
        Ok(artifact)
    }
    pub fn abort(&mut self) {
        self.control.cancel();
        self.release();
    }
    fn release(&mut self) {
        self.preview = None;
        self.temporary = None;
        self.lease = None;
        self.operation = None;
        self.session = None;
    }
}

/// Only a fresh exclusive owner may remove abandoned private staging. Live
/// transfer capabilities retain that same owner lock, including across UI loss.
pub(crate) fn recover(session: &EngineSession) -> Result<()> {
    let root = session.root().join("imports");
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        _ => return Err(invalid("invalid private import staging directory")),
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("wasmdroid-import-"))
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    sync::atomic::{AtomicUsize, Ordering},
};
use wasmdroid_engine::{
    EngineError, Result,
    api::{Artifact, ArtifactKind, ArtifactReference},
    installer::{InstallLimits, Installer, PullPhase},
    registry::{PackageRequirement, Registry, ResolvedArtifact, SearchPage},
    state::StateDb,
    store::{ContentStore, Digest},
};

struct FixtureRegistry {
    packages: BTreeMap<String, (ResolvedArtifact, Vec<u8>)>,
    downloads: AtomicUsize,
}
impl Registry for FixtureRegistry {
    fn search(&self, _: &str, _: Option<&str>, _: u32) -> Result<SearchPage> {
        unreachable!()
    }
    fn resolve(&self, reference: &ArtifactReference) -> Result<ResolvedArtifact> {
        self.packages
            .get(reference.as_str())
            .map(|(a, _)| a.clone())
            .ok_or_else(|| EngineError::NotFound("fixture".into()))
    }
    fn resolve_requirement(&self, requirement: &PackageRequirement) -> Result<ResolvedArtifact> {
        self.resolve(&ArtifactReference::parse(format!(
            "{}@1.0.0",
            requirement.reference.as_str()
        ))?)
    }
    fn download(&self, artifact: &ResolvedArtifact) -> Result<Box<dyn Read + Send>> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Cursor::new(
            self.packages[artifact.artifact.reference.as_str()]
                .1
                .clone(),
        )))
    }
}
fn package(name: &str, dependencies: &[&str]) -> (ResolvedArtifact, Vec<u8>) {
    let bytes = format!("verified fixture for {name}").into_bytes();
    (
        ResolvedArtifact {
            artifact: Artifact {
                reference: ArtifactReference::parse(format!("wasmer.io/example/{name}@1.0.0"))
                    .unwrap(),
                digest: Digest::of(&bytes),
                kind: ArtifactKind::Webc,
                driver: "wasmer-local".into(),
                platform: "wasm32-wasix".into(),
                commands: vec!["run".into()],
                size_bytes: bytes.len() as u64,
                labels: BTreeMap::new(),
            },
            download_url: "https://example.invalid/blob".into(),
            dependencies: dependencies
                .iter()
                .map(|dep| PackageRequirement {
                    reference: ArtifactReference::parse(format!("wasmer.io/example/{dep}"))
                        .unwrap(),
                    version_requirement: "*".into(),
                })
                .collect(),
        },
        bytes,
    )
}
fn registry(packages: Vec<(ResolvedArtifact, Vec<u8>)>) -> FixtureRegistry {
    FixtureRegistry {
        packages: packages
            .into_iter()
            .map(|p| (p.0.artifact.reference.as_str().to_owned(), p))
            .collect(),
        downloads: AtomicUsize::new(0),
    }
}
fn root() -> ArtifactReference {
    ArtifactReference::parse("wasmer.io/example/root@1.0.0").unwrap()
}

#[test]
fn installs_complete_graph_atomically_and_reuses_verified_cache() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path().join("store")).unwrap();
    let db = StateDb::open(dir.path().join("state.db")).unwrap();
    let registry = registry(vec![package("root", &["child"]), package("child", &[])]);
    let installer = Installer {
        registry: &registry,
        store: &store,
        state: &db,
        limits: InstallLimits::default(),
    };
    let mut phases = Vec::new();
    let graph = installer
        .pull(&root(), &mut |event| phases.push(event.phase))
        .unwrap();
    assert_eq!(graph.images.len(), 2);
    assert_eq!(db.artifacts().unwrap().len(), 2);
    assert!(matches!(phases.last(), Some(PullPhase::Installed)));
    installer.pull(&root(), &mut |_| {}).unwrap();
    assert_eq!(registry.downloads.load(Ordering::SeqCst), 2);
}

#[test]
fn corrupt_child_does_not_publish_any_image_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path().join("store")).unwrap();
    let db = StateDb::open(dir.path().join("state.db")).unwrap();
    let mut child = package("child", &[]);
    child.1 = b"corrupt".to_vec();
    let registry = registry(vec![package("root", &["child"]), child]);
    let result = Installer {
        registry: &registry,
        store: &store,
        state: &db,
        limits: InstallLimits::default(),
    }
    .pull(&root(), &mut |_| {});
    assert!(matches!(result, Err(EngineError::DigestMismatch { .. })));
    assert!(db.artifacts().unwrap().is_empty());
}

#[test]
fn cycles_and_package_limits_fail_before_publishing_metadata() {
    for (packages, limit) in [
        (
            vec![package("root", &["child"]), package("child", &["root"])],
            32,
        ),
        (vec![package("root", &["child"]), package("child", &[])], 1),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = ContentStore::open(dir.path().join("store")).unwrap();
        let db = StateDb::open(dir.path().join("state.db")).unwrap();
        let registry = registry(packages);
        let result = Installer {
            registry: &registry,
            store: &store,
            state: &db,
            limits: InstallLimits {
                max_packages: limit,
                ..Default::default()
            },
        }
        .pull(&root(), &mut |_| {});
        assert!(result.is_err());
        assert!(db.artifacts().unwrap().is_empty());
        assert_eq!(registry.downloads.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn diamond_dependency_is_downloaded_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path().join("store")).unwrap();
    let db = StateDb::open(dir.path().join("state.db")).unwrap();
    let registry = registry(vec![
        package("root", &["left", "right"]),
        package("left", &["shared"]),
        package("right", &["shared"]),
        package("shared", &[]),
    ]);
    let graph = Installer {
        registry: &registry,
        store: &store,
        state: &db,
        limits: InstallLimits::default(),
    }
    .pull(&root(), &mut |_| {})
    .unwrap();
    assert_eq!(graph.images.len(), 4);
    assert_eq!(registry.downloads.load(Ordering::SeqCst), 4);
}

#[test]
fn immutable_digest_conflict_rolls_back_entire_metadata_batch() {
    let dir = tempfile::tempdir().unwrap();
    let db = StateDb::open(dir.path().join("state.db")).unwrap();
    let old = package("root", &[]).0.artifact;
    db.put_artifact(&old).unwrap();
    let mut replacement = old.clone();
    replacement.digest = Digest::of(b"different");
    let child = package("child", &[]).0.artifact;
    assert!(matches!(
        db.put_artifacts(&[child, replacement]),
        Err(EngineError::Conflict(_))
    ));
    let saved = db.artifacts().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].digest, old.digest);
}

#[test]
fn corrupt_cached_blob_is_redownloaded_and_reverified() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path().join("store")).unwrap();
    let db = StateDb::open(dir.path().join("state.db")).unwrap();
    let root_package = package("root", &[]);
    let digest = root_package.0.artifact.digest.clone();
    let size = root_package.0.artifact.size_bytes;
    let registry = registry(vec![root_package]);
    let installer = Installer {
        registry: &registry,
        store: &store,
        state: &db,
        limits: InstallLimits::default(),
    };
    installer.pull(&root(), &mut |_| {}).unwrap();
    std::fs::write(store.path(&digest), b"changed after installation").unwrap();
    installer.pull(&root(), &mut |_| {}).unwrap();
    assert_eq!(store.verify(&digest, size).unwrap(), size);
    assert_eq!(registry.downloads.load(Ordering::SeqCst), 2);
}

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
    resolves: AtomicUsize,
}
impl Registry for FixtureRegistry {
    fn search(&self, _: &str, _: Option<&str>, _: u32) -> Result<SearchPage> {
        unreachable!()
    }
    fn resolve(&self, reference: &ArtifactReference) -> Result<ResolvedArtifact> {
        self.resolves.fetch_add(1, Ordering::SeqCst);
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
        resolves: AtomicUsize::new(0),
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

#[test]
fn controlled_cancel_before_resolution_and_at_last_verification_never_publishes() {
    use std::sync::Arc;
    use wasmdroid_engine::{
        operation::{OperationControl, OperationState},
        store::InstallBudget,
    };
    for before in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let store = ContentStore::open(dir.path().join("store")).unwrap();
        let db = StateDb::open(dir.path().join("db")).unwrap();
        let registry = registry(vec![package("root", &[])]);
        let control = Arc::new(OperationControl::default());
        if before {
            assert!(control.cancel());
        }
        let result = Installer {
            registry: &registry,
            store: &store,
            state: &db,
            limits: InstallLimits::default(),
        }
        .pull_controlled(
            &root(),
            &control,
            InstallBudget {
                max_new_bytes: 4096,
                max_store_bytes: Some(4096),
            },
            &mut |event| {
                if matches!(event.phase, PullPhase::Verifying) {
                    assert!(control.cancel());
                }
            },
        );
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert_eq!(control.state(), OperationState::Cancelled);
        assert!(db.artifacts().unwrap().is_empty());
        if before {
            assert_eq!(registry.resolves.load(Ordering::SeqCst), 0);
            assert_eq!(registry.downloads.load(Ordering::SeqCst), 0);
        } else {
            let artifact = &registry.packages[root().as_str()].0.artifact;
            assert_eq!(
                store.verify(&artifact.digest, 4096).unwrap(),
                artifact.size_bytes
            );
        }
    }
}
#[test]
fn installed_callback_cannot_cancel_committed_graph_or_reuse_control() {
    use std::sync::Arc;
    use wasmdroid_engine::{
        operation::{OperationControl, OperationState},
        store::InstallBudget,
    };
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path().join("store")).unwrap();
    let db = StateDb::open(dir.path().join("db")).unwrap();
    let registry = registry(vec![package("root", &["child"]), package("child", &[])]);
    let installer = Installer {
        registry: &registry,
        store: &store,
        state: &db,
        limits: InstallLimits::default(),
    };
    let control = Arc::new(OperationControl::default());
    let budget = InstallBudget {
        max_new_bytes: 4096,
        max_store_bytes: Some(4096),
    };
    installer
        .pull_controlled(&root(), &control, budget, &mut |event| {
            if matches!(event.phase, PullPhase::Installed) {
                assert!(!control.cancel());
            }
        })
        .unwrap();
    assert_eq!(db.artifacts().unwrap().len(), 2);
    assert_eq!(control.state(), OperationState::Committed);
    let resolves = registry.resolves.load(Ordering::SeqCst);
    assert!(matches!(
        installer.pull_controlled(&root(), &control, budget, &mut |_| {}),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(registry.resolves.load(Ordering::SeqCst), resolves);
}
#[test]
fn write_budget_rejects_before_download_but_zero_budget_reuses_verified_cache() {
    use std::sync::Arc;
    use wasmdroid_engine::{operation::OperationControl, store::InstallBudget};
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path().join("store")).unwrap();
    let db = StateDb::open(dir.path().join("db")).unwrap();
    let registry = registry(vec![package("root", &[])]);
    let installer = Installer {
        registry: &registry,
        store: &store,
        state: &db,
        limits: InstallLimits::default(),
    };
    let budget = InstallBudget {
        max_new_bytes: 0,
        max_store_bytes: Some(0),
    };
    assert!(
        installer
            .pull_controlled(
                &root(),
                &Arc::new(OperationControl::default()),
                budget,
                &mut |_| {}
            )
            .is_err()
    );
    assert_eq!(registry.downloads.load(Ordering::SeqCst), 0);
    assert!(db.artifacts().unwrap().is_empty());
    let (resolved, bytes) = &registry.packages[root().as_str()];
    store
        .ingest(
            Cursor::new(bytes),
            &resolved.artifact.digest,
            bytes.len() as u64,
        )
        .unwrap();
    installer
        .pull_controlled(
            &root(),
            &Arc::new(OperationControl::default()),
            budget,
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(registry.downloads.load(Ordering::SeqCst), 0);
    assert_eq!(db.artifacts().unwrap().len(), 1);
}
#[test]
fn corrupt_replacement_counts_staging_peak_and_preserves_unrelated_files() {
    use std::sync::Arc;
    use wasmdroid_engine::{operation::OperationControl, store::InstallBudget};
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path().join("store")).unwrap();
    let db = StateDb::open(dir.path().join("db")).unwrap();
    let registry = registry(vec![package("root", &[])]);
    let (resolved, bytes) = &registry.packages[root().as_str()];
    let length = bytes.len() as u64;
    std::fs::write(
        store.path(&resolved.artifact.digest),
        vec![b'x'; bytes.len()],
    )
    .unwrap();
    let unrelated = store
        .path(&resolved.artifact.digest)
        .parent()
        .unwrap()
        .join("preserve-unknown");
    std::fs::write(&unrelated, b"keep").unwrap();
    let installer = Installer {
        registry: &registry,
        store: &store,
        state: &db,
        limits: InstallLimits::default(),
    };
    assert!(
        installer
            .pull_controlled(
                &root(),
                &Arc::new(OperationControl::default()),
                InstallBudget {
                    max_new_bytes: length,
                    max_store_bytes: Some(length * 2 + 3)
                },
                &mut |_| {}
            )
            .is_err()
    );
    assert_eq!(registry.downloads.load(Ordering::SeqCst), 0);
    assert!(db.artifacts().unwrap().is_empty());
    assert_eq!(
        std::fs::read(store.path(&resolved.artifact.digest)).unwrap(),
        vec![b'x'; bytes.len()]
    );
    installer
        .pull_controlled(
            &root(),
            &Arc::new(OperationControl::default()),
            InstallBudget {
                max_new_bytes: length,
                max_store_bytes: Some(length * 2 + 4),
            },
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(
        store.verify(&resolved.artifact.digest, length).unwrap(),
        length
    );
    assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");
}
#[test]
fn all_ingest_paths_share_writer_admission_while_runtime_reads_remain_available() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path()).unwrap();
    let bytes = b"existing";
    let digest = Digest::of(bytes);
    store
        .ingest(Cursor::new(bytes), &digest, bytes.len() as u64)
        .unwrap();
    let writer = store.writer().unwrap();
    assert!(matches!(store.writer(), Err(EngineError::Conflict(_))));
    assert!(matches!(
        store.ingest(Cursor::new(bytes), &digest, bytes.len() as u64),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(
        store.verify(&digest, bytes.len() as u64).unwrap(),
        bytes.len() as u64
    );
    drop(writer);
    store
        .ingest(Cursor::new(bytes), &digest, bytes.len() as u64)
        .unwrap();
}
#[test]
fn cancelling_a_staging_read_removes_only_its_temporary_file() {
    use wasmdroid_engine::{operation::OperationControl, store::InstallBudget};
    struct CancelRead<'a> {
        control: &'a OperationControl,
    }
    impl Read for CancelRead<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            buffer[..4].copy_from_slice(b"new!");
            self.control.cancel();
            Ok(4)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path()).unwrap();
    let existing = Digest::of(b"keep");
    store.ingest(Cursor::new(b"keep"), &existing, 4).unwrap();
    let control = OperationControl::default();
    let mut writer = store.writer().unwrap();
    assert!(matches!(
        store.ingest_controlled(
            CancelRead { control: &control },
            &Digest::of(b"new!"),
            4,
            &mut writer,
            InstallBudget {
                max_new_bytes: 4,
                max_store_bytes: Some(8)
            },
            &control
        ),
        Err(EngineError::Cancelled)
    ));
    assert_eq!(std::fs::read(store.path(&existing)).unwrap(), b"keep");
    assert_eq!(
        std::fs::read_dir(store.path(&existing).parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

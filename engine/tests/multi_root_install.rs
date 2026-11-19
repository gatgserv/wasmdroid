use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    sync::{Arc, Mutex},
};
use wasmdroid_engine::{
    EngineError, Result,
    api::{Artifact, ArtifactKind, ArtifactReference},
    installer::{DEPENDENCIES_LABEL, InstallLimits, Installer, PullPhase},
    operation::{OperationControl, OperationState},
    registry::{PackageRequirement, Registry, ResolvedArtifact, SearchPage},
    state::StateDb,
    store::{ContentStore, Digest, InstallBudget},
};

struct FakeRegistry {
    packages: BTreeMap<String, (ResolvedArtifact, Vec<u8>)>,
    dependency_overrides: BTreeMap<String, ResolvedArtifact>,
    calls: Mutex<Vec<String>>,
}
impl Registry for FakeRegistry {
    fn search(&self, _: &str, _: Option<&str>, _: u32) -> Result<SearchPage> {
        unreachable!("installation does not search")
    }
    fn resolve(&self, reference: &ArtifactReference) -> Result<ResolvedArtifact> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("resolve:{}", reference.as_str()));
        self.packages
            .get(reference.as_str())
            .map(|(artifact, _)| artifact.clone())
            .ok_or_else(|| EngineError::NotFound("fixture package".into()))
    }
    fn resolve_requirement(&self, requirement: &PackageRequirement) -> Result<ResolvedArtifact> {
        if let Some(resolved) = self
            .dependency_overrides
            .get(requirement.reference.as_str())
        {
            return Ok(resolved.clone());
        }
        self.resolve(&requirement.reference)
    }
    fn download(&self, artifact: &ResolvedArtifact) -> Result<Box<dyn Read + Send>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("download:{}", artifact.artifact.reference.as_str()));
        Ok(Box::new(Cursor::new(
            self.packages[artifact.artifact.reference.as_str()]
                .1
                .clone(),
        )))
    }
}
fn reference(name: &str) -> ArtifactReference {
    ArtifactReference::parse(format!("wasmer.io/example/{name}@1.0.0")).unwrap()
}
fn package(name: &str, dependencies: &[&str]) -> (ResolvedArtifact, Vec<u8>) {
    let bytes = format!("verified bytes for package {name}").into_bytes();
    (
        ResolvedArtifact {
            artifact: Artifact {
                reference: reference(name),
                digest: Digest::of(&bytes),
                kind: ArtifactKind::Webc,
                driver: "wasmer-local".into(),
                platform: "wasm32-wasix".into(),
                commands: vec!["run".into()],
                size_bytes: bytes.len() as u64,
                labels: BTreeMap::new(),
            },
            download_url: "https://example.invalid/fixture".into(),
            dependencies: dependencies
                .iter()
                .map(|name| PackageRequirement {
                    reference: reference(name),
                    version_requirement: "=1.0.0".into(),
                })
                .collect(),
        },
        bytes,
    )
}
struct Fixture {
    dir: tempfile::TempDir,
    store: ContentStore,
    db: StateDb,
    registry: FakeRegistry,
}
impl Fixture {
    fn new(packages: Vec<(ResolvedArtifact, Vec<u8>)>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = ContentStore::open(dir.path().join("store")).unwrap();
        let db = StateDb::open(dir.path().join("state.db")).unwrap();
        Self {
            dir,
            store,
            db,
            registry: FakeRegistry {
                packages: packages
                    .into_iter()
                    .map(|p| (p.0.artifact.reference.as_str().to_owned(), p))
                    .collect(),
                dependency_overrides: BTreeMap::new(),
                calls: Mutex::new(Vec::new()),
            },
        }
    }
    fn installer(&self) -> Installer<'_> {
        Installer {
            registry: &self.registry,
            store: &self.store,
            state: &self.db,
            limits: InstallLimits::default(),
        }
    }
    fn roots(&self, names: &[&str]) -> Vec<(ArtifactReference, Digest)> {
        names
            .iter()
            .map(|name| {
                let artifact = &self.registry.packages[reference(name).as_str()].0.artifact;
                (artifact.reference.clone(), artifact.digest.clone())
            })
            .collect()
    }
    fn downloads(&self) -> usize {
        self.registry
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("download:"))
            .count()
    }
    fn assert_cached(&self, name: &str) {
        let artifact = &self.registry.packages[reference(name).as_str()].0.artifact;
        assert_eq!(
            self.store
                .verify(&artifact.digest, artifact.size_bytes)
                .unwrap(),
            artifact.size_bytes
        );
    }
}
fn budget() -> InstallBudget {
    InstallBudget {
        max_new_bytes: 4096,
        max_store_bytes: Some(4096),
    }
}

#[test]
fn later_root_digest_or_resolution_failure_downloads_nothing() {
    for missing in [false, true] {
        let mut f = Fixture::new(vec![package("alpha", &[]), package("beta", &[])]);
        let mut roots = f.roots(&["alpha", "beta"]);
        if missing {
            f.registry.packages.remove(reference("beta").as_str());
        } else {
            roots[1].1 = Digest::of(b"different expected pin");
        }
        let control = Arc::new(OperationControl::default());
        let mut operation = control.claim().unwrap();
        let result = f.installer().pull_many_expected_in_operation(
            &roots,
            &mut operation,
            budget(),
            &mut |_| {},
        );
        assert!(if missing {
            matches!(result, Err(EngineError::NotFound(_)))
        } else {
            matches!(result, Err(EngineError::DigestMismatch { .. }))
        });
        drop(operation);
        assert_eq!(control.state(), OperationState::Failed);
        assert_eq!(f.registry.calls.lock().unwrap().len(), 2);
        assert_eq!(f.downloads(), 0);
        assert!(f.db.artifacts().unwrap().is_empty());
    }
}

#[test]
fn shared_dependencies_publish_once_and_results_preserve_request_order() {
    let f = Fixture::new(vec![
        package("alpha", &["shared"]),
        package("beta", &["shared"]),
        package("shared", &[]),
    ]);
    let roots = f.roots(&["beta", "alpha"]);
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    let mut installed = Vec::new();
    let union_bytes = f
        .registry
        .packages
        .values()
        .map(|(resolved, _)| resolved.artifact.size_bytes)
        .sum();
    let graphs = f
        .installer()
        .pull_many_expected_in_operation(
            &roots,
            &mut operation,
            InstallBudget {
                max_new_bytes: union_bytes,
                max_store_bytes: Some(union_bytes),
            },
            &mut |event| {
                if matches!(event.phase, PullPhase::Installed) {
                    assert_eq!(control.state(), OperationState::Committed);
                    assert!(!control.cancel());
                    assert_eq!(f.db.artifacts().unwrap().len(), 3);
                    installed.push(event.reference);
                } else {
                    assert!(f.db.artifacts().unwrap().is_empty());
                }
            },
        )
        .unwrap();
    assert_eq!(graphs.len(), 2);
    assert_eq!(graphs[0].root.reference, reference("beta"));
    assert_eq!(graphs[1].root.reference, reference("alpha"));
    assert_eq!(installed, vec![reference("beta"), reference("alpha")]);
    for graph in &graphs {
        assert_eq!(graph.images.len(), 3);
        let deps: Vec<ArtifactReference> =
            serde_json::from_str(&graph.root.labels[DEPENDENCIES_LABEL]).unwrap();
        assert_eq!(deps, vec![reference("shared")]);
    }
    assert_eq!(f.downloads(), 3);
    let calls = f.registry.calls.lock().unwrap();
    assert_eq!(calls[0], format!("resolve:{}", reference("beta").as_str()));
    assert_eq!(calls[1], format!("resolve:{}", reference("alpha").as_str()));
    drop(calls);
    drop(operation);
    assert_eq!(control.state(), OperationState::Committed);

    // The union fits only because shared bytes were charged once. A verified
    // retry can publish metadata with no additional writes, even below usage.
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    f.installer()
        .pull_many_expected_in_operation(
            &roots,
            &mut operation,
            InstallBudget {
                max_new_bytes: 0,
                max_store_bytes: Some(0),
            },
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(f.downloads(), 3);
}

#[test]
fn new_byte_and_total_store_budgets_accumulate_across_roots() {
    for total_store in [false, true] {
        let f = Fixture::new(vec![package("alpha", &[]), package("beta", &[])]);
        let roots = f.roots(&["alpha", "beta"]);
        let alpha_bytes = f.registry.packages[reference("alpha").as_str()]
            .0
            .artifact
            .size_bytes;
        let beta_bytes = f.registry.packages[reference("beta").as_str()]
            .0
            .artifact
            .size_bytes;
        let cap = alpha_bytes + beta_bytes - 1;
        let selected = InstallBudget {
            max_new_bytes: if total_store { 4096 } else { cap },
            max_store_bytes: Some(if total_store { cap } else { 4096 }),
        };
        let control = Arc::new(OperationControl::default());
        let mut operation = control.claim().unwrap();
        assert!(
            f.installer()
                .pull_many_expected_in_operation(&roots, &mut operation, selected, &mut |_| {})
                .is_err()
        );
        drop(operation);
        assert_eq!(control.state(), OperationState::Failed);
        assert_eq!(f.downloads(), 1);
        assert!(f.db.artifacts().unwrap().is_empty());
        f.assert_cached("alpha");
        assert!(!f.store.path(&roots[1].1).exists());
    }
}

#[test]
fn graph_byte_and_package_limits_apply_to_the_union() {
    for package_limit in [false, true] {
        let f = Fixture::new(vec![package("alpha", &[]), package("beta", &[])]);
        let roots = f.roots(&["alpha", "beta"]);
        let mut installer = f.installer();
        if package_limit {
            installer.limits.max_packages = 1;
        } else {
            installer.limits.max_graph_bytes = f.registry.packages[reference("alpha").as_str()]
                .0
                .artifact
                .size_bytes;
        }
        let control = Arc::new(OperationControl::default());
        let mut operation = control.claim().unwrap();
        assert!(matches!(
            installer.pull_many_expected_in_operation(
                &roots,
                &mut operation,
                budget(),
                &mut |_| {}
            ),
            Err(EngineError::Invalid(_))
        ));
        assert_eq!(f.downloads(), 1);
        assert!(f.db.artifacts().unwrap().is_empty());
        f.assert_cached("alpha");
    }
}

#[test]
fn cancellation_after_first_root_preserves_cas_without_any_metadata() {
    let f = Fixture::new(vec![package("alpha", &[]), package("beta", &[])]);
    let roots = f.roots(&["alpha", "beta"]);
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    let mut installed = 0;
    let result = f.installer().pull_many_expected_in_operation(
        &roots,
        &mut operation,
        budget(),
        &mut |event| {
            if event.reference == reference("alpha") && matches!(event.phase, PullPhase::Verifying)
            {
                assert!(control.cancel());
            }
            if matches!(event.phase, PullPhase::Installed) {
                installed += 1;
            }
        },
    );
    assert!(matches!(result, Err(EngineError::Cancelled)));
    drop(operation);
    assert_eq!(control.state(), OperationState::Cancelled);
    assert_eq!(installed, 0);
    assert_eq!(f.downloads(), 1);
    assert!(f.db.artifacts().unwrap().is_empty());
    f.assert_cached("alpha");
    assert!(!f.store.path(&roots[1].1).exists());
    assert!(control.claim().is_err());
}

#[test]
fn sql_failure_on_later_image_rolls_back_metadata_and_preserves_verified_blobs() {
    let f = Fixture::new(vec![package("alpha", &[]), package("beta", &[])]);
    let sentinel = package("existing", &[]).0.artifact;
    f.db.put_artifact(&sentinel).unwrap();
    let connection = rusqlite::Connection::open(f.dir.path().join("state.db")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_beta BEFORE INSERT ON artifacts
             WHEN NEW.reference='wasmer.io/example/beta@1.0.0'
             BEGIN SELECT RAISE(ABORT,'fixture publication failure'); END;",
        )
        .unwrap();
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    let mut installed = 0;
    assert!(
        f.installer()
            .pull_many_expected_in_operation(
                &f.roots(&["alpha", "beta"]),
                &mut operation,
                budget(),
                &mut |event| {
                    if matches!(event.phase, PullPhase::Installed) {
                        installed += 1;
                    }
                },
            )
            .is_err()
    );
    drop(operation);
    assert_eq!(control.state(), OperationState::Failed);
    assert_eq!(installed, 0);
    assert_eq!(f.downloads(), 2);
    let saved = f.db.artifacts().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].reference, sentinel.reference);
    f.assert_cached("alpha");
    f.assert_cached("beta");
    assert!(control.claim().is_err());
}

#[test]
fn empty_oversized_duplicate_and_conflicting_requests_do_no_registry_work() {
    let f = Fixture::new(vec![package("alpha", &[])]);
    let root = f.roots(&["alpha"])[0].clone();
    let conflicting = (root.0.clone(), Digest::of(b"conflicting pin"));
    for roots in [
        vec![],
        vec![root.clone(); 9],
        vec![root.clone(), root.clone()],
        vec![root.clone(), conflicting],
    ] {
        let control = Arc::new(OperationControl::default());
        let mut operation = control.claim().unwrap();
        assert!(
            f.installer()
                .pull_many_expected_in_operation(&roots, &mut operation, budget(), &mut |_| {})
                .is_err()
        );
        drop(operation);
        assert_eq!(control.state(), OperationState::Failed);
        assert!(control.claim().is_err());
    }
    assert!(f.registry.calls.lock().unwrap().is_empty());
    assert!(f.db.artifacts().unwrap().is_empty());
}

#[test]
fn canonical_root_pin_conflicts_and_dependency_drift_fail_before_download() {
    for dependency in [false, true] {
        let mut f = Fixture::new(vec![package("alpha", &["beta"]), package("beta", &[])]);
        let mut changed = f.registry.packages[reference("beta").as_str()].0.clone();
        changed.artifact.digest = Digest::of(b"changed canonical beta");
        let mut roots = f.roots(&["alpha", "beta"]);
        if dependency {
            f.registry
                .dependency_overrides
                .insert(reference("beta").as_str().into(), changed);
        } else {
            let alias = reference("alias");
            roots[0] = (alias.clone(), changed.artifact.digest.clone());
            f.registry
                .packages
                .insert(alias.as_str().into(), (changed, vec![]));
        }
        let control = Arc::new(OperationControl::default());
        let mut operation = control.claim().unwrap();
        assert!(matches!(
            f.installer().pull_many_expected_in_operation(
                &roots,
                &mut operation,
                budget(),
                &mut |_| {}
            ),
            Err(EngineError::Conflict(_))
        ));
        assert_eq!(f.downloads(), 0);
        assert!(f.db.artifacts().unwrap().is_empty());
    }
}

#[test]
fn different_requested_aliases_cannot_repeat_one_canonical_root() {
    let mut f = Fixture::new(vec![package("beta", &[])]);
    let mut roots = f.roots(&["beta"]);
    let alias = reference("alias");
    let canonical = f.registry.packages[reference("beta").as_str()].clone();
    roots.push((alias.clone(), canonical.0.artifact.digest.clone()));
    f.registry.packages.insert(alias.as_str().into(), canonical);
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    assert!(matches!(
        f.installer().pull_many_expected_in_operation(
            &roots,
            &mut operation,
            budget(),
            &mut |_| {}
        ),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(f.downloads(), 0);
    assert!(f.db.artifacts().unwrap().is_empty());
}

#[test]
fn later_root_reference_substitution_with_the_same_digest_downloads_nothing() {
    let mut f = Fixture::new(vec![package("alpha", &[]), package("beta", &[])]);
    let roots = f.roots(&["alpha", "beta"]);
    // A digest match grants no permission to publish another endpoint/reference.
    f.registry
        .packages
        .get_mut(reference("beta").as_str())
        .unwrap()
        .0
        .artifact
        .reference = reference("substituted");
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    assert!(matches!(
        f.installer().pull_many_expected_in_operation(
            &roots,
            &mut operation,
            budget(),
            &mut |_| {}
        ),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(f.registry.calls.lock().unwrap().len(), 2);
    assert_eq!(f.downloads(), 0);
    assert!(f.db.artifacts().unwrap().is_empty());
}

#[test]
fn committed_guard_cannot_start_another_multi_or_single_pull() {
    let f = Fixture::new(vec![package("alpha", &[])]);
    let roots = f.roots(&["alpha"]);
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    f.installer()
        .pull_many_expected_in_operation(&roots, &mut operation, budget(), &mut |_| {})
        .unwrap();
    let calls = f.registry.calls.lock().unwrap().len();
    assert!(matches!(
        f.installer().pull_many_expected_in_operation(
            &roots,
            &mut operation,
            budget(),
            &mut |_| {}
        ),
        Err(EngineError::Conflict(_))
    ));
    assert!(matches!(
        f.installer()
            .pull_in_operation(&roots[0].0, None, &mut operation, budget(), &mut |_| {}),
        Err(EngineError::Conflict(_))
    ));
    assert!(control.claim().is_err());
    assert_eq!(f.registry.calls.lock().unwrap().len(), calls);
    assert_eq!(control.state(), OperationState::Committed);
}

#[test]
fn cancellation_before_resolution_refuses_work_with_the_claimed_guard() {
    let f = Fixture::new(vec![package("alpha", &[])]);
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    assert!(control.cancel());
    assert!(matches!(
        f.installer().pull_many_expected_in_operation(
            &f.roots(&["alpha"]),
            &mut operation,
            budget(),
            &mut |_| {}
        ),
        Err(EngineError::Cancelled)
    ));
    assert!(f.registry.calls.lock().unwrap().is_empty());
    assert!(f.db.artifacts().unwrap().is_empty());
}

#[test]
fn maximum_eight_roots_succeed_and_legacy_single_pull_still_returns_its_graph() {
    let names = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    ];
    let f = Fixture::new(names.iter().map(|name| package(name, &[])).collect());
    let roots = f.roots(&names);
    let control = Arc::new(OperationControl::default());
    let mut operation = control.claim().unwrap();
    let graphs = f
        .installer()
        .pull_many_expected_in_operation(&roots, &mut operation, budget(), &mut |_| {})
        .unwrap();
    assert_eq!(graphs.len(), 8);
    assert_eq!(f.downloads(), 8);
    assert_eq!(f.db.artifacts().unwrap().len(), 8);
    for (graph, (reference, digest)) in graphs.iter().zip(&roots) {
        assert_eq!(&graph.root.reference, reference);
        assert_eq!(&graph.root.digest, digest);
        assert_eq!(graph.images.len(), 8);
    }
    let single = f.installer().pull(&roots[0].0, &mut |_| {}).unwrap();
    assert_eq!(single.images.len(), 1);
    assert_eq!(single.root.reference, roots[0].0);
    assert_eq!(f.downloads(), 8);
}

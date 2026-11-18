//! Neutral transaction tests use a trusted fake inspector, never claim format
//! validation or guest execution. Driver/device fixtures cover those separately.
use std::{collections::BTreeMap, fs::File, sync::Arc};
use wasmdroid_engine::{
    EngineError, Result,
    api::{Artifact, ArtifactKind, ArtifactReference},
    installer::DEPENDENCIES_LABEL,
    local_import::{Dependency, Inspection, Inspector, LocalImport, graph},
    operation::{OperationControl, OperationState},
    session::EngineSession,
    store::{ContentStore, Digest, InstallBudget},
};
struct Fixture(Vec<Dependency>);
impl Inspector for Fixture {
    fn inspect(&self, _: &File, _: &OperationControl) -> Result<Inspection> {
        Ok(Inspection {
            kind: if self.0.is_empty() {
                ArtifactKind::Wasm
            } else {
                ArtifactKind::Webc
            },
            commands: vec!["main".into()],
            entrypoint: Some("main".into()),
            dependencies: self.0.clone(),
        })
    }
}
fn begin(session: &EngineSession, control: Arc<OperationControl>, budget: u64) -> LocalImport {
    LocalImport::begin(
        session.connection().unwrap(),
        InstallBudget {
            max_new_bytes: budget,
            max_store_bytes: None,
        },
        Some(5),
        control,
    )
    .unwrap()
}
fn dependency(session: &EngineSession) -> Artifact {
    let artifact = Artifact {
        reference: ArtifactReference::parse("wasmer.io/example/dependency@1.2.3").unwrap(),
        digest: Digest::of(b"dependency"),
        kind: ArtifactKind::Webc,
        driver: "wasmer-local".into(),
        platform: "wasm32-wasix".into(),
        commands: vec!["run".into()],
        size_bytes: 10,
        labels: BTreeMap::new(),
    };
    session.state().put_artifact(&artifact).unwrap();
    artifact
}
fn requirement() -> Dependency {
    Dependency::Named {
        reference: ArtifactReference::parse("wasmer.io/example/dependency").unwrap(),
        version_requirement: "^1.0".into(),
    }
}
#[test]
fn preview_is_sealed_and_commit_has_no_instance_or_dataset_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let control = Arc::new(OperationControl::default());
    let mut import = begin(&session, control.clone(), 10);
    import.append(b"bytes").unwrap();
    let preview = import.preview(&Fixture(vec![])).unwrap();
    assert!(
        preview
            .artifact
            .reference
            .as_str()
            .starts_with("local.wasmdroid/sha256/")
    );
    assert_eq!(preview.artifact.digest, Digest::of(b"bytes"));
    let token = preview.token.as_str().to_owned();
    assert!(session.state().artifacts().unwrap().is_empty());
    assert!(import.append(b"x").is_err());
    assert!(import.commit("unreviewed").is_err());
    let artifact = import.commit(&token).unwrap();
    assert_eq!(control.state(), OperationState::Committed);
    assert!(!control.cancel());
    assert!(session.state().instances("local").unwrap().is_empty());
    assert!(session.state().volumes().unwrap().is_empty());
    assert_eq!(graph(session.state(), &artifact).unwrap().len(), 1);
    assert_eq!(
        ContentStore::open(dir.path().join("store"))
            .unwrap()
            .read_verified(&artifact.digest, 5)
            .unwrap(),
        b"bytes"
    );
    assert!(import.commit(&token).is_err());
}
#[test]
fn budget_includes_input_and_cas_copy_and_verified_cas_can_be_reused() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let mut short = begin(&session, Default::default(), 9);
    short.append(b"bytes").unwrap();
    let token = short
        .preview(&Fixture(vec![]))
        .unwrap()
        .token
        .as_str()
        .to_owned();
    assert!(short.commit(&token).is_err());
    assert!(session.state().artifacts().unwrap().is_empty());
    short.abort();
    let mut first = begin(&session, Default::default(), 10);
    first.append(b"bytes").unwrap();
    let token = first
        .preview(&Fixture(vec![]))
        .unwrap()
        .token
        .as_str()
        .to_owned();
    first.commit(&token).unwrap();
    let mut cached = begin(&session, Default::default(), 5);
    cached.append(b"bytes").unwrap();
    let token = cached
        .preview(&Fixture(vec![]))
        .unwrap()
        .token
        .as_str()
        .to_owned();
    cached.commit(&token).unwrap();
    assert_eq!(session.state().artifacts().unwrap().len(), 1);
}
#[test]
fn missing_dependencies_require_new_review_and_changed_pins_prevent_publication() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let mut import = begin(&session, Default::default(), 10);
    import.append(b"bytes").unwrap();
    let fixture = Fixture(vec![requirement()]);
    let old = import.preview(&fixture).unwrap().token.as_str().to_owned();
    assert_eq!(
        import.preview(&fixture).unwrap().missing,
        vec![requirement()]
    );
    assert!(matches!(import.commit(&old), Err(EngineError::NotFound(_))));
    let mut dep = dependency(&session);
    let token = import.preview(&fixture).unwrap().token.as_str().to_owned();
    assert_ne!(token, old);
    assert!(import.commit(&old).is_err());
    dep.labels.insert("changed".into(), "metadata".into());
    session.state().put_artifact(&dep).unwrap();
    assert!(matches!(
        import.commit(&token),
        Err(EngineError::Conflict(_))
    ));
    let token = import.preview(&fixture).unwrap().token.as_str().to_owned();
    let artifact = import.commit(&token).unwrap();
    let references: Vec<ArtifactReference> =
        serde_json::from_str(&artifact.labels[DEPENDENCIES_LABEL]).unwrap();
    assert_eq!(references, vec![dep.reference]);
    assert_eq!(graph(session.state(), &artifact).unwrap().len(), 2);
}
#[test]
fn control_and_owner_survive_facade_loss_until_explicit_abort() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let control = Arc::new(OperationControl::default());
    let mut import = begin(&session, control.clone(), 10);
    drop(session);
    assert!(EngineSession::open(dir.path()).is_err());
    assert!(control.cancel());
    assert!(matches!(
        import.append(b"bytes"),
        Err(EngineError::Cancelled)
    ));
    import.abort();
    let session = EngineSession::open(dir.path()).unwrap();
    assert!(session.state().artifacts().unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(dir.path().join("imports"))
            .unwrap()
            .count(),
        0
    );
    assert!(
        LocalImport::begin(
            session.connection().unwrap(),
            InstallBudget {
                max_new_bytes: 10,
                max_store_bytes: None
            },
            Some(5),
            control
        )
        .is_err()
    );
}
#[test]
fn exact_length_and_chunk_bounds_fail_before_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let mut import = begin(&session, Default::default(), 10);
    assert!(import.append(&vec![0; 65_537]).is_err());
    import.append(b"byte").unwrap();
    assert!(import.preview(&Fixture(vec![])).is_err());
    assert!(import.append(b"ss").is_err());
    import.append(b"s").unwrap();
    assert!(import.preview(&Fixture(vec![])).is_ok());
}

#[test]
fn abandoned_staging_recovery_requires_a_fresh_exclusive_owner() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let mut import = begin(&session, Default::default(), 10);
    let root = dir.path().join("imports");
    std::fs::write(root.join("wasmdroid-import-abandoned"), b"private input").unwrap();
    std::fs::write(root.join("keep-unknown"), b"other data").unwrap();
    drop(session.connection().unwrap());
    assert!(root.join("wasmdroid-import-abandoned").is_file());
    import.abort();
    drop(import);
    drop(session);
    let _new_owner = EngineSession::open(dir.path()).unwrap();
    assert!(!root.join("wasmdroid-import-abandoned").exists());
    assert!(root.join("keep-unknown").is_file());
}

#[test]
fn dropping_a_failed_capability_releases_staging_and_marks_failure_without_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let control = Arc::new(OperationControl::default());
    let mut import = begin(&session, control.clone(), 10);
    import.append(b"bytes").unwrap();
    drop(session);
    assert!(EngineSession::open(dir.path()).is_err());
    drop(import);
    assert_eq!(control.state(), OperationState::Failed);
    let owner = EngineSession::open(dir.path()).unwrap();
    assert!(owner.state().artifacts().unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(dir.path().join("imports"))
            .unwrap()
            .count(),
        0
    );
}

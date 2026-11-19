//! Installed metadata/definition regressions only; no executable fixture or guest.
use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference},
    local_import::{self, DependencyPin, ORIGIN_LABEL, PINS_LABEL},
    session::EngineSession,
    stacks::StackManager,
    store::Digest,
};
fn imported(session: &EngineSession, kind: ArtifactKind) -> Artifact {
    let digest = Digest::of(format!("imported metadata fixture {kind:?}").as_bytes());
    let artifact = Artifact {
        reference: local_import::reference(&digest).unwrap(),
        digest,
        kind,
        driver: "wasmer-local".into(),
        platform: "wasm32-wasix".into(),
        commands: vec!["main".into()],
        size_bytes: 0,
        labels: BTreeMap::from([
            (ORIGIN_LABEL.into(), "local_import".into()),
            (PINS_LABEL.into(), "[]".into()),
            ("dev.wasmdroid.entrypoint".into(), "main".into()),
        ]),
    };
    session.state().put_artifact(&artifact).unwrap();
    artifact
}
fn document(reference: &ArtifactReference) -> String {
    serde_json::json!({"services":{"app":{"image":reference,"volumes":["data:/data"]}},"volumes":{"data":{}}}).to_string()
}
#[test]
fn imported_webc_and_wasm_use_common_group_metadata_and_lossless_update_export() {
    for kind in [ArtifactKind::Webc, ArtifactKind::Wasm] {
        let temp = tempfile::tempdir().unwrap();
        let session = EngineSession::open(temp.path()).unwrap();
        let artifact = imported(&session, kind);
        let manager = StackManager::new(&session);
        let plan = manager
            .preview("local", &document(&artifact.reference))
            .unwrap();
        let service = &plan.services["app"];
        assert_eq!(service.digest, artifact.digest);
        assert!(service.local_metadata.is_some());
        assert_eq!(service.spec.command, "main");
        assert!(session.state().volumes().unwrap().is_empty());
        let created = manager.create_plan(&plan).unwrap();
        assert_eq!(created.instances[0].kind, kind);
        let source = manager.edit_source("local").unwrap();
        let update = manager
            .preview_update("local", created.version, &source.document)
            .unwrap();
        assert_eq!(
            manager.update_plan(&update).unwrap().version,
            created.version
        );
        assert_eq!(
            session
                .state()
                .stack_for_instance(&created.instances[0].id)
                .unwrap()
                .as_deref(),
            Some("local")
        );
    }
}
#[test]
fn changed_import_metadata_and_omitted_local_pin_fail_before_dataset_preparation() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let mut artifact = imported(&session, ArtifactKind::Webc);
    let manager = StackManager::new(&session);
    let preview = manager
        .preview("local", &document(&artifact.reference))
        .unwrap();
    let mut forged = preview.clone();
    forged.services.get_mut("app").unwrap().local_metadata = None;
    assert!(manager.create_plan(&forged).is_err());
    let mut unpinned = preview.clone();
    unpinned
        .services
        .get_mut("app")
        .unwrap()
        .spec
        .labels
        .remove(wasmdroid_engine::stacks::LOCAL_METADATA_LABEL);
    assert!(manager.create_plan(&unpinned).is_err());
    artifact
        .labels
        .insert("new_local_metadata".into(), "changed after review".into());
    session.state().put_artifact(&artifact).unwrap();
    assert!(manager.create_plan(&preview).is_err());
    assert!(session.state().instances("local").unwrap().is_empty());
    assert!(session.state().volumes().unwrap().is_empty());
    assert!(
        manager
            .create("local", &document(&artifact.reference))
            .is_ok()
    );
}
#[test]
fn forged_unlocked_wrong_digest_unsupported_kind_and_missing_local_identity_are_rejected() {
    for variant in 0..5 {
        let temp = tempfile::tempdir().unwrap();
        let session = EngineSession::open(temp.path()).unwrap();
        let mut artifact = imported(&session, ArtifactKind::Webc);
        match variant {
            0 => {
                artifact.labels.remove(ORIGIN_LABEL);
            }
            1 => {
                artifact.labels.remove(PINS_LABEL);
            }
            2 => {
                artifact.reference =
                    local_import::reference(&Digest::of(b"another identity")).unwrap();
            }
            3 => {
                artifact.kind = ArtifactKind::OciImage;
            }
            _ => {
                artifact.reference = ArtifactReference::parse("other.example/image@1.0.0").unwrap();
            }
        }
        session.state().put_artifact(&artifact).unwrap();
        let manager = StackManager::new(&session);
        assert!(
            manager
                .create("local", &document(&artifact.reference))
                .is_err()
        );
        let missing = local_import::reference(&Digest::of(b"not imported")).unwrap();
        assert!(manager.create("missing", &document(&missing)).is_err());
        assert!(session.state().instances("local").unwrap().is_empty());
        assert!(session.state().volumes().unwrap().is_empty());
    }
}
#[test]
fn an_imported_dependency_lock_cannot_silently_change_after_stack_review() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let mut root = imported(&session, ArtifactKind::Webc);
    let mut dependency = root.clone();
    dependency.reference = ArtifactReference::parse("wasmer.io/example/dependency@1.0.0").unwrap();
    dependency.digest = Digest::of(b"dependency metadata");
    dependency.labels = BTreeMap::new();
    session.state().put_artifact(&dependency).unwrap();
    let pin = DependencyPin {
        reference: dependency.reference.clone(),
        digest: dependency.digest.clone(),
        metadata: Digest::of(&serde_json::to_vec(&dependency).unwrap()),
    };
    root.labels.insert(
        PINS_LABEL.into(),
        serde_json::to_string(&vec![pin]).unwrap(),
    );
    session.state().put_artifact(&root).unwrap();
    let manager = StackManager::new(&session);
    let preview = manager
        .preview("local", &document(&root.reference))
        .unwrap();
    dependency
        .labels
        .insert("metadata_revision".into(), "changed".into());
    session.state().put_artifact(&dependency).unwrap();
    assert!(manager.create_plan(&preview).is_err());
    assert!(session.state().volumes().unwrap().is_empty());
    assert!(session.state().instances("local").unwrap().is_empty());
}
#[test]
fn worker_checks_the_actual_artifact_pin_and_explicit_review_can_refresh_it() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let mut artifact = imported(&session, ArtifactKind::Webc);
    let manager = StackManager::new(&session);
    let created = manager
        .create("local", &document(&artifact.reference))
        .unwrap();
    let before = &created.instances[0];
    wasmdroid_engine::stacks::validate_worker_artifact_pin(&before.spec, &artifact).unwrap();
    artifact
        .labels
        .insert("review_revision".into(), "new dependency review".into());
    session.state().put_artifact(&artifact).unwrap();
    assert!(
        wasmdroid_engine::stacks::validate_worker_artifact_pin(&before.spec, &artifact).is_err()
    );
    // An editor can still open the actual user settings; it never silently
    // clears/replaces the old execution pin without an explicit reviewed save.
    let source = manager.edit_source("local").unwrap();
    let next = manager
        .preview_update("local", created.version, &source.document)
        .unwrap();
    let changed = manager.update_plan(&next).unwrap();
    let after = &changed.instances[0];
    assert_eq!(before.id, after.id);
    assert!(after.version > before.version);
    assert_ne!(
        before.spec.labels[wasmdroid_engine::stacks::LOCAL_METADATA_LABEL],
        after.spec.labels[wasmdroid_engine::stacks::LOCAL_METADATA_LABEL]
    );
    wasmdroid_engine::stacks::validate_worker_artifact_pin(&after.spec, &artifact).unwrap();
}
#[test]
fn legacy_public_only_plan_stays_compatible_without_import_metadata_fields() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let mut artifact = imported(&session, ArtifactKind::Webc);
    artifact.reference = ArtifactReference::parse("wasmer.io/example/public@1.0.0").unwrap();
    artifact.labels = BTreeMap::from([("dev.wasmdroid.entrypoint".into(), "main".into())]);
    session.state().put_artifact(&artifact).unwrap();
    let manager = StackManager::new(&session);
    let plan = manager
        .preview("public", &document(&artifact.reference))
        .unwrap();
    let json = serde_json::to_string(&plan).unwrap();
    assert!(!json.contains("local_metadata"));
    let decoded = serde_json::from_str(&json).unwrap();
    assert!(manager.create_plan(&decoded).is_ok());
}

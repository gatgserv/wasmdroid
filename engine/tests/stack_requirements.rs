use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference},
    session::EngineSession,
    stacks::package_requirements,
    store::Digest,
};

#[test]
fn missing_public_inventory_is_deduplicated_private_free_and_does_not_mutate_state() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let document = r#"services:
  web:
    image: wasmer.io/example/missing@1.0.0
    environment: {PRIVATE_TOKEN: secret_marker}
  worker:
    image: wasmer.io/example/missing@1.0.0
"#;
    let requirements = package_requirements(document, session.state()).unwrap();
    assert_eq!(requirements.len(), 1);
    assert_eq!(requirements[0].services, ["web", "worker"]);
    assert!(!requirements[0].installed);
    assert!(requirements[0].downloadable);
    let wire = serde_json::to_string(&requirements).unwrap();
    assert!(!wire.contains("PRIVATE_TOKEN"));
    assert!(!wire.contains("secret_marker"));
    assert!(session.state().events_after(0, 100).unwrap().is_empty());
    assert!(session.state().volumes().unwrap().is_empty());
    assert!(session.state().artifacts().unwrap().is_empty());
}

#[test]
fn installed_public_inventory_and_missing_local_digest_have_distinct_actions() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse("wasmer.io/example/installed@1.0.0").unwrap(),
            digest: Digest::of(b"fixture"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
    let local = wasmdroid_engine::local_import::reference(&Digest::of(b"missing local")).unwrap();
    let document = format!(
        "services:\n  web:\n    image: wasmer.io/example/installed@1.0.0\n  local:\n    image: {}\n",
        local.as_str()
    );
    let requirements = package_requirements(&document, session.state()).unwrap();
    assert_eq!(requirements.len(), 2);
    assert!(!requirements[0].installed);
    assert!(!requirements[0].downloadable);
    assert!(requirements[1].installed);
    assert!(requirements[1].downloadable);
}

#[test]
fn unsupported_documents_references_and_noncanonical_imports_never_offer_downloads() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    for document in [
        "services: {web: {image: wasmer.io/example/x@1, privileged: true}}",
        "services: {web: {image: other.example/private/x@1}}",
        "services: {web: {image: local.wasmdroid/sha256/not-a-digest}}",
        "services: {web: {image: 'wasmer.io/example/$PACKAGE'}}",
        "services: {web: {image: wasmer.io/example/x@1, volumes: ['/host:/data']}}",
    ] {
        assert!(package_requirements(document, session.state()).is_err());
    }
    assert!(session.state().events_after(0, 100).unwrap().is_empty());
    assert!(session.state().volumes().unwrap().is_empty());
}

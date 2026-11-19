use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec, InstanceState},
    session::EngineSession,
    store::Digest,
};
fn fixture() -> (tempfile::TempDir, EngineSession) {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse("wasmer.io/fixture/permissions@1.0.0").unwrap(),
            digest: Digest::of(b"metadata fixture"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
    let spec:InstanceSpec=serde_json::from_value(serde_json::json!({"artifact_ref":"wasmer.io/fixture/permissions@1.0.0","command":"run",
        "args":["private-argument"],"env":{"VALUE":"private-value"},"labels":{"dev.wasmdroid.ui.name":"Example"},
        "restart_policy":"always","start_on_boot":true,"tty":true,"stdin_open":true})).unwrap();
    session.state().create_instance("app", &spec).unwrap();
    (dir, session)
}
#[test]
fn independent_grants_and_withdrawal_preserve_every_unrelated_field() {
    let (_dir, session) = fixture();
    let source = session.state().network_permission_source("app").unwrap();
    assert!(!source.internet && !source.lan && source.network_enabled && !source.managed);
    let original = session.state().instance("app").unwrap().spec;
    for (internet, lan) in [(true, false), (false, true), (true, true), (false, false)] {
        let current = session.state().network_permission_source("app").unwrap();
        let record = session
            .state()
            .set_network_permissions("app", &current.identity, current.version, internet, lan)
            .unwrap();
        assert_eq!(
            (record.spec.internet_access, record.spec.lan_access),
            (internet, lan)
        );
        let mut expected = original.clone();
        expected.internet_access = internet;
        expected.lan_access = lan;
        assert_eq!(
            serde_json::to_value(record.spec).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(record.state, InstanceState::Created);
    }
}
#[test]
fn stale_version_and_recreated_same_id_cannot_change_another_application() {
    let (_dir, session) = fixture();
    let old = session.state().network_permission_source("app").unwrap();
    let saved = session
        .state()
        .set_network_permissions("app", &old.identity, old.version, true, false)
        .unwrap();
    assert!(
        session
            .state()
            .set_network_permissions("app", &old.identity, old.version, false, true)
            .is_err()
    );
    let spec = saved.spec;
    session
        .state()
        .remove_instance("app", saved.version)
        .unwrap();
    session.state().create_instance("app", &spec).unwrap();
    let replacement = session.state().network_permission_source("app").unwrap();
    assert_ne!(replacement.identity, old.identity);
    assert!(
        session
            .state()
            .set_network_permissions("app", &old.identity, replacement.version, false, true)
            .is_err()
    );
    let actual = session.state().instance("app").unwrap();
    assert!(actual.spec.internet_access && !actual.spec.lan_access);
    assert_eq!(actual.version, replacement.version);
}
#[test]
fn active_application_cannot_be_edited_and_noop_does_not_manufacture_revision() {
    let (_dir, session) = fixture();
    let source = session.state().network_permission_source("app").unwrap();
    let record = session
        .state()
        .set_network_permissions("app", &source.identity, source.version, false, false)
        .unwrap();
    assert_eq!(record.version, source.version);
    let running = session
        .state()
        .start_instance("app", record.version, &[])
        .unwrap();
    assert!(
        session
            .state()
            .set_network_permissions("app", &source.identity, running.version, true, false)
            .is_err()
    );
    assert!(
        !session
            .state()
            .instance("app")
            .unwrap()
            .spec
            .internet_access
    );
}
#[test]
fn source_contains_no_private_arguments_or_environment_values() {
    let (_dir, session) = fixture();
    let source = session.state().network_permission_source("app").unwrap();
    let raw = serde_json::to_string(&source).unwrap();
    assert!(!raw.contains("private-argument") && !raw.contains("private-value"));
    assert_eq!(source.name, "Example");
    assert!(!format!("{source:?}").contains(&source.identity));
}

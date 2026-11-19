//! Metadata regression definitions. No WebView/provider/browser is executed.
use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec, InstanceState},
    session::EngineSession,
    stacks::StackManager,
    store::Digest,
};

fn artifact(session: &EngineSession, version: &str) {
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse(format!("wasmer.io/example/browser@{version}"))
                .unwrap(),
            digest: Digest::of(version.as_bytes()),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["serve".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
}
fn create(session: &EngineSession, id: &str) -> wasmdroid_engine::state::InstanceRecord {
    artifact(session, "1.0.0");
    let spec:InstanceSpec=serde_json::from_value(serde_json::json!({"artifact_ref":"wasmer.io/example/browser@1.0.0","command":"serve","labels":{"dev.wasmdroid.browser.profile":"arbitrary-label-must-not-assign-profile"}})).unwrap();
    session.state().create_instance(id, &spec).unwrap()
}

#[test]
fn v4_migration_preserves_exact_legacy_profile_keys() {
    let temp = tempfile::tempdir().unwrap();
    {
        let session = EngineSession::open(temp.path()).unwrap();
        create(&session, "legacy");
    }
    let raw = rusqlite::Connection::open(temp.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("DROP TRIGGER revoke_group_network_on_spec_change; DROP TABLE group_network_grants; DROP TABLE group_network_revisions; DROP TABLE browser_origin_assignments; DROP TABLE browser_origin_profiles; DROP TABLE instance_browser_identity; DROP TABLE instance_automation; PRAGMA user_version=4;").unwrap();
    drop(raw);
    let owner = EngineSession::open(temp.path()).unwrap();
    assert_eq!(owner.state().browser_identity("legacy").unwrap(), "legacy");
}

#[test]
fn fresh_apps_and_reused_actual_names_get_fresh_private_profiles_independent_of_labels() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let first = create(&session, "app");
    let identity = session.state().browser_identity("app").unwrap();
    assert!(identity.starts_with("browser:"));
    assert_ne!(identity, "arbitrary-label-must-not-assign-profile");
    create(&session, "other");
    assert_ne!(session.state().browser_identity("other").unwrap(), identity);
    session
        .state()
        .remove_instance("app", first.version)
        .unwrap();
    create(&session, "app");
    assert_ne!(session.state().browser_identity("app").unwrap(), identity);
}

#[test]
fn standalone_dead_retry_moves_identity_atomically_and_failed_retry_preserves_both_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let created = create(&session, "app");
    create(&session, "occupied");
    let original = session.state().browser_identity("app").unwrap();
    let occupied = session.state().browser_identity("occupied").unwrap();
    let dead = session
        .state()
        .transition_instance(
            "app",
            created.version,
            InstanceState::Dead,
            None,
            Some("fixture death"),
        )
        .unwrap();
    assert!(
        session
            .state()
            .retry_dead_instance("app", dead.version, "occupied")
            .is_err()
    );
    assert_eq!(session.state().browser_identity("app").unwrap(), original);
    assert_eq!(
        session.state().browser_identity("occupied").unwrap(),
        occupied
    );
    session
        .state()
        .retry_dead_instance("app", dead.version, "replacement")
        .unwrap();
    assert_eq!(
        session.state().browser_identity("replacement").unwrap(),
        original
    );
    assert!(session.state().browser_identity("app").is_err());
    assert!(session.state().instance("app").is_err());
}

#[test]
fn managed_dead_retry_retains_profile_but_package_replacement_allocates_a_fresh_one() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    artifact(&session, "1.0.0");
    artifact(&session, "2.0.0");
    let manager = StackManager::new(&session);
    let source = "services: {web: {image: wasmer.io/example/browser@1.0.0}}";
    let stack = manager.create("group", source).unwrap();
    let previous = &stack.instances[0];
    let identity = session.state().browser_identity(&previous.id).unwrap();
    let dead = session
        .state()
        .transition_instance(
            &previous.id,
            previous.version,
            InstanceState::Dead,
            None,
            Some("fixture death"),
        )
        .unwrap();
    session
        .state()
        .retry_stack_member(
            "group",
            stack.version,
            "web",
            dead.version,
            "managed-replacement",
        )
        .unwrap();
    assert_eq!(
        session
            .state()
            .browser_identity("managed-replacement")
            .unwrap(),
        identity
    );
    let current = session.state().stack("group").unwrap();
    let preview = manager
        .preview_update(
            "group",
            current.version,
            "services: {web: {image: wasmer.io/example/browser@2.0.0}}",
        )
        .unwrap();
    let replaced = manager.update_plan(&preview).unwrap();
    assert_ne!(
        session
            .state()
            .browser_identity(&replaced.instances[0].id)
            .unwrap(),
        identity
    );
    assert!(
        session
            .state()
            .browser_identity("managed-replacement")
            .is_err()
    );
}

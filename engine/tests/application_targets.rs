use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec, InstanceState},
    session::EngineSession,
    stacks::StackManager,
    state::ApplicationTargetKind,
    store::Digest,
};

fn fixture(session: &EngineSession) -> InstanceSpec {
    let reference = ArtifactReference::parse("wasmer.io/fixture/application@1.0.0").unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: reference.clone(),
            digest: Digest::of(b"fixture"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
    serde_json::from_value(serde_json::json!({"artifact_ref":reference,"command":"run","env":{"PRIVATE_VALUE":"PRIVATE_MARKER"}})).unwrap()
}
fn dead(session: &EngineSession, id: &str) {
    let created = session.state().instance(id).unwrap();
    let running = session
        .state()
        .start_instance(id, created.version, &[])
        .unwrap();
    session
        .state()
        .transition_instance(
            id,
            running.version,
            InstanceState::Dead,
            None,
            Some("fixture interruption"),
        )
        .unwrap();
}

#[test]
fn standalone_target_follows_same_spec_retry_but_cannot_follow_a_recreated_actual_name() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let spec = fixture(&session);
    session.state().create_instance("old", &spec).unwrap();
    let target = session
        .state()
        .application_target(ApplicationTargetKind::Application, "old")
        .unwrap()
        .target;
    dead(&session, "old");
    let previous = session.state().instance("old").unwrap();
    session
        .state()
        .retry_dead_instance("old", previous.version, "replacement")
        .unwrap();
    assert_eq!(
        session
            .state()
            .resolve_application_target(&target)
            .unwrap()
            .target
            .id,
        "replacement"
    );
    let replacement = session.state().instance("replacement").unwrap();
    session
        .state()
        .remove_instance("replacement", replacement.version)
        .unwrap();
    session.state().create_instance("old", &spec).unwrap();
    assert!(session.state().resolve_application_target(&target).is_err());
    assert_ne!(
        session
            .state()
            .application_target(ApplicationTargetKind::Application, "old")
            .unwrap()
            .target
            .identity_token,
        target.identity_token
    );
}

#[test]
fn group_members_are_not_independent_targets_and_group_birth_survives_member_retry_only() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session);
    let source = "services: {web: {image: wasmer.io/fixture/application@1.0.0, command: [run]}}";
    let group = StackManager::new(&session).create("notes", source).unwrap();
    let target = session
        .state()
        .application_target(ApplicationTargetKind::Group, "notes")
        .unwrap()
        .target;
    let id = &group.instances[0].id;
    assert!(
        session
            .state()
            .application_target(ApplicationTargetKind::Application, id)
            .is_err()
    );
    dead(&session, id);
    let previous = session.state().instance(id).unwrap();
    let retried = session
        .state()
        .retry_stack_member("notes", group.version, "web", previous.version, "new-web")
        .unwrap();
    let current = session.state().resolve_application_target(&target).unwrap();
    assert_eq!(current.members[0].id, "new-web");
    assert_eq!(current.target, target);
    let page = session.state().application_page(None, 50).unwrap();
    assert_eq!(page.targets.len(), 1);
    // retry_stack_member returns the replacement instance revision. It must
    // never be mistaken for the separately versioned group revision.
    assert!(
        session
            .state()
            .remove_stack("notes", retried.version)
            .is_err()
    );
    session
        .state()
        .remove_stack("notes", current.version)
        .unwrap();
    StackManager::new(&session).create("notes", source).unwrap();
    assert!(session.state().resolve_application_target(&target).is_err());
    assert_ne!(
        session
            .state()
            .application_target(ApplicationTargetKind::Group, "notes")
            .unwrap()
            .target
            .identity_token,
        target.identity_token
    );
}

#[test]
fn target_pages_are_bounded_private_free_and_stop_all_epoch_survives_owner_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let epoch;
    {
        let session = EngineSession::open(temp.path()).unwrap();
        let spec = fixture(&session);
        for id in ["one", "two", "three"] {
            session.state().create_instance(id, &spec).unwrap();
        }
        let first = session.state().application_page(None, 2).unwrap();
        assert_eq!(first.targets.len(), 2);
        assert!(first.next.is_some());
        let next = session
            .state()
            .application_page(first.next.as_deref(), 2)
            .unwrap();
        assert_eq!(next.targets.len(), 1);
        assert!(next.next.is_none());
        let wire = serde_json::to_string(&first).unwrap();
        assert!(!wire.contains("PRIVATE_"));
        assert!(!wire.contains("artifact_ref"));
        assert!(session.state().application_page(None, 0).is_err());
        assert!(session.state().application_page(None, 51).is_err());
        let old = session.state().application_control_epoch().unwrap();
        assert_eq!(old, 0);
        assert_eq!(session.state().stop_all_run_intents().unwrap(), 0);
        epoch = session.state().application_control_epoch().unwrap();
        assert!(epoch > old);
    }
    let session = EngineSession::open(temp.path()).unwrap();
    assert_eq!(session.state().application_control_epoch().unwrap(), epoch);
    session.state().stop_all_run_intents().unwrap();
    assert!(session.state().application_control_epoch().unwrap() > epoch);
}

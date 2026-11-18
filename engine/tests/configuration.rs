//! Metadata/volume/CAS regressions; no guest execution is implied by transitions.
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};
use wasmdroid_engine::{
    EngineError,
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec, InstanceState},
    session::EngineSession,
    store::Digest,
    volumes::{VolumeManager, VolumeOpenOptions},
};
fn fixture() -> (tempfile::TempDir, EngineSession) {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse("wasmer.io/example/service@1.0.0").unwrap(),
            digest: Digest::of(b"metadata fixture"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["serve".into(), "alternate".into()],
            size_bytes: 16,
            labels: BTreeMap::new(),
        })
        .unwrap();
    let volumes = VolumeManager::open(&session).unwrap();
    for name in ["old", "new"] {
        volumes.create_new(name, BTreeMap::new()).unwrap();
    }
    drop(volumes);
    (dir, session)
}
fn spec(volume: &str) -> InstanceSpec {
    serde_json::from_value(serde_json::json!({"artifact_ref":"wasmer.io/example/service@1.0.0","command":"serve",
        "env":{"PRIVATE_ENV":"PRIVATE_OLD_VALUE"},"mounts":[{"source":{"type":"volume","source":volume},"target":"/data","read_only":false}]})).unwrap()
}
#[test]
fn editing_an_exited_spec_preserves_data_history_and_keeps_values_out_of_events() {
    let (dir, session) = fixture();
    let volumes = VolumeManager::open(&session).unwrap();
    volumes
        .lease("old", false)
        .unwrap()
        .open_file(
            "note",
            VolumeOpenOptions {
                write: true,
                create: true,
                ..Default::default()
            },
        )
        .unwrap()
        .write_all(b"retained dataset")
        .unwrap();
    let created = session
        .state()
        .create_instance("app", &spec("old"))
        .unwrap();
    let running = session
        .state()
        .transition_instance("app", created.version, InstanceState::Running, None, None)
        .unwrap();
    let exited = session
        .state()
        .transition_instance("app", running.version, InstanceState::Exited, Some(7), None)
        .unwrap();
    let mut next = spec("new");
    next.command = "alternate".into();
    next.args = vec!["PRIVATE_ARG_VALUE".into()];
    next.env
        .insert("PRIVATE_ENV".into(), "PRIVATE_NEW_VALUE".into());
    next.tty = true;
    next.stdin_open = true;
    let configured = session
        .state()
        .configure_instance("app", exited.version, &next)
        .unwrap();
    assert_eq!(configured.version, exited.version + 1);
    assert_eq!(configured.state, InstanceState::Exited);
    assert_eq!(configured.exit_code, Some(7));
    assert_eq!(configured.spec.command, "alternate");
    assert!(session.state().volume_references("old").unwrap().is_empty());
    assert_eq!(
        session.state().volume_references("new").unwrap(),
        vec!["app"]
    );
    let mut value = String::new();
    volumes
        .lease("old", true)
        .unwrap()
        .open_file(
            "note",
            VolumeOpenOptions {
                read: true,
                ..Default::default()
            },
        )
        .unwrap()
        .read_to_string(&mut value)
        .unwrap();
    assert_eq!(value, "retained dataset");
    assert_eq!(session.state().volumes().unwrap().len(), 2);
    let events = session.state().events_after(0, 100).unwrap();
    assert_eq!(
        events.last().unwrap().payload["type"],
        "instance_configured"
    );
    assert!(!serde_json::to_string(&events).unwrap().contains("PRIVATE_"));
    drop(volumes);
    drop(session);
    let reopened = EngineSession::open(dir.path())
        .unwrap()
        .state()
        .instance("app")
        .unwrap();
    assert_eq!(reopened.version, configured.version);
    assert_eq!(reopened.exit_code, Some(7));
    assert_eq!(reopened.spec.env["PRIVATE_ENV"], "PRIVATE_NEW_VALUE");
}
#[test]
fn no_op_and_stale_saves_cannot_publish_extra_revisions_or_reference_changes() {
    let (_dir, session) = fixture();
    let db = session.state();
    let original = spec("old");
    let created = db.create_instance("app", &original).unwrap();
    let count = db.events_after(0, 100).unwrap().len();
    let unchanged = db
        .configure_instance("app", created.version, &original)
        .unwrap();
    assert_eq!(unchanged.version, created.version);
    assert_eq!(db.events_after(0, 100).unwrap().len(), count);
    let mut next = original.clone();
    next.args.push("new argument".into());
    let saved = db
        .configure_instance("app", created.version, &next)
        .unwrap();
    assert!(matches!(
        db.configure_instance("app", created.version, &spec("new")),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(db.instance("app").unwrap().version, saved.version);
    assert_eq!(db.volume_references("old").unwrap(), vec!["app"]);
    assert!(db.volume_references("new").unwrap().is_empty());
    assert_eq!(db.events_after(0, 100).unwrap().len(), count + 1);
}
#[test]
fn active_and_incompatible_identity_edits_fail_without_side_effects() {
    let (_dir, session) = fixture();
    let db = session.state();
    for (id, target) in [
        ("running", InstanceState::Running),
        ("paused", InstanceState::Paused),
        ("restarting", InstanceState::Restarting),
    ] {
        let created = db.create_instance(id, &spec("old")).unwrap();
        let mut current = db
            .transition_instance(id, created.version, InstanceState::Running, None, None)
            .unwrap();
        if target != InstanceState::Running {
            current = db
                .transition_instance(id, current.version, target, None, None)
                .unwrap();
        }
        let before = db.events_after(0, 100).unwrap().len();
        assert!(matches!(
            db.configure_instance(id, current.version, &spec("new")),
            Err(EngineError::Conflict(_))
        ));
        assert_eq!(db.instance(id).unwrap().version, current.version);
        assert_eq!(db.events_after(0, 100).unwrap().len(), before);
    }
    let created = db.create_instance("idle", &spec("old")).unwrap();
    let mut changed = created.spec.clone();
    changed.endpoint_id = "other".into();
    assert!(
        db.configure_instance("idle", created.version, &changed)
            .is_err()
    );
    changed = created.spec.clone();
    changed.artifact_ref = ArtifactReference::parse("wasmer.io/example/other@1.0.0").unwrap();
    assert!(
        db.configure_instance("idle", created.version, &changed)
            .is_err()
    );
    changed = created.spec.clone();
    changed.command = "absent".into();
    assert!(
        db.configure_instance("idle", created.version, &changed)
            .is_err()
    );
    assert_eq!(db.instance("idle").unwrap().version, created.version);
    assert!(db.volume_references("new").unwrap().is_empty());
}
#[test]
fn missing_pending_and_failed_reference_writes_roll_back_the_whole_configuration() {
    let (dir, session) = fixture();
    let db = session.state();
    let original = spec("old");
    let created = db.create_instance("app", &original).unwrap();
    let count = db.events_after(0, 100).unwrap().len();
    assert!(
        db.configure_instance("app", created.version, &spec("missing"))
            .is_err()
    );
    let raw = rusqlite::Connection::open(dir.path().join("engine.sqlite")).unwrap();
    raw.execute(
        "INSERT INTO volume_deletions(name,requested_at_ms) VALUES ('new',0)",
        [],
    )
    .unwrap();
    assert!(matches!(
        db.configure_instance("app", created.version, &spec("new")),
        Err(EngineError::Conflict(_))
    ));
    raw.execute("DELETE FROM volume_deletions WHERE name='new'", [])
        .unwrap();
    raw.execute_batch("CREATE TRIGGER fail_config_reference BEFORE INSERT ON instance_volumes WHEN NEW.volume_name='new' BEGIN SELECT RAISE(ABORT,'fixture reference failure'); END;").unwrap();
    assert!(
        db.configure_instance("app", created.version, &spec("new"))
            .is_err()
    );
    assert_eq!(db.instance("app").unwrap().version, created.version);
    assert_eq!(
        serde_json::to_value(db.instance("app").unwrap().spec).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    assert_eq!(db.volume_references("old").unwrap(), vec!["app"]);
    assert!(db.volume_references("new").unwrap().is_empty());
    assert_eq!(db.events_after(0, 100).unwrap().len(), count);
}
#[test]
fn invalid_oversize_and_reserved_ownership_edits_cannot_claim_configuration_success() {
    let (_dir, session) = fixture();
    let db = session.state();
    let created = db.create_instance("app", &spec("old")).unwrap();
    let before = db.events_after(0, 100).unwrap().len();
    let mut invalid = created.spec.clone();
    invalid.args.push("x".repeat(128 * 1024));
    assert!(
        db.configure_instance("app", created.version, &invalid)
            .is_err()
    );
    invalid = created.spec.clone();
    invalid.mounts.push(invalid.mounts[0].clone());
    assert!(
        db.configure_instance("app", created.version, &invalid)
            .is_err()
    );
    invalid = created.spec.clone();
    invalid.labels.insert(
        wasmdroid_engine::stacks::STACK_LABEL.into(),
        "managed".into(),
    );
    assert!(matches!(
        db.configure_instance("app", created.version, &invalid),
        Err(EngineError::Unsupported(_))
    ));
    assert_eq!(db.instance("app").unwrap().version, created.version);
    assert_eq!(db.events_after(0, 100).unwrap().len(), before);
}

#[test]
fn a_managed_member_requires_definition_edit_even_if_the_requested_labels_are_removed() {
    let (_dir, session) = fixture();
    let stack = wasmdroid_engine::stacks::StackManager::new(&session)
        .create(
            "managed",
            "services:\n  app:\n    image: wasmer.io/example/service@1.0.0\n    command: [serve]\n",
        )
        .unwrap();
    let member = &stack.instances[0];
    let mut next = member.spec.clone();
    next.labels.remove(wasmdroid_engine::stacks::STACK_LABEL);
    next.labels.remove(wasmdroid_engine::stacks::SERVICE_LABEL);
    next.args.push("changed".into());
    let before = session.state().events_after(0, 100).unwrap().len();
    assert!(matches!(
        session
            .state()
            .configure_instance(&member.id, member.version, &next),
        Err(EngineError::Unsupported(_))
    ));
    assert_eq!(
        session.state().instance(&member.id).unwrap().version,
        member.version
    );
    assert_eq!(
        session.state().stack_for_instance(&member.id).unwrap(),
        Some("managed".into())
    );
    assert_eq!(session.state().events_after(0, 100).unwrap().len(), before);
}

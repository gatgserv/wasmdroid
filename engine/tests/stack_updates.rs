//! Metadata/definition regression definitions. No runtime, shell or registry.
use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceState, MountSource},
    session::EngineSession,
    stacks::{ServiceChange, StackManager},
    store::Digest,
    volumes::VolumeManager,
};

const SOURCE: &str = r#"
services:
  db:
    image: wasmer.io/example/update@1.0.0
    volumes: ['data:/data']
  web:
    image: wasmer.io/example/update@1.0.0
    depends_on: [db]
    environment: {PRIVATE_VALUE: 'kept $$HOME'}
volumes: {data: {}, unused: {}}
"#;
fn fixture(session: &EngineSession, version: &str) {
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse(format!("wasmer.io/example/update@{version}"))
                .unwrap(),
            digest: Digest::of(version.as_bytes()),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["serve".into()],
            size_bytes: 0,
            labels: BTreeMap::from([("dev.wasmdroid.entrypoint".into(), "serve".into())]),
        })
        .unwrap();
}
fn changed_document(manager: &StackManager<'_>) -> serde_json::Value {
    let source = manager.edit_source("demo").unwrap();
    let mut document: serde_json::Value = serde_json::from_str(&source.document).unwrap();
    document["services"]["web"]["environment"]["PRIVATE_VALUE"] = "changed PRIVATE_MARKER".into();
    document
}
#[test]
fn update_is_atomic_preserves_idle_identity_exit_history_and_dataset_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session, "1.0.0");
    let manager = StackManager::new(&session);
    let created = manager.create("demo", SOURCE).unwrap();
    let database = &created.instances[0];
    let running = session
        .state()
        .start_instance(&database.id, database.version, &[])
        .unwrap();
    let exited = session
        .state()
        .transition_instance(
            &database.id,
            running.version,
            InstanceState::Exited,
            Some(0),
            None,
        )
        .unwrap();
    let data = &created.plan.volumes["data"].name;
    let sentinel = temp.path().join("volumes/data").join(data).join("kept");
    std::fs::write(&sentinel, b"dataset bytes").unwrap();
    let mut document = changed_document(&manager);
    document["services"]["worker"] = document["services"]["web"].clone();
    let preview = manager
        .preview_update("demo", created.version, &document.to_string())
        .unwrap();
    assert!(preview.changes["db"] == ServiceChange::Keep);
    assert!(preview.changes["web"] == ServiceChange::Configure);
    assert!(preview.changes["worker"] == ServiceChange::Add);
    assert_eq!(
        session.state().stack("demo").unwrap().version,
        created.version
    );
    let applied = manager.update_plan(&preview).unwrap();
    assert!(applied.version > created.version);
    assert_eq!(applied.instances.len(), 3);
    let current = session.state().instance(&database.id).unwrap();
    assert_eq!(current.version, exited.version);
    assert_eq!(current.state, InstanceState::Exited);
    assert_eq!(current.exit_code, Some(0));
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"dataset bytes");
    assert_eq!(
        serde_json::to_value(session.state().stack_initial_plan("demo").unwrap()).unwrap(),
        serde_json::to_value(&created.plan).unwrap()
    );
    let exported = manager.edit_source("demo").unwrap();
    let noop = manager
        .preview_update("demo", applied.version, &exported.document)
        .unwrap();
    let events = session.state().events_after(0, 100).unwrap().len();
    assert_eq!(manager.update_plan(&noop).unwrap().version, applied.version);
    assert_eq!(session.state().events_after(0, 100).unwrap().len(), events);
    let events = serde_json::to_string(&session.state().events_after(0, 100).unwrap()).unwrap();
    assert!(!events.contains("PRIVATE_"));
    assert!(!events.contains("kept $HOME"));
}
#[test]
fn stale_restored_member_is_rejected_and_export_preserves_actual_restored_mounts() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session, "1.0.0");
    let manager = StackManager::new(&session);
    let created = manager.create("demo", SOURCE).unwrap();
    let preview = manager
        .preview_update(
            "demo",
            created.version,
            &changed_document(&manager).to_string(),
        )
        .unwrap();
    let volume = VolumeManager::open(&session).unwrap();
    volume.create_new("restored", BTreeMap::new()).unwrap();
    let old = &created.instances[0];
    let data = &created.plan.volumes["data"].name;
    session
        .state()
        .replace_instance_volume(&old.id, old.version, data, "restored")
        .unwrap();
    assert!(manager.update_plan(&preview).is_err());
    assert_eq!(
        session.state().stack("demo").unwrap().version,
        created.version
    );
    let current = manager.edit_source("demo").unwrap();
    let exported = manager.preview("demo", &current.document).unwrap();
    assert!(
        matches!(&exported.services["db"].spec.mounts[0].source,MountSource::Volume(name) if name=="restored")
    );
    assert_eq!(
        exported.services["web"].spec.env["PRIVATE_VALUE"],
        "kept $HOME"
    );
    // Explicitly remove the service and all current declarations. Both original
    // and restored data remain protected until the definition itself is removed.
    let mut document: serde_json::Value = serde_json::from_str(&current.document).unwrap();
    document["services"].as_object_mut().unwrap().remove("db");
    document["services"]["web"]["depends_on"] = serde_json::json!([]);
    document["volumes"] = serde_json::json!({});
    let update = manager
        .preview_update("demo", current.version, &document.to_string())
        .unwrap();
    let applied = manager.update_plan(&update).unwrap();
    assert!(session.state().instance(&old.id).is_err());
    assert!(volume.remove("restored").is_err());
    assert!(volume.remove(data).is_err());
    session
        .state()
        .remove_stack("demo", applied.version)
        .unwrap();
    assert!(volume.inspect("restored").is_ok());
    assert!(volume.inspect(data).is_ok());
    volume.remove("restored").unwrap();
}
#[test]
fn active_or_retried_members_and_tampered_review_cannot_apply_or_prepare_data() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session, "1.0.0");
    let manager = StackManager::new(&session);
    let created = manager.create("demo", SOURCE).unwrap();
    let mut document = changed_document(&manager);
    document["volumes"]["future"] = serde_json::json!({});
    let preview = manager
        .preview_update("demo", created.version, &document.to_string())
        .unwrap();
    let old = &created.instances[0];
    session
        .state()
        .start_instance(&old.id, old.version, &[])
        .unwrap();
    assert!(manager.update_plan(&preview).is_err());
    let active = manager
        .preview_update("demo", created.version, &document.to_string())
        .unwrap();
    assert!(manager.update_plan(&active).is_err());
    assert_eq!(session.state().volumes().unwrap().len(), 2);
    let running = session.state().instance(&old.id).unwrap();
    let dead = session
        .state()
        .transition_instance(
            &old.id,
            running.version,
            InstanceState::Dead,
            None,
            Some("fixture crash"),
        )
        .unwrap();
    let current = manager
        .preview_update("demo", created.version, &document.to_string())
        .unwrap();
    let browser_identity = session.state().browser_identity(&old.id).unwrap();
    session
        .state()
        .retry_stack_member("demo", created.version, "db", dead.version, "replacement")
        .unwrap();
    assert_eq!(
        session.state().browser_identity("replacement").unwrap(),
        browser_identity
    );
    assert!(manager.update_plan(&current).is_err());
    let stack = session.state().stack("demo").unwrap();
    let mut tampered = manager
        .preview_update("demo", stack.version, &document.to_string())
        .unwrap();
    tampered.changes.insert("db".into(), ServiceChange::Add);
    assert!(manager.update_plan(&tampered).is_err());
    let stable = session.state().stack("demo").unwrap();
    assert_eq!(stable.version, stack.version);
    assert_eq!(session.state().volumes().unwrap().len(), 2);
}
#[test]
fn package_replacement_and_reintroduced_service_use_fresh_actual_identities() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session, "1.0.0");
    fixture(&session, "2.0.0");
    let manager = StackManager::new(&session);
    let original = manager.create("demo", SOURCE).unwrap();
    let previous_web = original.instances[1].id.clone();
    let previous_profile = session.state().browser_identity(&previous_web).unwrap();
    let mut document = changed_document(&manager);
    document["services"]["web"]["image"] = "wasmer.io/example/update@2.0.0".into();
    let preview = manager
        .preview_update("demo", original.version, &document.to_string())
        .unwrap();
    assert!(preview.changes["web"] == ServiceChange::Replace);
    let replaced = manager.update_plan(&preview).unwrap();
    let next_web = replaced.instances[1].id.clone();
    let next_profile = session.state().browser_identity(&next_web).unwrap();
    assert_ne!(previous_profile, next_profile);
    assert_ne!(previous_web, next_web);
    assert!(session.state().instance(&previous_web).is_err());
    let restore = document.clone();
    document["services"].as_object_mut().unwrap().remove("web");
    let removed = manager
        .update_plan(
            &manager
                .preview_update("demo", replaced.version, &document.to_string())
                .unwrap(),
        )
        .unwrap();
    let reintroduced = manager
        .update_plan(
            &manager
                .preview_update("demo", removed.version, &restore.to_string())
                .unwrap(),
        )
        .unwrap();
    assert_ne!(reintroduced.instances[1].id, next_web);
    assert_ne!(reintroduced.instances[1].id, previous_web);
    let reintroduced_profile = session
        .state()
        .browser_identity(&reintroduced.instances[1].id)
        .unwrap();
    assert_ne!(reintroduced_profile, next_profile);
    assert_ne!(reintroduced_profile, previous_profile);
    assert!(session.state().instance(&next_web).is_err());
}
#[test]
fn sql_failure_rolls_back_specs_membership_references_events_and_history_but_keeps_prepared_data() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session, "1.0.0");
    let manager = StackManager::new(&session);
    let original = manager.create("demo", SOURCE).unwrap();
    let mut document = changed_document(&manager);
    document["volumes"]["future"] = serde_json::json!({});
    let preview = manager
        .preview_update("demo", original.version, &document.to_string())
        .unwrap();
    let before = serde_json::to_value(session.state().stack("demo").unwrap()).unwrap();
    let events = session.state().events_after(0, 100).unwrap();
    let prepared_name = &preview.plan.volumes["future"].name;
    let raw = rusqlite::Connection::open(temp.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("CREATE TRIGGER refuse_update BEFORE UPDATE OF plan_json ON stacks BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;").unwrap();
    assert!(manager.update_plan(&preview).is_err());
    assert_eq!(
        serde_json::to_value(session.state().stack("demo").unwrap()).unwrap(),
        before
    );
    let after_failure = session.state().events_after(0, 100).unwrap();
    // Dataset preparation commits independently so a failed definition update
    // can safely reuse it. Its audit event survives; no publication event does.
    assert_eq!(after_failure.len(), events.len() + 1);
    assert_eq!(
        serde_json::to_value(&after_failure[..events.len()]).unwrap(),
        serde_json::to_value(&events).unwrap()
    );
    let prepared_event = &after_failure[events.len()];
    assert!(prepared_event.instance_id.is_none());
    assert_eq!(prepared_event.kind, "directory");
    assert_eq!(prepared_event.driver, "local-fs");
    assert_eq!(
        prepared_event.payload,
        serde_json::json!({"type":"volume_created","name":prepared_name})
    );
    assert_eq!(session.state().volumes().unwrap().len(), 3);
    assert_eq!(
        serde_json::to_value(session.state().stack_initial_plan("demo").unwrap()).unwrap(),
        serde_json::to_value(&original.plan).unwrap()
    );
    raw.execute_batch("DROP TRIGGER refuse_update").unwrap();
    assert!(manager.update_plan(&preview).is_ok());
    assert_eq!(session.state().volumes().unwrap().len(), 3);
    assert_eq!(
        session
            .state()
            .events_after(0, 100)
            .unwrap()
            .iter()
            .filter(|event| event.payload == prepared_event.payload)
            .count(),
        1,
        "retry must reuse the prepared dataset without duplicating its audit event"
    );
}
#[test]
fn legacy_v3_stack_migrates_without_losing_membership_or_data_and_future_schema_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let original = {
        let session = EngineSession::open(temp.path()).unwrap();
        fixture(&session, "1.0.0");
        StackManager::new(&session).create("demo", SOURCE).unwrap()
    };
    let path = temp.path().join("engine.sqlite");
    let raw = rusqlite::Connection::open(&path).unwrap();
    // Reconstruct the prior schema in this disposable fixture only.
    raw.execute_batch("DROP TRIGGER revoke_group_network_on_spec_change; DROP TABLE group_network_grants; DROP TABLE group_network_revisions; DROP TABLE browser_origin_assignments; DROP TABLE browser_origin_profiles; DROP TABLE instance_browser_identity; DROP TABLE instance_automation; DROP TABLE stack_retained_volumes; ALTER TABLE stacks DROP COLUMN initial_plan_json; PRAGMA user_version=3;").unwrap();
    drop(raw);
    {
        let session = EngineSession::open(temp.path()).unwrap();
        assert_eq!(
            serde_json::to_value(session.state().stack("demo").unwrap()).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        let manager = StackManager::new(&session);
        let updated = manager
            .update_plan(
                &manager
                    .preview_update(
                        "demo",
                        original.version,
                        &changed_document(&manager).to_string(),
                    )
                    .unwrap(),
            )
            .unwrap();
        assert!(updated.version > original.version);
        assert_eq!(session.state().volumes().unwrap().len(), 2);
    }
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        raw.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        7
    );
    raw.pragma_update(None, "user_version", 99).unwrap();
    assert!(EngineSession::open(temp.path()).is_err());
    assert_eq!(
        raw.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        99
    );
}

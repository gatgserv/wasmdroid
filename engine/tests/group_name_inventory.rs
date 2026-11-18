//! Actual native plan/membership metadata, not running guests or public DNS.
use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceState},
    network::grants::topology,
    session::EngineSession,
    stacks::StackManager,
    store::Digest,
};
fn group(session: &EngineSession) -> wasmdroid_engine::stacks::StackRecord {
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse("wasmer.io/example/inventory@1.0.0").unwrap(),
            digest: Digest::of(b"metadata"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: BTreeMap::from([("requested_name".into(), "leak.invalid".into())]),
        })
        .unwrap();
    StackManager::new(session)
        .create(
            "group",
            r#"services:
  web:
    image: wasmer.io/example/inventory@1.0.0
    environment: {REQUESTED_NAME: leak.invalid}
  DB:
    image: wasmer.io/example/inventory@1.0.0
  queue:
    image: wasmer.io/example/inventory@1.0.0
"#,
        )
        .unwrap()
}
#[test]
fn inventory_includes_actual_stopped_and_ungranted_services_not_requested_labels() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let group = group(&session);
    // Use actual membership; UI/labels are deliberately not the lookup authority.
    let members = session.state().stack_members("group").unwrap();
    let id = members
        .iter()
        .find(|member| member["service"] == "web")
        .unwrap()["instance_id"]
        .as_str()
        .unwrap();
    let workers = BTreeMap::from([(id.to_owned(), "generation".into())]);
    let config = topology(session.state(), "group", &workers, &BTreeMap::new(), 1)
        .unwrap()
        .remove(id)
        .unwrap();
    assert!(config.incoming.is_empty() && config.outgoing.is_empty());
    let names = config.reserved_aliases(session.state()).unwrap();
    assert_eq!(names, ["db".into(), "queue".into(), "web".into()].into());
    assert!(!names.contains("leak.invalid"));
    assert!(
        group
            .instances
            .iter()
            .all(|instance| instance.state == InstanceState::Created)
    );
}
#[test]
fn forged_or_wrong_membership_cannot_choose_native_inventory() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    group(&session);
    let id = session.state().stack_members("group").unwrap()[0]["instance_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let workers = BTreeMap::from([(id.clone(), "generation".into())]);
    let config = topology(session.state(), "group", &workers, &BTreeMap::new(), 1)
        .unwrap()
        .remove(&id)
        .unwrap();
    let mut forged = config.clone();
    forged.alias = "untrusted-name".into();
    assert!(forged.reserved_aliases(session.state()).is_err());
    let mut wrong = config;
    wrong.instance_id = "not-a-member".into();
    assert!(wrong.reserved_aliases(session.state()).is_err());
}
#[test]
fn dead_member_replacement_invalidates_old_inventory_capture_and_uses_actual_new_member() {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let record = group(&session);
    let members = session.state().stack_members("group").unwrap();
    let member = &members[0];
    let service = member["service"].as_str().unwrap();
    let id = member["instance_id"].as_str().unwrap();
    let workers = BTreeMap::from([(id.to_owned(), "generation".into())]);
    let old = topology(session.state(), "group", &workers, &BTreeMap::new(), 1)
        .unwrap()
        .remove(id)
        .unwrap();
    let actual = session.state().instance(id).unwrap();
    let dead = session
        .state()
        .transition_instance(
            id,
            actual.version,
            InstanceState::Dead,
            None,
            Some("fixture"),
        )
        .unwrap();
    session
        .state()
        .retry_stack_member(
            "group",
            record.version,
            service,
            dead.version,
            "replacement",
        )
        .unwrap();
    assert!(old.reserved_aliases(session.state()).is_err());
    let current = topology(
        session.state(),
        "group",
        &BTreeMap::from([("replacement".into(), "new-generation".into())]),
        &BTreeMap::new(),
        2,
    )
    .unwrap()
    .remove("replacement")
    .unwrap();
    assert_eq!(current.reserved_aliases(session.state()).unwrap().len(), 3);
}

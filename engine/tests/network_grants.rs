//! Private topology/crypto/membership regressions. No worker/guest is executed.
use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceState},
    network::grants::{WorkerNetwork, topology},
    session::EngineSession,
    stacks::StackManager,
    store::Digest,
};
fn fixture() -> (tempfile::TempDir, EngineSession) {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse("wasmer.io/example/grant-fixture@1.0.0").unwrap(),
            digest: Digest::of(b"metadata only"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
    (temp, session)
}
fn workers(session: &EngineSession, id: &str) -> BTreeMap<String, String> {
    let document = r#"{"services":{"a":{"image":"wasmer.io/example/grant-fixture@1.0.0"},"b":{"image":"wasmer.io/example/grant-fixture@1.0.0"},"c":{"image":"wasmer.io/example/grant-fixture@1.0.0"}}}"#;
    let stack = StackManager::new(session).create(id, document).unwrap();
    stack
        .instances
        .iter()
        .enumerate()
        .map(|(index, member)| (member.id.clone(), format!("generation-{index}")))
        .collect()
}
fn node<'a>(map: &'a BTreeMap<String, WorkerNetwork>, alias: &str) -> &'a WorkerNetwork {
    map.values().find(|config| config.alias == alias).unwrap()
}
#[test]
fn directed_authentication_scope_and_revisions_keep_unaffected_keys() {
    let (_temp, session) = fixture();
    let jobs = workers(&session, "one");
    let first = topology(session.state(), "one", &jobs, &BTreeMap::new(), 1).unwrap();
    let a = node(&first, "a");
    let b = node(&first, "b");
    let c = node(&first, "c");
    let outbound = a
        .outgoing
        .iter()
        .find(|peer| peer.instance_id == b.instance_id)
        .unwrap();
    let inbound = b
        .incoming
        .iter()
        .find(|peer| peer.instance_id == a.instance_id)
        .unwrap();
    let header = b"WDP1auth-header";
    let tag = outbound.key.tag(header);
    assert!(inbound.key.verify(header, &tag));
    assert!(!inbound.key.verify(b"changed-header", &tag));
    let opposite = b
        .outgoing
        .iter()
        .find(|peer| peer.instance_id == a.instance_id)
        .unwrap();
    assert!(!opposite.key.verify(header, &tag));
    let mut reduced = jobs.clone();
    reduced.remove(&c.instance_id);
    let second = topology(session.state(), "one", &reduced, &first, 2).unwrap();
    assert!(node(&second, "a").outgoing[0].key.same(&outbound.key));
    let mut restarted = jobs.clone();
    restarted.insert(b.instance_id.clone(), "new-generation".into());
    let third = topology(session.state(), "one", &restarted, &first, 3).unwrap();
    assert!(
        !node(&third, "a")
            .outgoing
            .iter()
            .find(|peer| peer.instance_id == b.instance_id)
            .unwrap()
            .key
            .same(&outbound.key)
    );
    let ac = a
        .outgoing
        .iter()
        .find(|peer| peer.instance_id == c.instance_id)
        .unwrap();
    assert!(
        node(&third, "a")
            .outgoing
            .iter()
            .find(|peer| peer.instance_id == c.instance_id)
            .unwrap()
            .key
            .same(&ac.key)
    );
    let other_jobs = workers(&session, "other");
    let other = topology(session.state(), "other", &other_jobs, &BTreeMap::new(), 4).unwrap();
    assert!(
        !node(&other, "b")
            .incoming
            .iter()
            .find(|peer| peer.alias == "a")
            .unwrap()
            .key
            .verify(header, &tag)
    );
    let secret = serde_json::to_value(a).unwrap()["outgoing"][0]["key"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(!format!("{a:?}").contains(&secret));
    let mut forged = a.clone();
    forged.outgoing[0].instance_id = node(&other, "b").instance_id.clone();
    assert!(forged.validate_membership(session.state()).is_err());
}
#[test]
fn managed_retry_changes_only_current_membership_and_fences_stale_stack_versions() {
    let (_temp, session) = fixture();
    let _jobs = workers(&session, "retry");
    let stack = session.state().stack("retry").unwrap();
    let previous = &stack.instances[0];
    let dead = session
        .state()
        .transition_instance(
            &previous.id,
            previous.version,
            InstanceState::Dead,
            None,
            Some("fixture metadata failure"),
        )
        .unwrap();
    let replacement = session
        .state()
        .retry_stack_member(
            "retry",
            stack.version,
            &stack.plan.startup_order[0],
            dead.version,
            "retry-new-member",
        )
        .unwrap();
    assert_eq!(replacement.spec.command, previous.spec.command);
    assert!(session.state().instance(&previous.id).is_err());
    let updated = session.state().stack("retry").unwrap();
    assert!(updated.version > stack.version);
    assert_eq!(updated.instances[0].id, "retry-new-member");
    assert_eq!(
        updated.plan.services[&stack.plan.startup_order[0]].instance_id,
        previous.id
    ); // Immutable creation history.
    assert!(
        session
            .state()
            .remove_stack("retry", stack.version)
            .is_err()
    );
    let current: BTreeMap<_, _> = updated
        .instances
        .iter()
        .enumerate()
        .map(|(index, member)| (member.id.clone(), format!("fresh-{index}")))
        .collect();
    assert!(topology(session.state(), "retry", &current, &BTreeMap::new(), 5).is_ok());
}

//! Crash-window definitions for durable authority; no real worker is launched.
use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec, InstanceState, RestartPolicy},
    session::EngineSession,
    state::{RunOutcome, RunStartSource},
    store::Digest,
};

fn fixture(
    session: &EngineSession,
    id: &str,
    policy: RestartPolicy,
) -> wasmdroid_engine::state::InstanceRecord {
    let reference = ArtifactReference::parse("wasmer.io/example/authority@1.0.0").unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: reference.clone(),
            digest: Digest::of(b"authority fixture"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["serve".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
    let spec:InstanceSpec=serde_json::from_value(serde_json::json!({"artifact_ref":reference,"command":"serve","env":{"PRIVATE_FIXTURE":"PRIVATE_FIXTURE_VALUE"},"restart_policy":policy,"start_on_boot":false})).unwrap();
    session.state().create_instance(id, &spec).unwrap()
}
fn running(
    session: &EngineSession,
    id: &str,
    policy: RestartPolicy,
) -> wasmdroid_engine::state::InstanceRecord {
    let instance = fixture(session, id, policy);
    session
        .state()
        .begin_authorized_run(
            id,
            instance.version,
            0,
            "first",
            Some(42),
            RunStartSource::Manual,
        )
        .unwrap();
    session
        .state()
        .ready_authorized_run(id, instance.version, "first", &[])
        .unwrap()
}

#[test]
fn automatic_start_preference_only_edits_two_fields_and_fences_the_reviewed_birth() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let db = session.state();
    let original = fixture(&session, "app", RestartPolicy::No);
    let identity = db.browser_identity("app").unwrap();
    let source = serde_json::to_value(db.automatic_start_source("app").unwrap()).unwrap();
    assert_eq!(source["editable"], true);
    assert!(!source.to_string().contains("PRIVATE_FIXTURE"));

    db.begin_authorized_run(
        "app",
        original.version,
        0,
        "reserved",
        Some(42),
        RunStartSource::Manual,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(db.automatic_start_source("app").unwrap()).unwrap()["editable"],
        false
    );
    assert!(
        db.set_automatic_start(
            "app",
            &identity,
            original.version,
            true,
            RestartPolicy::Always
        )
        .is_err()
    );
    assert!(db.run_authority("app").unwrap().desired_run);
    let ready = db
        .ready_authorized_run("app", original.version, "reserved", &[])
        .unwrap();
    let stopped = db
        .finish_authorized_run("app", ready.version, "reserved", Some(0), None, false)
        .unwrap();

    assert!(
        db.set_automatic_start(
            "app",
            "different-birth",
            stopped.version,
            true,
            RestartPolicy::Always
        )
        .is_err()
    );
    assert!(db.run_authority("app").unwrap().desired_run);
    let version = db
        .set_automatic_start(
            "app",
            &identity,
            stopped.version,
            true,
            RestartPolicy::Always,
        )
        .unwrap();
    let actual = db.instance("app").unwrap();
    assert_eq!(actual.version, version);
    assert_eq!(actual.state, InstanceState::Exited);
    let mut expected = original.spec;
    expected.start_on_boot = true;
    expected.restart_policy = RestartPolicy::Always;
    assert_eq!(
        serde_json::to_value(actual.spec).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(!db.run_authority("app").unwrap().desired_run);
}

#[test]
fn legacy_absence_and_boot_checkbox_do_not_grant_desired_run_or_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    for (id, policy) in [
        ("never", RestartPolicy::No),
        ("failure", RestartPolicy::OnFailure),
        ("always", RestartPolicy::Always),
    ] {
        let instance = fixture(&session, id, policy);
        let authority = session.state().run_authority(id).unwrap();
        assert_eq!(authority.revision, 0);
        assert!(!authority.desired_run);
        assert!(authority.boot_epoch.is_none());
        assert!(
            session
                .state()
                .begin_authorized_run(
                    id,
                    instance.version,
                    0,
                    "first",
                    Some(42),
                    RunStartSource::Recovery
                )
                .is_err()
        );
        assert!(
            session
                .state()
                .begin_authorized_run(
                    id,
                    instance.version,
                    0,
                    "first",
                    Some(42),
                    RunStartSource::Boot
                )
                .is_err()
        );
    }
    assert!(
        session
            .state()
            .recovery_candidates(Some(42), None, 100)
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn manual_stop_before_terminal_ack_survives_owner_death_before_and_after_ready() {
    for ready in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        {
            let session = EngineSession::open(temp.path()).unwrap();
            let created = fixture(&session, "app", RestartPolicy::Always);
            session
                .state()
                .begin_authorized_run(
                    "app",
                    created.version,
                    0,
                    "first",
                    Some(42),
                    RunStartSource::Manual,
                )
                .unwrap();
            let current = if ready {
                session
                    .state()
                    .ready_authorized_run("app", created.version, "first", &[])
                    .unwrap()
            } else {
                created
            };
            let authority = session.state().run_authority("app").unwrap();
            let stopped = session
                .state()
                .stop_run_intent("app", current.version, authority.revision, Some("first"))
                .unwrap();
            assert!(!stopped.desired_run);
            assert_eq!(
                session.state().instance("app").unwrap().state,
                current.state
            ); // Intent is not fake death.
        }
        let owner = EngineSession::open(temp.path()).unwrap();
        assert_eq!(owner.recovered_instances(), 1);
        assert_eq!(
            owner.state().instance("app").unwrap().state,
            InstanceState::Dead
        );
        assert_eq!(
            owner.state().run_authority("app").unwrap().outcome,
            RunOutcome::Stopped
        );
        assert!(
            owner
                .state()
                .recovery_candidates(Some(42), None, 100)
                .unwrap()
                .items
                .is_empty()
        );
    }
}

#[test]
fn unexpected_owner_loss_requires_same_boot_and_new_dead_identity() {
    let temp = tempfile::tempdir().unwrap();
    {
        let session = EngineSession::open(temp.path()).unwrap();
        running(&session, "app", RestartPolicy::OnFailure);
    }
    let owner = EngineSession::open(temp.path()).unwrap();
    let candidates = owner
        .state()
        .recovery_candidates(Some(42), None, 100)
        .unwrap()
        .items;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].authority.outcome, RunOutcome::Interrupted);
    assert!(
        owner
            .state()
            .recovery_candidates(Some(43), None, 100)
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        owner
            .state()
            .recovery_candidates(None, None, 100)
            .unwrap()
            .items
            .is_empty()
    );
    let prior = &candidates[0];
    assert!(
        owner
            .state()
            .begin_authorized_run(
                "app",
                prior.version,
                prior.authority.revision,
                "second",
                Some(42),
                RunStartSource::Recovery
            )
            .is_err()
    );
    let replacement = owner
        .state()
        .retry_dead_instance("app", prior.version, "replacement")
        .unwrap();
    let copied = owner.state().run_authority("replacement").unwrap();
    assert!(copied.desired_run);
    assert_eq!(copied.boot_epoch, Some(42));
    assert!(copied.revision > prior.authority.revision);
    assert!(owner.state().instance("app").is_err());
    owner
        .state()
        .begin_authorized_run(
            "replacement",
            replacement.version,
            copied.revision,
            "second",
            Some(42),
            RunStartSource::Recovery,
        )
        .unwrap();
    assert!(
        owner
            .state()
            .ready_authorized_run("replacement", replacement.version, "first", &[])
            .is_err()
    );
    owner
        .state()
        .ready_authorized_run("replacement", replacement.version, "second", &[])
        .unwrap();
}

#[test]
fn success_failure_and_explicit_boot_keep_independent_authority() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    for (id, policy, error, eligible) in [
        ("clean", RestartPolicy::OnFailure, None, false),
        ("always", RestartPolicy::Always, None, true),
        (
            "timeout",
            RestartPolicy::OnFailure,
            Some("Service execution timed out"),
            true,
        ),
    ] {
        let current = running(&session, id, policy);
        session
            .state()
            .finish_authorized_run(id, current.version, "first", Some(0), error, false)
            .unwrap();
        assert_eq!(
            session
                .state()
                .recovery_candidates(Some(42), None, 100)
                .unwrap()
                .items
                .iter()
                .any(|candidate| candidate.id == id),
            eligible
        );
    }
    let created = fixture(&session, "boot", RestartPolicy::No);
    let mut spec = created.spec.clone();
    spec.start_on_boot = true;
    let configured = session
        .state()
        .configure_instance("boot", created.version, &spec)
        .unwrap();
    assert!(!session.state().run_authority("boot").unwrap().desired_run);
    session
        .state()
        .begin_authorized_run(
            "boot",
            configured.version,
            0,
            "boot-run",
            Some(43),
            RunStartSource::Boot,
        )
        .unwrap();
    assert_eq!(
        session.state().run_authority("boot").unwrap().boot_epoch,
        Some(43)
    );
    assert!(
        !session
            .state()
            .recovery_candidates(Some(43), None, 100)
            .unwrap()
            .items
            .iter()
            .any(|candidate| candidate.id == "boot")
    );
}

#[test]
fn exact_generation_and_revision_fence_ready_finish_stop_and_ack_retry() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let current = running(&session, "app", RestartPolicy::Always);
    let count = session.state().events_after(0, 100).unwrap().len();
    assert_eq!(
        session
            .state()
            .ready_authorized_run("app", 0, "first", &[])
            .unwrap()
            .version,
        current.version
    );
    assert_eq!(session.state().events_after(0, 100).unwrap().len(), count);
    assert!(
        session
            .state()
            .finish_authorized_run("app", current.version, "other", Some(1), None, false)
            .is_err()
    );
    let authority = session.state().run_authority("app").unwrap();
    assert!(
        session
            .state()
            .stop_run_intent(
                "app",
                current.version,
                authority.revision - 1,
                Some("first")
            )
            .is_err()
    );
    let ended = session
        .state()
        .finish_authorized_run("app", current.version, "first", Some(1), None, false)
        .unwrap();
    let events = session.state().events_after(0, 100).unwrap().len();
    assert_eq!(
        session
            .state()
            .finish_authorized_run("app", current.version, "first", Some(1), None, false)
            .unwrap()
            .version,
        ended.version
    );
    assert_eq!(session.state().events_after(0, 100).unwrap().len(), events);
    assert!(
        session
            .state()
            .ready_authorized_run("app", ended.version, "first", &[])
            .is_err()
    );
}

#[test]
fn stop_all_is_atomic_before_any_worker_signal_and_failed_write_rolls_back_every_intent() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    running(&session, "a", RestartPolicy::Always);
    running(&session, "b", RestartPolicy::Always);
    let raw = rusqlite::Connection::open(temp.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("CREATE TRIGGER refuse_stop BEFORE UPDATE ON instance_automation WHEN NEW.instance_id='b' BEGIN SELECT RAISE(ABORT,'fixture stop failure'); END;").unwrap();
    let before = session.state().events_after(0, 100).unwrap().len();
    assert!(session.state().stop_all_run_intents().is_err());
    assert!(session.state().run_authority("a").unwrap().desired_run);
    assert!(session.state().run_authority("b").unwrap().desired_run);
    assert_eq!(session.state().events_after(0, 100).unwrap().len(), before);
    raw.execute_batch("DROP TRIGGER refuse_stop").unwrap();
    assert_eq!(session.state().stop_all_run_intents().unwrap(), 2);
    assert!(!session.state().run_authority("a").unwrap().desired_run);
    assert!(!session.state().run_authority("b").unwrap().desired_run);
}

#[test]
fn configuration_save_suppresses_completed_authority_including_noop_and_no_to_always() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let current = running(&session, "app", RestartPolicy::No);
    let ended = session
        .state()
        .finish_authorized_run("app", current.version, "first", Some(0), None, false)
        .unwrap();
    assert!(session.state().run_authority("app").unwrap().desired_run);
    let mut spec = ended.spec.clone();
    spec.restart_policy = RestartPolicy::Always;
    let changed = session
        .state()
        .configure_instance("app", ended.version, &spec)
        .unwrap();
    assert!(!session.state().run_authority("app").unwrap().desired_run);
    assert!(
        session
            .state()
            .recovery_candidates(Some(42), None, 100)
            .unwrap()
            .items
            .is_empty()
    );
    session
        .state()
        .begin_authorized_run(
            "app",
            changed.version,
            session.state().run_authority("app").unwrap().revision,
            "second",
            Some(42),
            RunStartSource::Manual,
        )
        .unwrap();
    let again = session
        .state()
        .ready_authorized_run("app", changed.version, "second", &[])
        .unwrap();
    let completed = session
        .state()
        .finish_authorized_run("app", again.version, "second", Some(0), None, false)
        .unwrap();
    let saved = session
        .state()
        .configure_instance("app", completed.version, &completed.spec)
        .unwrap();
    assert_eq!(saved.version, completed.version);
    assert!(!session.state().run_authority("app").unwrap().desired_run);
}

#[test]
fn a_retained_worker_owner_lease_prevents_recovery_and_authority_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    running(&session, "app", RestartPolicy::Always);
    let retained = session.ownership();
    drop(session);
    assert!(EngineSession::open(temp.path()).is_err());
    drop(retained);
    let owner = EngineSession::open(temp.path()).unwrap();
    assert_eq!(owner.recovered_instances(), 1);
    assert_eq!(
        owner
            .state()
            .recovery_candidates(Some(42), None, 100)
            .unwrap()
            .items
            .len(),
        1
    );
}

#[test]
fn recovery_pages_are_bounded_metadata_and_advance_over_ineligible_policy_rows() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    for (id, policy) in [
        ("a", RestartPolicy::No),
        ("b", RestartPolicy::Always),
        ("c", RestartPolicy::Always),
    ] {
        let current = running(&session, id, policy);
        session
            .state()
            .finish_authorized_run(id, current.version, "first", Some(0), None, false)
            .unwrap();
    }
    let first = session
        .state()
        .recovery_candidates(Some(42), None, 1)
        .unwrap();
    assert_eq!(first.scanned, 1);
    assert_eq!(first.skipped, 1);
    assert!(first.items.is_empty());
    assert_eq!(first.next_cursor.as_deref(), Some("a"));
    let second = session
        .state()
        .recovery_candidates(Some(42), first.next_cursor.as_deref(), 1)
        .unwrap();
    assert_eq!(second.items[0].id, "b");
    assert_eq!(second.next_cursor.as_deref(), Some("b"));
    let last = session
        .state()
        .recovery_candidates(Some(42), second.next_cursor.as_deref(), 1)
        .unwrap();
    assert_eq!(last.items[0].id, "c");
    assert!(last.next_cursor.is_none());
    let frame = serde_json::to_string(&second).unwrap();
    assert!(!frame.contains("PRIVATE_FIXTURE"));
    assert!(!frame.contains("spec"));
    assert!(frame.len() < 256 * 1024);
    assert!(
        session
            .state()
            .recovery_candidates(Some(42), None, 101)
            .is_err()
    );
    assert!(
        session
            .state()
            .recovery_candidates(Some(42), Some("../escape"), 1)
            .is_err()
    );
}

#[test]
fn lost_ready_and_terminal_acknowledgements_are_generation_fenced_and_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let current = running(&session, "app", RestartPolicy::Always);
    // Native READY committed, but the common owner still has pre-READY version0.
    let ended = session
        .state()
        .finish_authorized_run("app", 0, "first", Some(1), None, false)
        .unwrap();
    assert_eq!(ended.version, current.version + 1);
    let count = session.state().events_after(0, 100).unwrap().len();
    assert_eq!(
        session
            .state()
            .finish_authorized_run("app", 0, "first", Some(1), None, false)
            .unwrap()
            .version,
        ended.version
    );
    assert_eq!(session.state().events_after(0, 100).unwrap().len(), count);
    assert!(
        session
            .state()
            .finish_authorized_run("app", 0, "other", Some(1), None, false)
            .is_err()
    );
}

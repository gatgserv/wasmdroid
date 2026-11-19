//! Actual SQLite/operator grants and lifecycle metadata, no guest/Android IO.
use std::collections::BTreeMap;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceState},
    session::EngineSession,
    stacks::StackManager,
    state::{
        ApplicationTargetKind, GroupNetworkReview, GroupPermissionChoice, InstanceRecord,
        ManagedRunStart, RunStartSource,
    },
    store::Digest,
};
fn artifact(session: &EngineSession, version: &str) {
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse(format!("wasmer.io/example/granted@{version}"))
                .unwrap(),
            digest: Digest::of(version.as_bytes()),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
}
fn document(web: &str, extra: bool) -> String {
    format!(
        "services:\n  web:\n    image: wasmer.io/example/granted@{web}\n    environment: {{PRIVATE_VALUE: retained-secret}}\n    volumes: [notes:/data]\n  database:\n    image: wasmer.io/example/granted@1.0.0\n{}volumes: {{notes: {{}}}}\n",
        if extra {
            "  queue:\n    image: wasmer.io/example/granted@1.0.0\n"
        } else {
            ""
        }
    )
}
fn fixture() -> (tempfile::TempDir, EngineSession) {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    artifact(&session, "1.0.0");
    artifact(&session, "2.0.0");
    StackManager::new(&session)
        .create("group", &document("1.0.0", false))
        .unwrap();
    (dir, session)
}
fn review(session: &EngineSession) -> GroupNetworkReview {
    let target = session
        .state()
        .application_target(ApplicationTargetKind::Group, "group")
        .unwrap()
        .target;
    session.state().group_network_review(&target).unwrap()
}
fn member(session: &EngineSession, name: &str) -> InstanceRecord {
    let stack = session.state().stack("group").unwrap();
    let at = stack
        .plan
        .startup_order
        .iter()
        .position(|service| service == name)
        .unwrap();
    stack.instances[at].clone()
}
fn grant(session: &EngineSession, internet: bool, lan: bool) -> GroupNetworkReview {
    session
        .state()
        .set_group_network_permissions(&review(session), internet, lan)
        .unwrap()
}
fn start(record: &InstanceRecord, generation: &str) -> ManagedRunStart {
    ManagedRunStart {
        instance_id: record.id.clone(),
        version: record.version,
        generation: generation.into(),
    }
}
fn canonical_specs(session: &EngineSession) -> Vec<String> {
    session
        .state()
        .stack("group")
        .unwrap()
        .instances
        .iter()
        .map(|instance| serde_json::to_string(&instance.spec).unwrap())
        .collect()
}

#[test]
fn all_listed_grant_changes_neither_raw_specs_identity_data_nor_template_export() {
    let (dir, session) = fixture();
    let before = canonical_specs(&session);
    let web = member(&session, "web");
    let profile = session.state().browser_identity(&web.id).unwrap();
    let stack = session.state().stack("group").unwrap();
    let volume = &stack.plan.volumes["notes"].name;
    let path = dir
        .path()
        .join("volumes/data")
        .join(volume)
        .join("sentinel");
    std::fs::write(&path, b"retained bytes").unwrap();
    assert_eq!(
        review(&session).source().internet,
        GroupPermissionChoice::Off
    );
    let saved = grant(&session, true, false);
    assert_eq!(saved.source().internet, GroupPermissionChoice::Allow);
    assert_eq!(saved.source().lan, GroupPermissionChoice::Off);
    assert!(saved.source().version > stack.version);
    assert_eq!(saved.source().grant_revision, 1);
    assert_eq!(canonical_specs(&session), before);
    assert_eq!(session.state().browser_identity(&web.id).unwrap(), profile);
    assert_eq!(member(&session, "web").version, web.version);
    assert_eq!(std::fs::read(path).unwrap(), b"retained bytes");
    assert!(StackManager::new(&session).edit_source("group").is_ok());
    let source = serde_json::to_string(saved.source()).unwrap();
    for private in ["retained-secret", "PRIVATE_VALUE", "/data", "artifact_ref"] {
        assert!(!source.contains(private));
    }
    let effective = session.state().effective_worker_spec(&web.id).unwrap();
    assert!(effective.internet());
    assert!(!effective.lan());
    assert!(!effective.raw_spec().internet_access && !effective.raw_spec().lan_access);
}
#[test]
fn stale_review_and_identical_other_database_are_rejected() {
    let (_dir, session) = fixture();
    let old = review(&session);
    grant(&session, true, true);
    assert!(
        session
            .state()
            .set_group_network_permissions(&old, false, false)
            .is_err()
    );
    let (_other_dir, other) = fixture();
    assert!(
        other
            .state()
            .set_group_network_permissions(&old, true, true)
            .is_err()
    );
}
#[test]
fn failed_later_grant_write_rolls_back_every_row_revision_and_event() {
    let (dir, session) = fixture();
    let before = review(&session);
    let count = session.state().events_after(0, 1000).unwrap().len();
    let raw = rusqlite::Connection::open(dir.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("CREATE TRIGGER fail_web_grant BEFORE INSERT ON group_network_grants WHEN NEW.service='web' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        session
            .state()
            .set_group_network_permissions(&before, true, true)
            .is_err()
    );
    assert_eq!(review(&session).source().grant_revision, 0);
    assert_eq!(review(&session).source().version, before.source().version);
    assert_eq!(session.state().events_after(0, 1000).unwrap().len(), count);
    assert_eq!(
        raw.query_row("SELECT count(*) FROM group_network_grants", [], |row| row
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}
#[test]
fn spec_change_revokes_irreversibly_but_unchanged_sql_write_preserves_grant() {
    let (dir, session) = fixture();
    grant(&session, true, true);
    let web = member(&session, "web");
    let raw = rusqlite::Connection::open(dir.path().join("engine.sqlite")).unwrap();
    raw.execute(
        "UPDATE instances SET spec_json=spec_json WHERE id=?1",
        [&web.id],
    )
    .unwrap();
    assert!(
        session
            .state()
            .effective_worker_spec(&web.id)
            .unwrap()
            .internet()
    );
    assert_eq!(review(&session).source().grant_revision, 1);
    let original = serde_json::to_string(&web.spec).unwrap();
    let mut edited = web.spec;
    edited.args.push("changed".into());
    raw.execute(
        "UPDATE instances SET spec_json=?1,version=version+1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(&edited).unwrap(), web.id],
    )
    .unwrap();
    assert!(
        !session
            .state()
            .effective_worker_spec(&web.id)
            .unwrap()
            .internet()
    );
    assert_eq!(review(&session).source().grant_revision, 2);
    raw.execute(
        "UPDATE instances SET spec_json=?1,version=version+1 WHERE id=?2",
        rusqlite::params![original, web.id],
    )
    .unwrap();
    assert!(
        !session
            .state()
            .effective_worker_spec(&web.id)
            .unwrap()
            .internet()
    );
    assert_eq!(
        review(&session).source().internet,
        GroupPermissionChoice::Mixed
    );
}
#[test]
fn same_spec_dead_retry_retains_grant_for_private_profile_not_old_actual_id() {
    let (_dir, session) = fixture();
    grant(&session, true, true);
    let web = member(&session, "web");
    let identity = session.state().browser_identity(&web.id).unwrap();
    let dead = session
        .state()
        .transition_instance(
            &web.id,
            web.version,
            InstanceState::Dead,
            None,
            Some("fixture"),
        )
        .unwrap();
    let group = session.state().stack("group").unwrap();
    session
        .state()
        .retry_stack_member("group", group.version, "web", dead.version, "replacement")
        .unwrap();
    assert_eq!(
        session.state().browser_identity("replacement").unwrap(),
        identity
    );
    assert!(
        session
            .state()
            .effective_worker_spec("replacement")
            .unwrap()
            .internet()
    );
    assert!(session.state().effective_worker_spec(&web.id).is_err());
}
#[test]
fn newly_added_and_package_replaced_members_default_off_without_recipe_inheritance() {
    let (_dir, session) = fixture();
    grant(&session, true, true);
    let old = member(&session, "web");
    let manager = StackManager::new(&session);
    let group = session.state().stack("group").unwrap();
    let preview = manager
        .preview_update("group", group.version, &document("2.0.0", true))
        .unwrap();
    manager.update_plan(&preview).unwrap();
    let web = member(&session, "web");
    let queue = member(&session, "queue");
    let db = member(&session, "database");
    assert_ne!(web.id, old.id);
    assert!(
        !session
            .state()
            .effective_worker_spec(&web.id)
            .unwrap()
            .internet()
    );
    assert!(
        !session
            .state()
            .effective_worker_spec(&queue.id)
            .unwrap()
            .internet()
    );
    assert!(
        session
            .state()
            .effective_worker_spec(&db.id)
            .unwrap()
            .internet()
    );
    assert_eq!(
        review(&session).source().internet,
        GroupPermissionChoice::Mixed
    );
}
#[test]
fn no_op_preview_and_export_do_not_discard_grants_or_relax_plan_positive_guard() {
    let (_dir, session) = fixture();
    grant(&session, true, true);
    let manager = StackManager::new(&session);
    let current = session.state().stack("group").unwrap();
    let source = manager.edit_source("group").unwrap();
    let preview = manager
        .preview_update("group", current.version, &source.document)
        .unwrap();
    let unchanged = manager.update_plan(&preview).unwrap();
    assert_eq!(unchanged.version, current.version);
    assert_eq!(
        review(&session).source().internet,
        GroupPermissionChoice::Allow
    );
    let mut invalid = manager.preview("other", &document("1.0.0", false)).unwrap();
    invalid.services.get_mut("web").unwrap().spec.lan_access = true;
    assert!(manager.create_plan(&invalid).is_err());
}
#[test]
fn created_starting_authority_and_old_stop_snapshot_fence_permission_save() {
    let (_dir, session) = fixture();
    grant(&session, true, true);
    let captured = review(&session);
    let web = member(&session, "web");
    let authorities = session
        .state()
        .begin_managed_runs(&captured, &[start(&web, "generation")])
        .unwrap();
    assert_eq!(member(&session, "web").version, web.version);
    assert!(!review(&session).source().can_edit);
    assert!(
        session
            .state()
            .set_group_network_permissions(&captured, false, false)
            .is_err()
    );
    assert!(
        session
            .state()
            .set_group_network_permissions(&review(&session), false, false)
            .is_err()
    );
    let old = review(&session);
    session
        .state()
        .stop_run_intent(
            &web.id,
            web.version,
            authorities[0].revision,
            Some("generation"),
        )
        .unwrap();
    assert!(
        session
            .state()
            .begin_managed_runs(
                &old,
                &[start(&member(&session, "database"), "database-generation")]
            )
            .is_err()
    );
}
#[test]
fn managed_manual_batch_is_atomic_and_standalone_managed_rejection_stays() {
    let (_dir, session) = fixture();
    let captured = review(&session);
    let web = member(&session, "web");
    let db = member(&session, "database");
    let mut stale = start(&db, "database-generation");
    stale.version += 1;
    assert!(
        session
            .state()
            .begin_managed_runs(&captured, &[start(&web, "web-generation"), stale])
            .is_err()
    );
    assert_eq!(session.state().run_authority(&web.id).unwrap().revision, 0);
    assert!(
        session
            .state()
            .begin_authorized_run(
                &web.id,
                web.version,
                0,
                "generation",
                None,
                RunStartSource::Manual
            )
            .is_err()
    );
    let admitted = session
        .state()
        .begin_managed_runs(
            &captured,
            &[
                start(&web, "web-generation"),
                start(&db, "database-generation"),
            ],
        )
        .unwrap();
    assert_eq!(admitted.len(), 2);
    assert!(
        admitted
            .iter()
            .all(|authority| authority.desired_run && authority.boot_epoch.is_none())
    );
}
#[test]
fn later_manual_sql_failure_rolls_back_first_members_starting_authority() {
    let (dir, session) = fixture();
    let web = member(&session, "web");
    let db = member(&session, "database");
    let raw = rusqlite::Connection::open(dir.path().join("engine.sqlite")).unwrap();
    raw.execute_batch(&format!("CREATE TRIGGER fail_second_start BEFORE INSERT ON instance_automation WHEN NEW.instance_id='{}' BEGIN SELECT RAISE(ABORT,'fixture'); END;",db.id)).unwrap();
    assert!(
        session
            .state()
            .begin_managed_runs(
                &review(&session),
                &[start(&web, "web-generation"), start(&db, "db-generation")]
            )
            .is_err()
    );
    assert_eq!(session.state().run_authority(&web.id).unwrap().revision, 0);
}
#[test]
fn effective_starting_proof_accepts_exact_ready_delta_then_refuses_stop() {
    let (_dir, session) = fixture();
    grant(&session, true, false);
    let web = member(&session, "web");
    session
        .state()
        .begin_managed_runs(&review(&session), &[start(&web, "generation")])
        .unwrap();
    let proof = session.state().effective_worker_spec(&web.id).unwrap();
    assert!(proof.internet());
    assert!(!proof.raw_spec().internet_access);
    proof
        .validate_starting(session.state(), "generation")
        .unwrap();
    let running = session
        .state()
        .ready_authorized_run(&web.id, web.version, "generation", &[])
        .unwrap();
    proof
        .validate_running(session.state(), "generation")
        .unwrap();
    assert!(
        proof
            .validate_starting(session.state(), "generation")
            .is_err()
    );
    let running_proof = session.state().effective_worker_spec(&web.id).unwrap();
    running_proof
        .validate_running(session.state(), "generation")
        .unwrap();
    let authority = session.state().run_authority(&web.id).unwrap();
    session
        .state()
        .stop_run_intent(
            &web.id,
            running.version,
            authority.revision,
            Some("generation"),
        )
        .unwrap();
    assert!(
        proof
            .validate_running(session.state(), "generation")
            .is_err()
    );
    assert!(
        running_proof
            .validate_running(session.state(), "generation")
            .is_err()
    );
}
#[test]
fn needed_subset_can_start_while_an_actual_other_member_is_running() {
    let (_dir, session) = fixture();
    grant(&session, true, true);
    let web = member(&session, "web");
    session
        .state()
        .begin_managed_runs(&review(&session), &[start(&web, "web-generation")])
        .unwrap();
    session
        .state()
        .ready_authorized_run(&web.id, web.version, "web-generation", &[])
        .unwrap();
    let db = member(&session, "database");
    let result = session
        .state()
        .begin_managed_runs(&review(&session), &[start(&db, "db-generation")])
        .unwrap();
    assert_eq!(result.len(), 1);
    session
        .state()
        .effective_worker_spec(&db.id)
        .unwrap()
        .validate_starting(session.state(), "db-generation")
        .unwrap();
}
#[test]
fn cold_recovery_closes_manual_desire_but_keeps_operator_grant_and_private_profile() {
    let (dir, session) = fixture();
    grant(&session, true, true);
    let web = member(&session, "web");
    let profile = session.state().browser_identity(&web.id).unwrap();
    session
        .state()
        .begin_managed_runs(&review(&session), &[start(&web, "generation")])
        .unwrap();
    let proof = session.state().effective_worker_spec(&web.id).unwrap();
    drop(session);
    let recovered = EngineSession::open(dir.path()).unwrap();
    assert_eq!(member(&recovered, "web").state, InstanceState::Dead);
    assert!(
        !recovered
            .state()
            .run_authority(&web.id)
            .unwrap()
            .desired_run
    );
    assert!(
        proof
            .validate_starting(recovered.state(), "generation")
            .is_err()
    );
    assert!(
        recovered
            .state()
            .effective_worker_spec(&web.id)
            .unwrap()
            .internet()
    );
    assert_eq!(
        recovered.state().browser_identity(&web.id).unwrap(),
        profile
    );
}
#[test]
fn new_group_birth_cannot_use_old_review_even_with_same_name_and_versions() {
    let (_dir, session) = fixture();
    let old = review(&session);
    session
        .state()
        .remove_stack("group", old.source().version)
        .unwrap();
    StackManager::new(&session)
        .create("group", &document("1.0.0", false))
        .unwrap();
    assert!(
        session
            .state()
            .set_group_network_permissions(&old, true, true)
            .is_err()
    );
    assert_eq!(
        review(&session).source().internet,
        GroupPermissionChoice::Off
    );
}

#[test]
fn created_starting_member_blocks_native_group_remove_and_update_before_new_volumes() {
    let (dir, session) = fixture();
    grant(&session, true, true);
    let web = member(&session, "web");
    let manager = StackManager::new(&session);
    let group = session.state().stack("group").unwrap();
    let changed =
        document("1.0.0", false).replace("volumes: {notes: {}}", "volumes: {notes: {}, extra: {}}");
    let preview = manager
        .preview_update("group", group.version, &changed)
        .unwrap();
    let extra = preview.plan.volumes["extra"].name.clone();
    session
        .state()
        .begin_managed_runs(&review(&session), &[start(&web, "generation")])
        .unwrap();
    assert!(
        session
            .state()
            .remove_stack("group", group.version)
            .is_err()
    );
    assert!(manager.update_plan(&preview).is_err());
    assert!(session.state().volume(&extra).is_err());
    assert!(!dir.path().join("volumes/data").join(extra).exists());
}
#[test]
fn actual_restored_mount_rebind_revokes_only_affected_member_and_preserves_datasets() {
    let (dir, session) = fixture();
    grant(&session, true, true);
    let web = member(&session, "web");
    let old = session.state().stack("group").unwrap().plan.volumes["notes"]
        .name
        .clone();
    let manager = wasmdroid_engine::volumes::VolumeManager::open(&session).unwrap();
    manager.create("restored", BTreeMap::new()).unwrap();
    let original = dir.path().join("volumes/data").join(&old).join("old");
    let restored = dir.path().join("volumes/data/restored/new");
    std::fs::write(&original, b"original").unwrap();
    std::fs::write(&restored, b"restored").unwrap();
    let changed = session
        .state()
        .replace_instance_volume(&web.id, web.version, &old, "restored")
        .unwrap();
    assert!(
        !session
            .state()
            .effective_worker_spec(&changed.id)
            .unwrap()
            .internet()
    );
    assert!(
        session
            .state()
            .effective_worker_spec(&member(&session, "database").id)
            .unwrap()
            .internet()
    );
    assert_eq!(std::fs::read(original).unwrap(), b"original");
    assert_eq!(std::fs::read(restored).unwrap(), b"restored");
    assert!(StackManager::new(&session).edit_source("group").is_ok());
}
#[test]
fn effective_grants_remain_independent_across_all_four_operator_choices() {
    let (_dir, session) = fixture();
    for (internet, lan) in [(false, false), (true, false), (false, true), (true, true)] {
        let saved = grant(&session, internet, lan);
        assert_eq!(
            saved.source().internet,
            if internet {
                GroupPermissionChoice::Allow
            } else {
                GroupPermissionChoice::Off
            }
        );
        assert_eq!(
            saved.source().lan,
            if lan {
                GroupPermissionChoice::Allow
            } else {
                GroupPermissionChoice::Off
            }
        );
        for instance in session.state().stack("group").unwrap().instances {
            let actual = session.state().effective_worker_spec(&instance.id).unwrap();
            assert_eq!((actual.internet(), actual.lan()), (internet, lan));
            assert!(!actual.raw_spec().internet_access && !actual.raw_spec().lan_access);
        }
    }
}
#[test]
fn v6_migration_defaults_to_off_without_changing_prior_group_data_or_identity() {
    let (dir, session) = fixture();
    let before = canonical_specs(&session);
    let web = member(&session, "web");
    let identity = session.state().browser_identity(&web.id).unwrap();
    drop(session);
    let raw = rusqlite::Connection::open(dir.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("DROP TRIGGER revoke_group_network_on_spec_change; DROP TABLE group_network_grants; DROP TABLE group_network_revisions; PRAGMA user_version=6;").unwrap();
    drop(raw);
    let migrated = EngineSession::open(dir.path()).unwrap();
    assert_eq!(canonical_specs(&migrated), before);
    assert_eq!(
        migrated.state().browser_identity(&web.id).unwrap(),
        identity
    );
    assert_eq!(review(&migrated).source().grant_revision, 0);
    assert_eq!(
        review(&migrated).source().internet,
        GroupPermissionChoice::Off
    );
}
#[test]
fn durable_stop_before_local_running_publication_prevents_callback_invocation() {
    let (_dir, session) = fixture();
    grant(&session, true, true);
    let web = member(&session, "web");
    session
        .state()
        .begin_managed_runs(&review(&session), &[start(&web, "generation")])
        .unwrap();
    let proof = session.state().effective_worker_spec(&web.id).unwrap();
    let running = session
        .state()
        .ready_authorized_run(&web.id, web.version, "generation", &[])
        .unwrap();
    let published = std::cell::Cell::new(false);
    proof
        .with_running(session.state(), "generation", || {
            published.set(true);
            Ok(())
        })
        .unwrap();
    assert!(published.get());
    let authority = session.state().run_authority(&web.id).unwrap();
    session
        .state()
        .stop_run_intent(
            &web.id,
            running.version,
            authority.revision,
            Some("generation"),
        )
        .unwrap();
    published.set(false);
    assert!(
        proof
            .with_running(session.state(), "generation", || {
                published.set(true);
                Ok(())
            })
            .is_err()
    );
    assert!(!published.get());
}

#[test]
fn unbound_withdrawal_invalidates_starting_proof_without_inventing_exit_or_changing_instance() {
    let (_dir, session) = fixture();
    grant(&session, true, true);
    let web = member(&session, "web");
    let before = serde_json::to_value(&web).unwrap();
    session
        .state()
        .begin_managed_runs(&review(&session), &[start(&web, "generation")])
        .unwrap();
    let proof = session.state().effective_worker_spec(&web.id).unwrap();
    proof
        .validate_starting(session.state(), "generation")
        .unwrap();
    let withdrawn = session
        .state()
        .withdraw_unbound_managed_run(&web.id, web.version, "generation")
        .unwrap();
    assert!(!withdrawn.desired_run);
    assert_eq!(
        withdrawn.outcome,
        wasmdroid_engine::state::RunOutcome::Stopped
    );
    assert_eq!(
        serde_json::to_value(member(&session, "web")).unwrap(),
        before
    );
    assert!(member(&session, "web").exit_code.is_none());
    assert!(
        proof
            .validate_starting(session.state(), "generation")
            .is_err()
    );
    assert_eq!(
        session
            .state()
            .withdraw_unbound_managed_run(&web.id, web.version, "generation")
            .unwrap()
            .revision,
        withdrawn.revision
    );
    assert!(
        session
            .state()
            .effective_worker_spec(&web.id)
            .unwrap()
            .internet()
    );
    let next = session
        .state()
        .begin_managed_runs(&review(&session), &[start(&web, "next-generation")])
        .unwrap();
    session
        .state()
        .ready_authorized_run(&web.id, web.version, "next-generation", &[])
        .unwrap();
    assert!(
        session
            .state()
            .withdraw_unbound_managed_run(&web.id, web.version, "next-generation")
            .is_err()
    );
    assert!(next[0].desired_run);
}

//! Real loopback socket + durable metadata regressions. No WebView, guest or
//! Android localStorage execution is implied by these tests.
use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, TcpListener},
    sync::{Arc, Barrier},
};
use wasmdroid_engine::{
    EngineError,
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec, InstanceState, PortBinding},
    network::PortLease,
    session::EngineSession,
    state::{RunStartSource, StateDb},
    store::Digest,
};

fn fixture() -> (tempfile::TempDir, EngineSession) {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse("wasmer.io/example/origin@1.0.0").unwrap(),
            digest: Digest::of(b"origin metadata only"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["serve".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
    (dir, session)
}
fn specification(ports: &[(u16, u16)]) -> InstanceSpec {
    serde_json::from_value(serde_json::json!({
        "artifact_ref":"wasmer.io/example/origin@1.0.0","command":"serve",
        "ports": ports.iter().map(|(guest,host)| serde_json::json!({"instance_port":guest,"host_port":host,"protocol":"tcp"})).collect::<Vec<_>>()
    })).unwrap()
}
fn create(session: &EngineSession, id: &str, ports: &[(u16, u16)]) -> InstanceSpec {
    let spec = specification(ports);
    session.state().create_instance(id, &spec).unwrap();
    spec
}
fn revision(session: &EngineSession, id: &str) -> u64 {
    session.state().instance(id).unwrap().version
}
fn reserve(session: &EngineSession, id: &str) -> Vec<PortLease> {
    let spec = session.state().instance(id).unwrap().spec;
    let capture = session.state().capture_browser_origins(id, &spec).unwrap();
    let leases = PortLease::reserve_browser_origins(session.state(), &capture).unwrap();
    PortLease::validate_browser_origins(session.state(), &capture, &leases).unwrap();
    leases
}
fn actual(spec: &InstanceSpec, leases: &[PortLease]) -> Vec<PortBinding> {
    spec.ports
        .iter()
        .zip(leases)
        .map(|(port, lease)| {
            let mut port = port.clone();
            port.host_port = lease.host_address().unwrap().port();
            port
        })
        .collect()
}
fn assignment_count(dir: &std::path::Path) -> i64 {
    rusqlite::Connection::open(dir.join("engine.sqlite"))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM browser_origin_assignments",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn retained_socket_and_stopped_retry_use_exact_same_numeric_origin() {
    let (_dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0)]);
    assert!(
        session
            .state()
            .browser_origin_history_from_birth("app")
            .unwrap()
    );
    let leases = reserve(&session, "app");
    let address = leases[0].host_address().unwrap();
    assert_eq!(address.ip(), Ipv4Addr::LOCALHOST);
    assert!(TcpListener::bind(address).is_err());
    let record = session.state().instance("app").unwrap();
    let running = session
        .state()
        .start_instance("app", record.version, &actual(&spec, &leases))
        .unwrap();
    session
        .state()
        .transition_instance("app", running.version, InstanceState::Exited, Some(0), None)
        .unwrap();
    drop(leases);
    assert_eq!(reserve(&session, "app")[0].host_address().unwrap(), address);
}

#[test]
fn occupied_saved_port_fails_without_fallback_then_recovers_at_same_address() {
    let (dir, session) = fixture();
    create(&session, "app", &[(8080, 0)]);
    let leases = reserve(&session, "app");
    let address = leases[0].host_address().unwrap();
    drop(leases);
    let occupied = TcpListener::bind(address).unwrap();
    let spec = session.state().instance("app").unwrap().spec;
    let capture = session
        .state()
        .capture_browser_origins("app", &spec)
        .unwrap();
    assert!(PortLease::reserve_browser_origins(session.state(), &capture).is_err());
    assert_eq!(assignment_count(dir.path()), 1);
    drop(occupied);
    assert_eq!(reserve(&session, "app")[0].host_address().unwrap(), address);
}

#[test]
fn removed_app_and_reused_actual_name_cannot_give_old_origin_to_new_profile() {
    let (dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0)]);
    let old_capture = session
        .state()
        .capture_browser_origins("app", &spec)
        .unwrap();
    let leases = reserve(&session, "app");
    let host = leases[0].host_address().unwrap().port();
    drop(leases);
    session
        .state()
        .remove_instance("app", revision(&session, "app"))
        .unwrap();
    create(&session, "app", &[(8080, host)]);
    assert!(PortLease::reserve_browser_origins(session.state(), &old_capture).is_err());
    let capture = session
        .state()
        .capture_browser_origins("app", &specification(&[(8080, host)]))
        .unwrap();
    assert!(PortLease::reserve_browser_origins(session.state(), &capture).is_err());
    assert_eq!(assignment_count(dir.path()), 1);
    assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, host)).is_ok());
}

#[test]
fn dead_replacement_moves_private_identity_and_retains_original_origin() {
    let (_dir, session) = fixture();
    create(&session, "old", &[(8080, 0)]);
    let profile = session.state().browser_identity("old").unwrap();
    let leases = reserve(&session, "old");
    let address = leases[0].host_address().unwrap();
    drop(leases);
    let dead = session
        .state()
        .transition_instance(
            "old",
            revision(&session, "old"),
            InstanceState::Dead,
            None,
            Some("fixture"),
        )
        .unwrap();
    session
        .state()
        .retry_dead_instance("old", dead.version, "replacement")
        .unwrap();
    assert_eq!(
        session.state().browser_identity("replacement").unwrap(),
        profile
    );
    assert_eq!(
        reserve(&session, "replacement")[0].host_address().unwrap(),
        address
    );
}

#[test]
fn explicit_matching_port_is_honored_and_changed_saved_port_is_unsupported() {
    let (_dir, session) = fixture();
    create(&session, "app", &[(8080, 0)]);
    let leases = reserve(&session, "app");
    let host = leases[0].host_address().unwrap().port();
    drop(leases);
    let matching = specification(&[(8080, host)]);
    let record = session
        .state()
        .configure_instance("app", revision(&session, "app"), &matching)
        .unwrap();
    assert_eq!(
        reserve(&session, "app")[0].host_address().unwrap().port(),
        host
    );
    // Keep the original port occupied while asking the OS for a distinct one.
    let original = TcpListener::bind((Ipv4Addr::LOCALHOST, host)).unwrap();
    let other_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let other = other_listener.local_addr().unwrap().port();
    drop(original);
    let changed = specification(&[(8080, other)]);
    session
        .state()
        .configure_instance("app", record.version, &changed)
        .unwrap();
    let capture = session
        .state()
        .capture_browser_origins("app", &changed)
        .unwrap();
    assert!(matches!(
        PortLease::reserve_browser_origins(session.state(), &capture),
        Err(EngineError::Unsupported(_))
    ));
}

#[test]
fn ambiguous_guest_ports_fail_only_scoped_origin_admission() {
    let (_dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0), (8080, 0)]);
    assert!(spec.validate().is_ok());
    assert!(matches!(
        session.state().capture_browser_origins("app", &spec),
        Err(EngineError::Unsupported(_))
    ));
}

#[test]
fn no_forwarding_network_none_worker_keeps_empty_origin_set() {
    let (dir, session) = fixture();
    let mut spec = specification(&[]);
    spec.network = wasmdroid_engine::api::NetworkMode::None;
    session.state().create_instance("app", &spec).unwrap();
    let capture = session
        .state()
        .capture_browser_origins("app", &spec)
        .unwrap();
    let leases = PortLease::reserve_browser_origins(session.state(), &capture).unwrap();
    assert!(leases.is_empty());
    assert_eq!(assignment_count(dir.path()), 0);
}

#[test]
fn later_busy_listener_leaves_no_partial_assignment() {
    let (dir, session) = fixture();
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let spec = create(
        &session,
        "app",
        &[(8080, 0), (8081, occupied.local_addr().unwrap().port())],
    );
    let capture = session
        .state()
        .capture_browser_origins("app", &spec)
        .unwrap();
    assert!(PortLease::reserve_browser_origins(session.state(), &capture).is_err());
    assert_eq!(assignment_count(dir.path()), 0);
}

#[test]
fn sql_failure_rolls_back_complete_listener_set() {
    let (dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0), (8081, 0)]);
    let raw = rusqlite::Connection::open(dir.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("CREATE TRIGGER fail_second_origin BEFORE INSERT ON browser_origin_assignments WHEN NEW.guest_port=8081 BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let capture = session
        .state()
        .capture_browser_origins("app", &spec)
        .unwrap();
    assert!(PortLease::reserve_browser_origins(session.state(), &capture).is_err());
    assert_eq!(assignment_count(dir.path()), 0);
    raw.execute_batch("DROP TRIGGER fail_second_origin;")
        .unwrap();
    assert_eq!(reserve(&session, "app").len(), 2);
}

#[test]
fn concurrent_first_allocations_publish_only_one_assignment_and_retained_listener() {
    let (dir, session) = fixture();
    create(&session, "app", &[(8080, 0)]);
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let connection = session.connection_handle();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let session = connection.connect().unwrap();
            let spec = session.state().instance("app").unwrap().spec;
            let capture = session
                .state()
                .capture_browser_origins("app", &spec)
                .unwrap();
            barrier.wait();
            PortLease::reserve_browser_origins(session.state(), &capture)
        }));
    }
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(assignment_count(dir.path()), 1);
    let address = results
        .iter()
        .find_map(|result| result.as_ref().ok())
        .unwrap()[0]
        .host_address()
        .unwrap();
    assert!(TcpListener::bind(address).is_err());
    drop(results);
    assert_eq!(reserve(&session, "app")[0].host_address().unwrap(), address);
}

#[test]
fn capture_rejects_cross_database_same_names_and_stale_configuration() {
    let (_dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0)]);
    let (_other_dir, other) = fixture();
    create(&other, "app", &[(8080, 0)]);
    let capture = session
        .state()
        .capture_browser_origins("app", &spec)
        .unwrap();
    assert!(PortLease::reserve_browser_origins(other.state(), &capture).is_err());
    let mut edited = spec.clone();
    edited.args.push("different".into());
    session
        .state()
        .configure_instance("app", revision(&session, "app"), &edited)
        .unwrap();
    assert!(PortLease::reserve_browser_origins(session.state(), &capture).is_err());
}

#[test]
fn stop_intent_without_instance_revision_change_invalidates_preparation_capture() {
    let (_dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0)]);
    let instance_revision = revision(&session, "app");
    let authority_revision = session.state().run_authority("app").unwrap().revision;
    let authority = session
        .state()
        .begin_authorized_run(
            "app",
            instance_revision,
            authority_revision,
            "generation",
            None,
            RunStartSource::Manual,
        )
        .unwrap();
    let capture = session
        .state()
        .capture_browser_origins("app", &spec)
        .unwrap();
    session
        .state()
        .stop_run_intent(
            "app",
            instance_revision,
            authority.revision,
            Some("generation"),
        )
        .unwrap();
    assert_eq!(revision(&session, "app"), instance_revision);
    assert!(PortLease::reserve_browser_origins(session.state(), &capture).is_err());
}

#[test]
fn publication_refuses_mapping_disagreeing_with_saved_origin() {
    let (_dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0)]);
    let leases = reserve(&session, "app");
    let mut ports = actual(&spec, &leases);
    let other = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    ports[0].host_port = other.local_addr().unwrap().port();
    let instance_revision = revision(&session, "app");
    assert!(
        session
            .state()
            .start_instance("app", instance_revision, &ports)
            .is_err()
    );
    assert_eq!(
        session.state().instance("app").unwrap().state,
        InstanceState::Created
    );
    assert!(
        session
            .state()
            .start_instance("app", instance_revision, &actual(&spec, &leases))
            .is_ok()
    );
}

#[test]
fn migration_recovers_active_mapping_but_never_claims_missing_legacy_history() {
    let (dir, session) = fixture();
    let spec = create(&session, "active", &[(8080, 0)]);
    create(&session, "stopped", &[(8080, 0)]);
    let lease = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let host = lease.local_addr().unwrap().port();
    let mut ports = spec.ports;
    ports[0].host_port = host;
    session
        .state()
        .start_instance("active", revision(&session, "active"), &ports)
        .unwrap();
    drop(session);
    drop(lease);
    let raw = rusqlite::Connection::open(dir.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("DROP TRIGGER revoke_group_network_on_spec_change; DROP TABLE group_network_grants; DROP TABLE group_network_revisions; DROP TABLE browser_origin_assignments; DROP TABLE browser_origin_profiles; PRAGMA user_version=5;").unwrap();
    drop(raw);
    // StateDb open migrates before the EngineSession recovery deletes active ports.
    let migrated = StateDb::open(dir.path().join("engine.sqlite")).unwrap();
    assert!(
        !migrated
            .browser_origin_history_from_birth("active")
            .unwrap()
    );
    assert!(
        !migrated
            .browser_origin_history_from_birth("stopped")
            .unwrap()
    );
    drop(migrated);
    let recovered = EngineSession::open(dir.path()).unwrap();
    let dead = recovered.state().instance("active").unwrap();
    recovered
        .state()
        .retry_dead_instance("active", dead.version, "replacement")
        .unwrap();
    assert_eq!(
        reserve(&recovered, "replacement")[0]
            .host_address()
            .unwrap()
            .port(),
        host
    );
    assert_eq!(assignment_count(dir.path()), 1);
    create(&recovered, "new", &[(8080, 0)]);
    assert!(
        recovered
            .state()
            .browser_origin_history_from_birth("new")
            .unwrap()
    );
}

#[test]
fn new_assignment_survives_owner_reopen_and_interrupted_run_recovery() {
    let (dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0)]);
    let leases = reserve(&session, "app");
    let address = leases[0].host_address().unwrap();
    session
        .state()
        .start_instance("app", revision(&session, "app"), &actual(&spec, &leases))
        .unwrap();
    drop(leases);
    drop(session);
    let recovered = EngineSession::open(dir.path()).unwrap();
    let dead = recovered.state().instance("app").unwrap();
    assert_eq!(dead.state, InstanceState::Dead);
    recovered
        .state()
        .retry_dead_instance("app", dead.version, "retry")
        .unwrap();
    assert!(
        recovered
            .state()
            .browser_origin_history_from_birth("retry")
            .unwrap()
    );
    assert_eq!(
        reserve(&recovered, "retry")[0].host_address().unwrap(),
        address
    );
}

#[test]
fn publication_cannot_reassign_another_profiles_historical_port() {
    let (_dir, session) = fixture();
    let first = create(&session, "first", &[(8080, 0)]);
    let leases = reserve(&session, "first");
    let ports = actual(&first, &leases);
    drop(leases);
    create(&session, "second", &[(8080, 0)]);
    assert!(
        session
            .state()
            .start_instance("second", revision(&session, "second"), &ports)
            .is_err()
    );
    assert_eq!(
        session.state().instance("second").unwrap().state,
        InstanceState::Created
    );
}

#[test]
fn revalidation_after_claim_observes_changed_record_and_keeps_socket_owned_until_drop() {
    let (_dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0)]);
    let capture = session
        .state()
        .capture_browser_origins("app", &spec)
        .unwrap();
    let leases = PortLease::reserve_browser_origins(session.state(), &capture).unwrap();
    let address = leases[0].host_address().unwrap();
    let mut edited = spec;
    edited.args.push("changed".into());
    session
        .state()
        .configure_instance("app", revision(&session, "app"), &edited)
        .unwrap();
    assert!(PortLease::validate_browser_origins(session.state(), &capture, &leases).is_err());
    assert!(TcpListener::bind(address).is_err());
    drop(leases);
    assert_eq!(reserve(&session, "app")[0].host_address().unwrap(), address);
}

#[test]
fn ambiguous_legacy_active_backfill_fails_transactionally_without_inventing_origins() {
    let (dir, session) = fixture();
    let spec = create(&session, "app", &[(8080, 0), (8080, 0)]);
    let first = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let second = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut ports = spec.ports;
    ports[0].host_port = first.local_addr().unwrap().port();
    ports[1].host_port = second.local_addr().unwrap().port();
    session
        .state()
        .start_instance("app", revision(&session, "app"), &ports)
        .unwrap();
    drop(session);
    let path = dir.path().join("engine.sqlite");
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch("DROP TRIGGER revoke_group_network_on_spec_change; DROP TABLE group_network_grants; DROP TABLE group_network_revisions; DROP TABLE browser_origin_assignments; DROP TABLE browser_origin_profiles; PRAGMA user_version=5;").unwrap();
    assert!(StateDb::open(&path).is_err());
    assert_eq!(
        raw.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        5
    );
    assert_eq!(
        raw.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='browser_origin_assignments'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

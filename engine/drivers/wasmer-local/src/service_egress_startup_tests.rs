//! Actual SQLite authority and one-use startup handoff boundaries. No guest or
//! Android execution is claimed by these metadata/callback fixtures.
use super::*;
use crate::service_egress::{
    BindFuture, DestinationPolicy, EgressLimits, EgressScope, InvalidationFuture, ResolveFuture,
    SelectedNetwork,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wasmdroid_engine::{
    api::{Artifact, ArtifactReference},
    state::RunStartSource,
    store::Digest,
};

struct Platform;
impl WorkerPlatform for Platform {
    fn terminate_after(&self, _seconds: u64) {}
    fn terminate(&self) {}
}
#[derive(Debug)]
struct Deny;
impl DestinationPolicy for Deny {
    fn authorize_name(&self, _scope: &EgressScope, _host: &str) -> Result<()> {
        Err(EngineError::Unsupported(
            "fixture denies external DNS".into(),
        ))
    }
    fn authorize_ip(&self, _scope: &EgressScope, _ip: IpAddr) -> Result<()> {
        Err(EngineError::Unsupported(
            "fixture denies external IP".into(),
        ))
    }
    fn authorize_tcp(&self, _scope: &EgressScope, _peer: SocketAddr) -> Result<()> {
        Err(EngineError::Unsupported(
            "fixture denies external TCP".into(),
        ))
    }
}
struct Selected {
    scope: EgressScope,
    driver: Weak<ServiceDriver>,
    lock_available: AtomicBool,
}
impl std::fmt::Debug for Selected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FixtureSelectedNetwork")
    }
}
impl SelectedNetwork for Selected {
    fn scope(&self) -> &EgressScope {
        &self.scope
    }
    fn current(&self) -> bool {
        let unlocked = self
            .driver
            .upgrade()
            .is_some_and(|driver| driver.state.try_lock().is_ok());
        self.lock_available.store(unlocked, Ordering::SeqCst);
        unlocked
    }
    fn invalidated(&self) -> InvalidationFuture<'_> {
        Box::pin(std::future::pending())
    }
    fn bind<'a>(
        &'a self,
        _scope: &'a EgressScope,
        _socket: &'a tokio::net::TcpSocket,
    ) -> BindFuture<'a> {
        Box::pin(async { panic!("No fixture should dispatch bind") })
    }
    fn resolve<'a>(
        &'a self,
        _scope: &'a EgressScope,
        _host: &'a str,
        _port: Option<u16>,
    ) -> ResolveFuture<'a> {
        Box::pin(async { panic!("No fixture should dispatch DNS") })
    }
}
fn limits() -> EgressLimits {
    EgressLimits::new(1, 1, 1, Duration::from_secs(1), Duration::from_secs(1)).unwrap()
}
fn fixture(
    authorize: bool,
) -> (
    tempfile::TempDir,
    EngineSession,
    InstanceSpec,
    Arc<ServiceDriver>,
) {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let reference = ArtifactReference::parse("wasmer.io/fixture/startup@1.0.0").unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: reference.clone(),
            digest: Digest::of(b"metadata only"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: Default::default(),
        })
        .unwrap();
    let spec: InstanceSpec = serde_json::from_value(
        serde_json::json!({"artifact_ref":reference,"command":"run","lan_access":true}),
    )
    .unwrap();
    let record = session.state().create_instance("owned", &spec).unwrap();
    if authorize {
        session
            .state()
            .begin_authorized_run(
                "owned",
                record.version,
                0,
                "generation",
                Some(1),
                RunStartSource::Manual,
            )
            .unwrap();
    }
    let driver = Arc::new(ServiceDriver::new(
        session.connection().unwrap(),
        "owned".into(),
        "generation".into(),
        Arc::new(Platform),
    ));
    (dir, session, spec, driver)
}
fn selected(driver: &Arc<ServiceDriver>) -> Arc<Selected> {
    Arc::new(Selected {
        scope: driver.capture_egress_scope(1, 1).unwrap(),
        driver: Arc::downgrade(driver),
        lock_available: AtomicBool::new(false),
    })
}

#[test]
fn legacy_create_stays_closed_and_explicit_configured_create_does_not_authorize_start() {
    let (_dir, session, spec, driver) = fixture(true);
    assert!(matches!(
        driver.create("owned", &spec),
        Err(EngineError::Unsupported(_))
    ));
    driver.create_with_egress_spec("owned", &spec).unwrap();
    assert!(matches!(
        driver.start("owned"),
        Err(EngineError::Unsupported(_))
    ));
    assert!(driver.state.lock().unwrap().execution.is_none());
    assert!(driver.start("owned").is_err());
    assert_eq!(
        session.state().instance("owned").unwrap().state,
        wasmdroid_engine::api::InstanceState::Created
    );
}

#[test]
fn native_capture_refuses_absent_wrong_generation_and_stopped_run_authority() {
    let (_dir, session, spec, driver) = fixture(false);
    driver.create_with_egress_spec("owned", &spec).unwrap();
    assert!(driver.capture_egress_scope(1, 1).is_err());
    let record = session.state().instance("owned").unwrap();
    session
        .state()
        .begin_authorized_run(
            "owned",
            record.version,
            0,
            "different-generation",
            Some(1),
            RunStartSource::Manual,
        )
        .unwrap();
    assert!(driver.capture_egress_scope(1, 1).is_err());
    let authority = session.state().run_authority("owned").unwrap();
    session
        .state()
        .stop_run_intent(
            "owned",
            record.version,
            authority.revision,
            authority.generation.as_deref(),
        )
        .unwrap();
    assert!(driver.capture_egress_scope(1, 1).is_err());
}

#[test]
fn prepare_is_one_use_and_injected_current_never_runs_under_driver_control_lock() {
    let (_dir, _session, spec, driver) = fixture(true);
    driver.create_with_egress_spec("owned", &spec).unwrap();
    let selected = selected(&driver);
    driver
        .prepare_egress(selected.clone(), Arc::new(Deny), limits())
        .unwrap();
    assert!(selected.lock_available.load(Ordering::SeqCst));
    assert!(
        driver
            .prepare_egress(selected.clone(), Arc::new(Deny), limits())
            .is_err()
    );
    let prepared = driver.state.lock().unwrap().prepared_egress.take().unwrap();
    assert!(
        driver
            .prepare_egress(selected, Arc::new(Deny), limits())
            .is_err()
    );
    drop(prepared);
}

#[test]
fn durable_stop_during_preparation_invalidates_native_handoff_before_install() {
    let (_dir, session, spec, driver) = fixture(true);
    driver.create_with_egress_spec("owned", &spec).unwrap();
    driver
        .prepare_egress(selected(&driver), Arc::new(Deny), limits())
        .unwrap();
    let prepared = driver.state.lock().unwrap().prepared_egress.take().unwrap();
    prepared.validate(&session, &spec).unwrap();
    let authority = session.state().run_authority("owned").unwrap();
    let record = session.state().instance("owned").unwrap();
    session
        .state()
        .stop_run_intent(
            "owned",
            record.version,
            authority.revision,
            Some("generation"),
        )
        .unwrap();
    assert!(prepared.validate(&session, &spec).is_err());
}

#[test]
fn changed_spec_or_actual_record_revision_and_permanent_stop_refuse_prepared_handoff() {
    let (_dir, session, spec, driver) = fixture(true);
    driver.create_with_egress_spec("owned", &spec).unwrap();
    driver
        .prepare_egress(selected(&driver), Arc::new(Deny), limits())
        .unwrap();
    let prepared = driver.state.lock().unwrap().prepared_egress.take().unwrap();
    let mut changed = spec.clone();
    changed.args.push("reviewed-next-start".into());
    assert!(prepared.validate(&session, &changed).is_err());
    let record = session.state().instance("owned").unwrap();
    // Active authority correctly prevents configuration; do not bypass it to
    // manufacture the fixture. A real metadata revision still fences startup.
    assert!(
        session
            .state()
            .configure_instance("owned", record.version, &changed)
            .is_err()
    );
    session
        .state()
        .ready_authorized_run("owned", record.version, "generation", &[])
        .unwrap();
    assert!(prepared.validate(&session, &spec).is_err());
    driver.kill("owned").unwrap();
    assert!(driver.capture_egress_scope(2, 2).is_err());
    assert!(driver.start("owned").is_err());
}

#[test]
fn configured_preflight_keeps_network_resource_and_default_execution_constraints() {
    let (_dir, _session, spec, _driver) = fixture(true);
    assert!(crate::service_execution::validate_spec_for_egress_configuration(&spec).is_ok());
    assert!(crate::service_execution::validate_spec(&spec).is_err());
    let mut none = spec.clone();
    none.network = wasmdroid_engine::api::NetworkMode::None;
    assert!(crate::service_execution::validate_spec_for_egress_configuration(&none).is_err());
    let mut resource = spec;
    resource.resources.memory_max_bytes = Some(1024);
    assert!(crate::service_execution::validate_spec_for_egress_configuration(&resource).is_err());
}

#[test]
fn retained_startup_preparation_is_not_an_invented_closed_or_ready_receipt() {
    use crate::service_egress_regrant::EgressPhase;
    let (_dir, _session, spec, driver) = fixture(true);
    driver.create_with_egress_spec("owned", &spec).unwrap();
    driver
        .prepare_egress(selected(&driver), Arc::new(Deny), limits())
        .unwrap();
    let status = driver.egress_status().unwrap();
    assert!(matches!(status.phase, EgressPhase::Preparing));
    assert!(!status.closed && !status.ready && !status.uncertain);
    assert!(status.current.is_none());
    driver.kill("owned").unwrap();
    let stopped = driver.egress_status().unwrap();
    assert!(matches!(stopped.phase, EgressPhase::Stopped));
    assert!(!stopped.closed && !stopped.ready);
}

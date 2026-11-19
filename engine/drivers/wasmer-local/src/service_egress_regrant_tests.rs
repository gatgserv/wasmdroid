//! Actual metadata, retained receipt and real TCP observations. Injected
//! captures/policies attest mechanism ordering, never Android routing or guests.
use super::*;
use crate::service_egress::*;
use crate::service_egress_regrant::*;
use std::sync::{
    Condvar,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Notify, Semaphore},
};
use wasmdroid_engine::{
    EngineError, Result as EngineResult,
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec},
    network::lan::LanRevocation,
    session::EngineSession,
    state::RunStartSource,
    store::Digest,
};
const TIME: Duration = Duration::from_secs(3);
struct Block {
    open: Mutex<bool>,
    changed: Condvar,
    started: Notify,
}
impl Block {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            open: Mutex::new(false),
            changed: Condvar::new(),
            started: Notify::new(),
        })
    }
    fn wait(&self) {
        self.started.notify_one();
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}
struct Release(Arc<Block>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct Selected {
    scope: EgressScope,
    lost: LanRevocation,
    bind_gate: Option<Arc<Semaphore>>,
    bind_started: Notify,
    calls: AtomicUsize,
    fail_at: usize,
    block: Option<Arc<Block>>,
}
impl std::fmt::Debug for Selected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FixtureSelected")
    }
}
impl SelectedNetwork for Selected {
    fn scope(&self) -> &EgressScope {
        &self.scope
    }
    fn current(&self) -> bool {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == 3
            && let Some(block) = &self.block
        {
            block.wait();
        }
        !self.lost.is_revoked() && call != self.fail_at
    }
    fn invalidated(&self) -> InvalidationFuture<'_> {
        Box::pin(self.lost.revoked())
    }
    fn bind<'a>(
        &'a self,
        _scope: &'a EgressScope,
        _socket: &'a tokio::net::TcpSocket,
    ) -> BindFuture<'a> {
        Box::pin(async move {
            self.bind_started.notify_one();
            if let Some(gate) = &self.bind_gate {
                gate.acquire().await.unwrap().forget();
            }
            Ok(())
        })
    }
    fn resolve<'a>(
        &'a self,
        _scope: &'a EgressScope,
        _host: &'a str,
        _port: Option<u16>,
    ) -> ResolveFuture<'a> {
        Box::pin(async { Err(EngineError::Unsupported("fixture DNS unused".into())) })
    }
}
#[derive(Debug)]
struct Policy(EgressScope);
impl DestinationPolicy for Policy {
    fn authorize_name(&self, scope: &EgressScope, _host: &str) -> EngineResult<()> {
        if scope == &self.0 {
            Ok(())
        } else {
            Err(denied())
        }
    }
    fn authorize_ip(&self, scope: &EgressScope, _ip: IpAddr) -> EngineResult<()> {
        if scope == &self.0 {
            Ok(())
        } else {
            Err(denied())
        }
    }
    fn authorize_tcp(&self, scope: &EgressScope, _peer: SocketAddr) -> EngineResult<()> {
        if scope == &self.0 {
            Ok(())
        } else {
            Err(denied())
        }
    }
}
fn denied() -> EngineError {
    EngineError::Unsupported("fixture scope differs".into())
}
fn selected(
    scope: EgressScope,
    gate: Option<Arc<Semaphore>>,
    fail_at: usize,
    block: Option<Arc<Block>>,
) -> Arc<Selected> {
    Arc::new(Selected {
        scope,
        lost: LanRevocation::new(),
        bind_gate: gate,
        bind_started: Notify::new(),
        calls: AtomicUsize::new(0),
        fail_at,
        block,
    })
}
fn limits() -> EgressLimits {
    EgressLimits::new(2, 1, 2, TIME, TIME).unwrap()
}
struct Fixture {
    _dir: tempfile::TempDir,
    session: EngineSession,
    spec: InstanceSpec,
    network: ServiceNetwork,
    initial: Arc<ServiceEgress>,
    selected: Arc<Selected>,
    admission: Arc<RegrantAdmission>,
    stop: LanRevocation,
}
fn fixture(ready: bool, gate: Option<Arc<Semaphore>>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let reference = ArtifactReference::parse("wasmer.io/fixture/regrant@1.0.0").unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: reference.clone(),
            digest: Digest::of(b"metadata fixture"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: Default::default(),
        })
        .unwrap();
    let spec: InstanceSpec = serde_json::from_value(
        serde_json::json!({"artifact_ref":reference,"command":"run","internet_access":true}),
    )
    .unwrap();
    let record = session.state().create_instance("owned", &spec).unwrap();
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
    let stop = LanRevocation::new();
    let mut scope = EgressScope::capture(&session, "owned", "generation", &spec, 1, 1).unwrap();
    scope.bind_stop(stop.clone()).unwrap();
    let platform = selected(scope.clone(), gate, usize::MAX, None);
    let initial = ServiceEgress::new(
        platform.clone(),
        Arc::new(Policy(scope)),
        session.ownership(),
        tokio::runtime::Handle::current(),
        limits(),
    )
    .unwrap();
    let network = ServiceNetwork::new(true);
    network
        .install_egress(("owned", "generation", record.version), initial.clone())
        .unwrap();
    let admission = RegrantAdmission::new();
    admission.startup(1).unwrap();
    if ready {
        session
            .state()
            .ready_authorized_run("owned", record.version, "generation", &[])
            .unwrap();
    }
    Fixture {
        _dir: dir,
        session,
        spec,
        network,
        initial,
        selected: platform,
        admission,
        stop,
    }
}
fn context(fixture: &Fixture) -> RegrantContext {
    RegrantContext {
        connection: fixture.session.connection_handle(),
        spec: fixture.spec.clone(),
        previous: fixture.network.egress_manager().unwrap().unwrap(),
        network: fixture.network.clone(),
        executor: tokio::runtime::Handle::current(),
        stop: fixture.stop.clone(),
    }
}
async fn close(fixture: &Fixture) {
    fixture.initial.revoke();
    tokio::time::timeout(TIME, fixture.initial.receipt().wait())
        .await
        .unwrap()
        .unwrap();
}
fn candidate(ticket: &EgressRegrantTicket) -> Arc<Selected> {
    selected(ticket.scope().clone(), None, usize::MAX, None)
}
fn install(ticket: EgressRegrantTicket, platform: Arc<Selected>) -> EngineResult<EgressReceipt> {
    let policy = Arc::new(Policy(ticket.scope().clone()));
    ticket.install(platform, policy, limits())
}
fn direct(manager: &ServiceEgress, network: &ServiceNetwork, peer: SocketAddr) -> Socket {
    // Loopback policy exists only in this explicit private transport fixture.
    manager
        .open(
            network,
            network.reserve((Ipv4Addr::LOCALHOST, 0).into()).unwrap(),
            peer,
            64,
        )
        .unwrap()
}
async fn no_accept(listener: &tokio::net::TcpListener) {
    std::future::poll_fn(|cx| match listener.poll_accept(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("Late unowned connect"),
    })
    .await;
}

#[tokio::test]
async fn committed_ready_is_required_and_cancelled_issued_epochs_are_never_reused() {
    let fixture = fixture(false, None);
    close(&fixture).await;
    assert!(fixture.admission.reserve(context(&fixture), 2).is_err());
    let record = fixture.session.state().instance("owned").unwrap();
    fixture
        .session
        .state()
        .ready_authorized_run("owned", record.version, "generation", &[])
        .unwrap();
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    assert!(ticket.internet());
    assert!(!ticket.lan());
    assert_eq!(ticket.scope().grant_epoch(), 2);
    let control = ticket.control();
    assert!(!control.status().unwrap().closed);
    control.cancel();
    drop(ticket);
    let done = control.wait_closed().await.unwrap();
    assert!(done.closed && done.terminal && !done.admitted);
    let next = fixture.admission.reserve(context(&fixture), 3).unwrap();
    assert_eq!(next.scope().grant_epoch(), 3);
    drop(next);
}

#[tokio::test]
async fn one_pending_ticket_and_native_proof_refuse_caller_field_substitution() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    assert!(fixture.admission.reserve(context(&fixture), 3).is_err());
    let control = ticket.control();
    let fake = EgressScope::new(
        "owned".into(),
        "generation".into(),
        ticket.scope().spec_revision(),
        2,
        2,
    )
    .unwrap();
    assert!(install(ticket, selected(fake, None, usize::MAX, None)).is_err());
    assert!(control.wait_closed().await.unwrap().closed);
    let next = fixture.admission.reserve(context(&fixture), 3).unwrap();
    assert_eq!(next.scope().grant_epoch(), 3);
    drop(next);
}

#[tokio::test]
async fn pending_actual_binder_prevents_regrant_until_true_old_receipt_closed() {
    let gate = Arc::new(Semaphore::new(0));
    let fixture = fixture(true, Some(gate.clone()));
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let _client = direct(
        &fixture.initial,
        &fixture.network,
        listener.local_addr().unwrap(),
    );
    fixture.selected.bind_started.notified().await;
    fixture.initial.revoke();
    assert!(!fixture.initial.drained().unwrap());
    assert!(fixture.admission.reserve(context(&fixture), 2).is_err());
    no_accept(&listener).await;
    gate.add_permits(1);
    fixture.initial.receipt().wait().await.unwrap();
    no_accept(&listener).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    drop(ticket);
}

#[tokio::test]
async fn actual_replacement_transfers_bytes_old_loss_cannot_revoke_new_and_stop_gate_drains_it() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    let control = ticket.control();
    let platform = candidate(&ticket);
    let receipt = install(ticket, platform).unwrap();
    let active = control.status().unwrap();
    assert!(active.admitted && active.terminal && !active.closed);
    let manager = fixture.network.egress_manager().unwrap().unwrap();
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = direct(&manager, &fixture.network, listener.local_addr().unwrap());
    std::future::poll_fn(|cx| client.poll_write_ready(cx))
        .await
        .unwrap();
    let (mut server, _) = listener.accept().await.unwrap();
    let mut client: Box<dyn VirtualTcpSocket + Sync> = Box::new(client);
    fixture.selected.lost.revoke();
    client
        .write_all(b"new generation-owned capture")
        .await
        .unwrap();
    let mut bytes = [0; 28];
    server.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"new generation-owned capture");
    fixture.stop.revoke();
    assert!(
        tokio::time::timeout(TIME, receipt.wait())
            .await
            .unwrap()
            .unwrap()
            .closed
    );
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(TIME, server.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert!(control.status().unwrap().closed);
    assert!(fixture.admission.reserve(context(&fixture), 3).is_err());
}

#[tokio::test]
async fn durable_stop_after_reservation_refuses_manager_admission() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    let control = ticket.control();
    let platform = candidate(&ticket);
    let record = fixture.session.state().instance("owned").unwrap();
    let authority = fixture.session.state().run_authority("owned").unwrap();
    fixture
        .session
        .state()
        .stop_run_intent(
            "owned",
            record.version,
            authority.revision,
            Some("generation"),
        )
        .unwrap();
    assert!(install(ticket, platform).is_err());
    let done = control.wait_closed().await.unwrap();
    assert!(done.closed && !done.admitted);
    assert!(fixture.admission.reserve(context(&fixture), 3).is_err());
}

#[tokio::test]
async fn failure_after_candidate_creation_retains_receipt_until_actual_cleanup() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    let control = ticket.control();
    let platform = selected(ticket.scope().clone(), None, 3, None);
    assert!(install(ticket, platform).is_err());
    assert!(!control.status().unwrap().terminal);
    assert!(!control.status().unwrap().closed);
    let done = tokio::time::timeout(TIME, control.wait_closed())
        .await
        .unwrap()
        .unwrap();
    assert!(done.closed && done.report.unwrap().closed && !done.admitted);
    let next = fixture.admission.reserve(context(&fixture), 3).unwrap();
    assert_eq!(next.scope().grant_epoch(), 3);
    drop(next);
}

#[tokio::test]
async fn cancelled_dispatch_observer_does_not_release_inflight_native_candidate() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    let control = ticket.control();
    let block = Block::new();
    let _release = Release(block.clone());
    let platform = selected(
        ticket.scope().clone(),
        None,
        usize::MAX,
        Some(block.clone()),
    );
    let observer = tokio::spawn(async move {
        tokio::task::spawn_blocking(move || install(ticket, platform)).await
    });
    block.started.notified().await;
    observer.abort();
    let _ = observer.await;
    control.cancel();
    assert!(!control.status().unwrap().closed);
    assert!(fixture.admission.reserve(context(&fixture), 3).is_err());
    block.release();
    let done = tokio::time::timeout(TIME, control.wait_closed())
        .await
        .unwrap()
        .unwrap();
    assert!(done.closed && !done.admitted);
    let next = fixture.admission.reserve(context(&fixture), 3).unwrap();
    drop(next);
}

#[tokio::test]
async fn failed_owner_on_closed_executor_is_uncertain_and_never_fabricates_closed() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let handle = std::thread::spawn(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        drop(runtime);
        handle
    })
    .join()
    .unwrap();
    let mut closed_context = context(&fixture);
    closed_context.executor = handle;
    let ticket = fixture.admission.reserve(closed_context, 2).unwrap();
    let control = ticket.control();
    let platform = candidate(&ticket);
    let _ = install(ticket, platform);
    let status = tokio::time::timeout(TIME, async {
        loop {
            let status = control.status().unwrap();
            if status.uncertain {
                break status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!status.closed);
    assert!(fixture.admission.reserve(context(&fixture), 3).is_err());
    assert!(fixture.admission.poll_completion(|| Ok(Some(()))).is_err());
}

#[tokio::test]
async fn permanent_stop_before_install_and_wire_epoch_overflow_fail_closed() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    assert!(
        fixture
            .admission
            .reserve(context(&fixture), u64::MAX)
            .is_err()
    );
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    let control = ticket.control();
    let platform = candidate(&ticket);
    fixture.stop.revoke();
    assert!(install(ticket, platform).is_err());
    assert!(control.wait_closed().await.unwrap().closed);
    assert!(fixture.admission.reserve(context(&fixture), 3).is_err());
}

#[tokio::test]
async fn durable_stop_committed_during_external_preparation_wins_final_publication() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    let control = ticket.control();
    let block = Block::new();
    let _release = Release(block.clone());
    let platform = selected(
        ticket.scope().clone(),
        None,
        usize::MAX,
        Some(block.clone()),
    );
    let dispatch = tokio::task::spawn_blocking(move || install(ticket, platform));
    block.started.notified().await;
    let record = fixture.session.state().instance("owned").unwrap();
    let authority = fixture.session.state().run_authority("owned").unwrap();
    fixture
        .session
        .state()
        .stop_run_intent(
            "owned",
            record.version,
            authority.revision,
            Some("generation"),
        )
        .unwrap();
    block.release();
    assert!(dispatch.await.unwrap().is_err());
    let done = tokio::time::timeout(TIME, control.wait_closed())
        .await
        .unwrap()
        .unwrap();
    assert!(done.closed && !done.admitted);
    assert!(Arc::ptr_eq(
        &fixture.network.egress_manager().unwrap().unwrap(),
        &fixture.initial
    ));
}

#[tokio::test]
async fn final_cas_rejects_intervening_closed_manager_even_when_new_epoch_would_be_higher() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    drop(fixture.admission.reserve(context(&fixture), 2).unwrap());
    let ticket = fixture.admission.reserve(context(&fixture), 3).unwrap();
    let control = ticket.control();
    let platform = candidate(&ticket);
    // A different trusted admission wins the slot while this ticket is being
    // prepared. The next ticket's numeric epoch3 would exceed this epoch2;
    // only exact previous-manager identity rejects the stale CAS.
    let scope = fixture
        .initial
        .scope()
        .capture_running(&fixture.session, &fixture.spec, 2, 2)
        .unwrap();
    let version = scope.spec_revision();
    let intervening = ServiceEgress::new(
        selected(scope.clone(), None, usize::MAX, None),
        Arc::new(Policy(scope)),
        fixture.session.ownership(),
        tokio::runtime::Handle::current(),
        limits(),
    )
    .unwrap();
    fixture
        .network
        .install_egress(("owned", "generation", version), intervening.clone())
        .unwrap();
    intervening.revoke();
    intervening.receipt().wait().await.unwrap();
    assert!(install(ticket, platform).is_err());
    let done = tokio::time::timeout(TIME, control.wait_closed())
        .await
        .unwrap()
        .unwrap();
    assert!(done.closed && !done.admitted);
    assert!(Arc::ptr_eq(
        &fixture.network.egress_manager().unwrap().unwrap(),
        &intervening
    ));
    let next = fixture.admission.reserve(context(&fixture), 4).unwrap();
    assert_eq!(next.scope().grant_epoch(), 4);
    drop(next);
}

#[tokio::test]
async fn terminal_completion_waits_for_candidate_cleanup_and_fences_future_issuance() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    let control = ticket.control();
    let platform = selected(ticket.scope().clone(), None, 3, None);
    assert!(install(ticket, platform).is_err());
    // The failed candidate's real owner has not yet run on this current-thread
    // executor. Neither poll nor remove may publish a native terminal result.
    assert!(
        fixture
            .admission
            .poll_completion::<()>(|| panic!("Execution polled before actual candidate cleanup"))
            .unwrap()
            .is_none()
    );
    let done = tokio::time::timeout(TIME, control.wait_closed())
        .await
        .unwrap()
        .unwrap();
    assert!(done.closed);
    assert_eq!(
        fixture.admission.poll_completion(|| Ok(Some(42))).unwrap(),
        Some(42)
    );
    // Durable metadata intentionally still says Running in this fixture: the
    // local final-reserve fence must independently reject a new admission.
    assert!(fixture.admission.reserve(context(&fixture), 3).is_err());
}

#[tokio::test]
async fn current_only_interrupt_cancels_pending_ticket_without_forbidding_fresh_capture() {
    let fixture = fixture(true, None);
    close(&fixture).await;
    let ticket = fixture.admission.reserve(context(&fixture), 2).unwrap();
    let control = ticket.control();
    let platform = candidate(&ticket);
    fixture.admission.cancel_pending();
    assert!(install(ticket, platform).is_err());
    assert!(control.wait_closed().await.unwrap().closed);
    assert!(!fixture.stop.is_revoked());
    let next = fixture.admission.reserve(context(&fixture), 3).unwrap();
    assert_eq!(next.scope().grant_epoch(), 3);
    drop(next);
}

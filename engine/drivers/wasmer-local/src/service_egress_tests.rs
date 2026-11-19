//! Actual retained TCP endpoints and private namespace arbitration. Injected
//! bind/DNS fixtures attest ordering only, never Android Network/Internet/LAN.
use super::*;
use crate::service_egress::*;
use std::{
    future::poll_fn,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Notify, Semaphore, oneshot},
};
use wasmdroid_engine::{
    Result as EngineResult, network::lan::LanRevocation, session::EngineSession,
};

const DEADLINE: Duration = Duration::from_secs(3);
#[derive(Debug)]
struct Policy {
    scope: EgressScope,
    tcp: bool,
    dns: bool,
    allowed: IpAddr,
    tcp_calls: AtomicUsize,
}
impl DestinationPolicy for Policy {
    fn authorize_name(&self, scope: &EgressScope, _host: &str) -> EngineResult<()> {
        if *scope == self.scope && self.dns {
            Ok(())
        } else {
            Err(wasmdroid_engine::EngineError::Unsupported(
                "fixture DNS denied".into(),
            ))
        }
    }
    fn authorize_ip(&self, scope: &EgressScope, address: IpAddr) -> EngineResult<()> {
        if *scope == self.scope && address == self.allowed {
            Ok(())
        } else {
            Err(wasmdroid_engine::EngineError::Unsupported(
                "fixture address denied".into(),
            ))
        }
    }
    fn authorize_tcp(&self, scope: &EgressScope, _peer: SocketAddr) -> EngineResult<()> {
        self.tcp_calls.fetch_add(1, Ordering::SeqCst);
        if *scope == self.scope && self.tcp {
            Ok(())
        } else {
            Err(wasmdroid_engine::EngineError::Unsupported(
                "fixture TCP denied".into(),
            ))
        }
    }
}
struct Live<'a>(&'a AtomicBool);
impl Drop for Live<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
struct Platform {
    scope: EgressScope,
    lost: LanRevocation,
    bind_started: Notify,
    bind_calls: AtomicUsize,
    bind_gate: Option<Arc<Semaphore>>,
    binder_live: AtomicBool,
    bind_denied: bool,
    dns_started: Notify,
    dns_calls: AtomicUsize,
    dns_live: AtomicBool,
    answer: Mutex<Option<oneshot::Receiver<EngineResult<EgressResolution>>>>,
}
impl std::fmt::Debug for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExplicitTestPlatform")
    }
}
impl SelectedNetwork for Platform {
    fn scope(&self) -> &EgressScope {
        &self.scope
    }
    fn current(&self) -> bool {
        !self.lost.is_revoked()
    }
    fn invalidated(&self) -> InvalidationFuture<'_> {
        Box::pin(self.lost.revoked())
    }
    fn bind<'a>(
        &'a self,
        scope: &'a EgressScope,
        _socket: &'a tokio::net::TcpSocket,
    ) -> BindFuture<'a> {
        Box::pin(async move {
            assert_eq!(*scope, self.scope);
            self.binder_live.store(true, Ordering::SeqCst);
            let _live = Live(&self.binder_live);
            self.bind_calls.fetch_add(1, Ordering::SeqCst);
            self.bind_started.notify_one();
            if let Some(gate) = &self.bind_gate {
                gate.acquire().await.unwrap().forget();
            }
            if self.bind_denied {
                return Err(wasmdroid_engine::EngineError::Unsupported(
                    "fixture binder denied".into(),
                ));
            }
            Ok(())
        })
    }
    fn resolve<'a>(
        &'a self,
        scope: &'a EgressScope,
        _host: &'a str,
        _port: Option<u16>,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            assert_eq!(*scope, self.scope);
            self.dns_live.store(true, Ordering::SeqCst);
            let _live = Live(&self.dns_live);
            self.dns_calls.fetch_add(1, Ordering::SeqCst);
            self.dns_started.notify_one();
            let answer = self.answer.lock().unwrap().take().unwrap();
            answer.await.unwrap()
        })
    }
}
fn scope() -> EgressScope {
    EgressScope::new("owned".into(), "generation".into(), 7, 1, 2).unwrap()
}
fn platform(
    scope: EgressScope,
    bind_gate: Option<Arc<Semaphore>>,
    bind_denied: bool,
) -> Arc<Platform> {
    Arc::new(Platform {
        scope,
        lost: LanRevocation::new(),
        bind_started: Notify::new(),
        bind_calls: AtomicUsize::new(0),
        bind_gate,
        binder_live: AtomicBool::new(false),
        bind_denied,
        dns_started: Notify::new(),
        dns_calls: AtomicUsize::new(0),
        dns_live: AtomicBool::new(false),
        answer: Mutex::new(None),
    })
}
fn policy(scope: EgressScope, tcp: bool, dns: bool) -> Arc<Policy> {
    Arc::new(Policy {
        scope,
        tcp,
        dns,
        allowed: "203.0.113.7".parse().unwrap(),
        tcp_calls: AtomicUsize::new(0),
    })
}
fn limits(clients: usize, resolvers: usize) -> EgressLimits {
    EgressLimits::new(clients, resolvers, 4, DEADLINE, DEADLINE).unwrap()
}
fn manager(
    session: &EngineSession,
    platform: Arc<Platform>,
    policy: Arc<Policy>,
    limits: EgressLimits,
) -> Arc<ServiceEgress> {
    ServiceEgress::new(
        platform,
        policy,
        session.ownership(),
        tokio::runtime::Handle::current(),
        limits,
    )
    .unwrap()
}
fn direct(manager: &ServiceEgress, namespace: &ServiceNetwork, peer: SocketAddr) -> Socket {
    // Explicit loopback authorization exists ONLY in this injected transport
    // fixture; normal ServiceNetwork guest routing is separately tested below.
    manager
        .open(
            namespace,
            namespace.reserve((Ipv4Addr::LOCALHOST, 0).into()).unwrap(),
            peer,
            64,
        )
        .unwrap()
}
async fn ready(client: &mut Socket) {
    tokio::time::timeout(DEADLINE, poll_fn(|cx| client.poll_write_ready(cx)))
        .await
        .unwrap()
        .unwrap();
}
async fn no_accept(listener: &tokio::net::TcpListener) {
    poll_fn(|cx| match listener.poll_accept(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("Forbidden TCP connection reached listener"),
    })
    .await;
}

#[tokio::test]
async fn real_tcp_bridge_transfers_bytes_and_preserves_normal_half_close() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let platform = platform(scope.clone(), None, false);
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope, true, true),
        limits(2, 1),
    );
    let namespace = ServiceNetwork::new(true);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = direct(&manager, &namespace, listener.local_addr().unwrap());
    ready(&mut client).await;
    let (mut server, _) = listener.accept().await.unwrap();
    assert_eq!(platform.bind_calls.load(Ordering::SeqCst), 1);
    let mut client: Box<dyn VirtualTcpSocket + Sync> = Box::new(client);
    client.write_all(b"request").await.unwrap();
    client.shutdown().await.unwrap();
    let mut request = Vec::new();
    tokio::time::timeout(DEADLINE, server.read_to_end(&mut request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(request, b"request");
    server.write_all(b"reply after EOF").await.unwrap();
    server.shutdown().await.unwrap();
    let mut reply = Vec::new();
    tokio::time::timeout(DEADLINE, client.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply, b"reply after EOF");
    drop(client);
    drop(server);
    let terminal = tokio::time::timeout(DEADLINE, manager.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(terminal.closed && !terminal.uncertain);
}

#[tokio::test]
async fn destination_denial_or_reconstructed_scope_never_calls_binder_or_connects() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let selected = scope();
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    for granted in [false, true] {
        let platform = platform(selected.clone(), None, false);
        let policy_scope = if granted { scope() } else { selected.clone() }; // Matching fields, different private capture.
        assert!(policy_scope != selected || !granted);
        let manager = manager(
            &session,
            platform.clone(),
            policy(policy_scope, granted, false),
            limits(1, 1),
        );
        let namespace = ServiceNetwork::new(true);
        let binding = namespace.reserve((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        assert!(
            manager
                .open(&namespace, binding, listener.local_addr().unwrap(), 64)
                .is_err()
        );
        assert_eq!(platform.bind_calls.load(Ordering::SeqCst), 0);
        no_accept(&listener).await;
        manager.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn binder_must_finish_before_connect_and_revoke_retains_pending_bind_ownership() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let gate = Arc::new(Semaphore::new(0));
    let platform = platform(scope.clone(), Some(gate.clone()), false);
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope, true, false),
        limits(1, 1),
    );
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let namespace = ServiceNetwork::new(true);
    let mut client = direct(&manager, &namespace, listener.local_addr().unwrap());
    platform.bind_started.notified().await;
    no_accept(&listener).await;
    manager.revoke();
    assert!(!manager.drained().unwrap());
    assert!(platform.binder_live.load(Ordering::SeqCst));
    let pending = tokio::time::timeout(DEADLINE, poll_fn(|cx| client.poll_write_ready(cx)))
        .await
        .unwrap();
    assert!(pending.is_err());
    gate.add_permits(1);
    let terminal = tokio::time::timeout(DEADLINE, manager.receipt().wait())
        .await
        .unwrap()
        .unwrap();
    assert!(terminal.closed);
    assert!(!platform.binder_live.load(Ordering::SeqCst));
    no_accept(&listener).await;
}

#[tokio::test]
async fn failed_platform_binding_cannot_fall_back_to_an_ambient_connect() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let platform = platform(scope.clone(), None, true);
    let manager = manager(&session, platform, policy(scope, true, false), limits(1, 1));
    let namespace = ServiceNetwork::new(true);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = direct(&manager, &namespace, listener.local_addr().unwrap());
    assert!(
        tokio::time::timeout(DEADLINE, poll_fn(|cx| client.poll_write_ready(cx)))
            .await
            .unwrap()
            .is_err()
    );
    no_accept(&listener).await;
    let result = manager.shutdown().await.unwrap();
    assert!(result.closed);
    assert_eq!(result.failed, 1);
}

#[tokio::test]
async fn client_bound_rejects_extra_work_without_discarding_the_first_capture() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let gate = Arc::new(Semaphore::new(0));
    let platform = platform(scope.clone(), Some(gate.clone()), false);
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope, true, false),
        limits(1, 1),
    );
    let namespace = ServiceNetwork::new(true);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let _first = direct(&manager, &namespace, listener.local_addr().unwrap());
    platform.bind_started.notified().await;
    let binding = namespace.reserve((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
    assert!(matches!(
        manager.open(&namespace, binding, listener.local_addr().unwrap(), 64),
        Err(NetworkError::TooManyOpenFiles)
    ));
    assert_eq!(manager.receipt().status().unwrap().tcp_active, 1);
    assert_eq!(platform.bind_calls.load(Ordering::SeqCst), 1);
    manager.revoke();
    gate.add_permits(1);
    manager.receipt().wait().await.unwrap();
    no_accept(&listener).await;
}

#[tokio::test]
async fn abandoning_dns_observer_keeps_slot_until_actual_provider_finishes_and_discards_late_answer()
 {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let platform = platform(scope.clone(), None, false);
    let (answer, receive) = oneshot::channel();
    *platform.answer.lock().unwrap() = Some(receive);
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope.clone(), true, true),
        limits(1, 1),
    );
    let observed = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.resolve("fixture.invalid", Some(443)).await })
    };
    platform.dns_started.notified().await;
    observed.abort();
    let _ = observed.await;
    assert_eq!(manager.receipt().status().unwrap().dns_active, 1);
    assert!(platform.dns_live.load(Ordering::SeqCst));
    assert!(matches!(
        manager.resolve("second.invalid", None).await,
        Err(NetworkError::TooManyOpenFiles)
    ));
    manager.revoke();
    assert!(!manager.drained().unwrap());
    answer
        .send(Ok(EgressResolution {
            scope,
            addresses: vec!["203.0.113.7".parse().unwrap()],
        }))
        .unwrap();
    let terminal = tokio::time::timeout(DEADLINE, manager.receipt().wait())
        .await
        .unwrap()
        .unwrap();
    assert!(terminal.closed);
    assert!(!platform.dns_live.load(Ordering::SeqCst));
    assert_eq!(terminal.dns_active, 0);
}

#[tokio::test]
async fn resolver_rejects_a_forged_scope_or_unapproved_answer() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    for forged in [true, false] {
        let selected = scope();
        let platform = platform(selected.clone(), None, false);
        let (answer, receive) = oneshot::channel();
        *platform.answer.lock().unwrap() = Some(receive);
        let manager = manager(
            &session,
            platform,
            policy(selected.clone(), false, true),
            limits(1, 1),
        );
        answer
            .send(Ok(EgressResolution {
                scope: if forged { scope() } else { selected },
                addresses: vec![
                    if forged { "203.0.113.7" } else { "192.168.1.7" }
                        .parse()
                        .unwrap(),
                ],
            }))
            .unwrap();
        assert!(manager.resolve("fixture.invalid", None).await.is_err());
        manager.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn real_capture_loss_closes_established_tcp_and_never_rearms_same_scope() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let platform = platform(scope.clone(), None, false);
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope, true, false),
        limits(1, 1),
    );
    let namespace = ServiceNetwork::new(true);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = direct(&manager, &namespace, listener.local_addr().unwrap());
    ready(&mut client).await;
    let (mut server, _) = listener.accept().await.unwrap();
    platform.lost.revoke();
    tokio::time::timeout(DEADLINE, manager.receipt().wait())
        .await
        .unwrap()
        .unwrap();
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(DEADLINE, server.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert!(
        manager
            .open(
                &namespace,
                namespace.reserve((Ipv4Addr::LOCALHOST, 0).into()).unwrap(),
                listener.local_addr().unwrap(),
                64
            )
            .is_err()
    );
}

#[tokio::test]
async fn dropping_manager_and_cancelling_shutdown_observer_cannot_release_pending_provider_or_owner()
 {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let gate = Arc::new(Semaphore::new(0));
    let platform = platform(scope.clone(), Some(gate.clone()), false);
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope, true, false),
        limits(1, 1),
    );
    let namespace = ServiceNetwork::new(true);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let _client = direct(&manager, &namespace, listener.local_addr().unwrap());
    platform.bind_started.notified().await;
    let receipt = manager.receipt();
    let observer = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.shutdown().await })
    };
    manager.revoke();
    observer.abort();
    let _ = observer.await;
    drop(manager);
    drop(session);
    assert!(matches!(
        EngineSession::open(temporary.path()),
        Err(wasmdroid_engine::EngineError::Conflict(_))
    ));
    assert!(!receipt.status().unwrap().closed);
    assert!(platform.binder_live.load(Ordering::SeqCst));
    gate.add_permits(1);
    tokio::time::timeout(DEADLINE, receipt.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!platform.binder_live.load(Ordering::SeqCst));
    no_accept(&listener).await;
    let _reopened = EngineSession::open(temporary.path()).unwrap();
}

#[tokio::test]
async fn namespace_localhost_reserved_addresses_and_custom_dns_never_fall_through() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let platform = platform(scope.clone(), None, false);
    let policy = policy(scope, true, true);
    let manager = manager(&session, platform.clone(), policy.clone(), limits(2, 1));
    let namespace = ServiceNetwork::new(true);
    assert!(
        namespace
            .install_egress(("wrong", "generation", 7), manager.clone())
            .is_err()
    );
    namespace
        .install_egress(("owned", "generation", 7), manager.clone())
        .unwrap();
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    for peer in [
        listener.local_addr().unwrap(),
        "127.0.0.2:80".parse().unwrap(),
        "0.0.0.0:80".parse().unwrap(),
        "10.203.0.2:80".parse().unwrap(),
        "10.203.0.99:80".parse().unwrap(),
    ] {
        assert!(
            namespace
                .connect_tcp((Ipv4Addr::LOCALHOST, 0).into(), peer)
                .await
                .is_err()
        );
    }
    assert!(
        namespace
            .resolve("ungranted.localhost", None, None)
            .await
            .is_err()
    );
    assert!(
        namespace
            .resolve("fixture.invalid", None, Some(Ipv4Addr::LOCALHOST.into()))
            .await
            .is_err()
    );
    assert!(
        namespace
            .resolve("::ffff:127.0.0.1", None, None)
            .await
            .is_err()
    );
    assert_eq!(platform.bind_calls.load(Ordering::SeqCst), 0);
    assert_eq!(platform.dns_calls.load(Ordering::SeqCst), 0);
    assert_eq!(policy.tcp_calls.load(Ordering::SeqCst), 0);
    no_accept(&listener).await;
    assert!(!namespace.egress_drained().unwrap());
    namespace.close();
    manager.receipt().wait().await.unwrap();
    assert!(namespace.egress_drained().unwrap());
}

#[tokio::test]
async fn deadline_refuses_late_bind_but_retains_provider_and_client_slot_until_real_return() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let gate = Arc::new(Semaphore::new(0));
    let platform = platform(scope.clone(), Some(gate.clone()), false);
    let limits = EgressLimits::new(1, 1, 4, Duration::from_millis(20), DEADLINE).unwrap();
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope, true, false),
        limits,
    );
    let namespace = ServiceNetwork::new(true);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = direct(&manager, &namespace, listener.local_addr().unwrap());
    platform.bind_started.notified().await;
    assert!(
        tokio::time::timeout(DEADLINE, poll_fn(|cx| client.poll_write_ready(cx)))
            .await
            .unwrap()
            .is_err()
    );
    assert!(platform.binder_live.load(Ordering::SeqCst));
    assert_eq!(manager.receipt().status().unwrap().tcp_active, 1);
    assert!(
        manager
            .open(
                &namespace,
                namespace.reserve((Ipv4Addr::LOCALHOST, 0).into()).unwrap(),
                listener.local_addr().unwrap(),
                64
            )
            .is_err()
    );
    manager.revoke();
    assert!(!manager.drained().unwrap());
    gate.add_permits(1);
    tokio::time::timeout(DEADLINE, manager.receipt().wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!platform.binder_live.load(Ordering::SeqCst));
    no_accept(&listener).await;
}

#[tokio::test]
async fn namespace_replacement_requires_actual_drain_fresh_grant_and_old_capture_cannot_revoke_new()
{
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let first_scope = scope();
    let first_platform = platform(first_scope.clone(), None, false);
    let first = manager(
        &session,
        first_platform.clone(),
        policy(first_scope, true, false),
        limits(1, 1),
    );
    let namespace = ServiceNetwork::new(true);
    namespace
        .install_egress(("owned", "generation", 7), first.clone())
        .unwrap();
    let second_scope = EgressScope::new("owned".into(), "generation".into(), 7, 2, 3).unwrap();
    let second = manager(
        &session,
        platform(second_scope.clone(), None, false),
        policy(second_scope, true, false),
        limits(1, 1),
    );
    assert!(
        namespace
            .install_egress(("owned", "generation", 7), second.clone())
            .is_err()
    );
    first.shutdown().await.unwrap();
    let same_scope = scope();
    let same = manager(
        &session,
        platform(same_scope.clone(), None, false),
        policy(same_scope, true, false),
        limits(1, 1),
    );
    assert!(
        namespace
            .install_egress(("owned", "generation", 7), same.clone())
            .is_err()
    );
    same.shutdown().await.unwrap();
    namespace
        .install_egress(("owned", "generation", 7), second.clone())
        .unwrap();
    first_platform.lost.revoke();
    first.revoke();
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client: Box<dyn VirtualTcpSocket + Sync> =
        Box::new(direct(&second, &namespace, listener.local_addr().unwrap()));
    let (mut server, _) = tokio::time::timeout(DEADLINE, listener.accept())
        .await
        .unwrap()
        .unwrap();
    client.write_all(b"new").await.unwrap();
    let mut bytes = [0; 3];
    server.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"new");
    assert!(!second.receipt().status().unwrap().revoked);
    namespace.close();
    second.receipt().wait().await.unwrap();
}

#[tokio::test]
async fn stop_before_first_install_permanently_fences_outbound_without_changing_private_loopback() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let namespace = ServiceNetwork::new(true);
    namespace.revoke_egress();
    let scope = scope();
    let manager = manager(
        &session,
        platform(scope.clone(), None, false),
        policy(scope, true, false),
        limits(1, 1),
    );
    assert!(
        namespace
            .install_egress(("owned", "generation", 7), manager.clone())
            .is_err()
    );
    let _listener = namespace
        .listen_tcp((Ipv4Addr::LOCALHOST, 8123).into(), false, false, false)
        .await
        .unwrap();
    let _private = namespace
        .connect_tcp(
            (Ipv4Addr::LOCALHOST, 0).into(),
            (Ipv4Addr::LOCALHOST, 8123).into(),
        )
        .await
        .unwrap();
    manager.shutdown().await.unwrap();
    assert!(namespace.egress_drained().unwrap());
    namespace.close();
}

#[tokio::test]
async fn malformed_private_name_variants_never_reach_external_provider() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let platform = platform(scope.clone(), None, false);
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope, false, true),
        limits(1, 1),
    );
    let namespace = ServiceNetwork::new(true);
    namespace
        .install_egress(("owned", "generation", 7), manager.clone())
        .unwrap();
    for host in [
        "database..",
        "database\u{3002}",
        "database\u{ff0e}",
        "database\u{ff61}",
        "data..base",
        "%64atabase",
        "database\0",
        "database\n",
        "database ",
        ".database",
        ".",
    ] {
        assert!(namespace.resolve(host, None, None).await.is_err());
    }
    assert!(
        namespace
            .resolve(&format!("{}.invalid", "a".repeat(64)), None, None)
            .await
            .is_err()
    );
    assert_eq!(platform.dns_calls.load(Ordering::SeqCst), 0);
    assert_eq!(manager.receipt().status().unwrap().dns_active, 0);
    manager.shutdown().await.unwrap();
}

async fn no_tcp_work(manager: &ServiceEgress) {
    tokio::time::timeout(DEADLINE, async {
        while manager.receipt().status().unwrap().tcp_active != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dropped_guest_pending_open_retains_binder_but_never_connects_after_actual_return() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let gate = Arc::new(Semaphore::new(0));
    let platform = platform(scope.clone(), Some(gate.clone()), false);
    let manager = manager(
        &session,
        platform.clone(),
        policy(scope, true, false),
        limits(1, 1),
    );
    let namespace = ServiceNetwork::new(true);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = direct(&manager, &namespace, listener.local_addr().unwrap());
    platform.bind_started.notified().await;
    drop(client);
    assert!(platform.binder_live.load(Ordering::SeqCst));
    assert_eq!(manager.receipt().status().unwrap().tcp_active, 1);
    no_accept(&listener).await;
    gate.add_permits(1);
    no_tcp_work(&manager).await;
    assert!(!platform.binder_live.load(Ordering::SeqCst));
    no_accept(&listener).await;
    let status = manager.receipt().status().unwrap();
    assert!(!status.revoked);
    assert_eq!(status.cancelled, 1);
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn established_full_guest_drop_closes_actual_host_stream_without_revoking_whole_capture() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let scope = scope();
    let manager = manager(
        &session,
        platform(scope.clone(), None, false),
        policy(scope, true, false),
        limits(1, 1),
    );
    let namespace = ServiceNetwork::new(true);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = direct(&manager, &namespace, listener.local_addr().unwrap());
    ready(&mut client).await;
    let (mut server, _) = listener.accept().await.unwrap();
    let mut client: Box<dyn VirtualTcpSocket + Sync> = Box::new(client);
    client.write_all(b"open").await.unwrap();
    let mut bytes = [0; 4];
    server.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"open");
    drop(client);
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(DEADLINE, server.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    no_tcp_work(&manager).await;
    let status = manager.receipt().status().unwrap();
    assert!(!status.revoked);
    assert_eq!(status.cancelled, 1);
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn interactive_cancel_allows_explicit_fresh_capture_at_newer_record_revision_but_stop_and_rollback_do_not()
 {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let original = scope();
    let first = manager(
        &session,
        platform(original.clone(), None, false),
        policy(original, true, false),
        limits(1, 1),
    );
    let namespace = ServiceNetwork::new(true);
    namespace
        .install_egress(("owned", "generation", 7), first.clone())
        .unwrap();
    namespace.revoke_current_egress();
    first.receipt().wait().await.unwrap();
    // The driver separately verifies the current persisted record and exact
    // immutable spec; this exercises namespace transition/admission only.
    let forward = EgressScope::new("owned".into(), "generation".into(), 8, 2, 3).unwrap();
    let second = manager(
        &session,
        platform(forward.clone(), None, false),
        policy(forward, true, false),
        limits(1, 1),
    );
    namespace
        .install_egress(("owned", "generation", 8), second.clone())
        .unwrap();
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = direct(&second, &namespace, listener.local_addr().unwrap());
    ready(&mut client).await;
    let (mut server, _) = listener.accept().await.unwrap();
    let mut client: Box<dyn VirtualTcpSocket + Sync> = Box::new(client);
    client.write_all(b"fresh").await.unwrap();
    let mut bytes = [0; 5];
    server.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"fresh");
    second.shutdown().await.unwrap();
    let rollback = EgressScope::new("owned".into(), "generation".into(), 7, 3, 4).unwrap();
    let stale = manager(
        &session,
        platform(rollback.clone(), None, false),
        policy(rollback, true, false),
        limits(1, 1),
    );
    assert!(
        namespace
            .install_egress(("owned", "generation", 7), stale.clone())
            .is_err()
    );
    stale.shutdown().await.unwrap();
    namespace.revoke_egress();
    let later = EgressScope::new("owned".into(), "generation".into(), 9, 3, 4).unwrap();
    let refused = manager(
        &session,
        platform(later.clone(), None, false),
        policy(later, true, false),
        limits(1, 1),
    );
    assert!(
        namespace
            .install_egress(("owned", "generation", 9), refused.clone())
            .is_err()
    );
    refused.shutdown().await.unwrap();
    namespace.close();
}

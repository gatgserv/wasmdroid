//! Real portable TCP fixtures exercise the same forwarding core. Only the
//! private test entry uses loopback listeners; LanLease::bind remains strict
//! RFC1918 plus a mandatory platform binder. These are not Wi-Fi attestations.
use super::*;
use std::{future::pending, sync::atomic::AtomicUsize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(3);

fn scope(generation: &str) -> LanScope {
    LanScope::new(
        "lan-fixture".into(),
        generation.into(),
        8080,
        1,
        Ipv4Addr::new(192, 168, 1, 7),
    )
    .unwrap()
}
fn limits(clients: usize) -> LanLimits {
    LanLimits::new(clients, Duration::from_secs(30)).unwrap()
}

#[test]
fn ipv6_wifi_admits_only_unicast_peers_in_the_observed_on_link_prefix() {
    let selected = LanIpv6::new("2001:db8:12:34::10".parse().unwrap(), 64).unwrap();
    assert!(selected.allows_peer("2001:db8:12:34::20".parse().unwrap()));
    assert!(!selected.allows_peer("2001:db8:12:35::20".parse().unwrap()));
    for address in ["::", "::1", "fe80::1", "::ffff:192.168.1.8", "ff02::1"] {
        assert!(LanIpv6::new(address.parse().unwrap(), 64).is_err());
    }
    assert!(LanIpv6::new("2001:db8::10".parse().unwrap(), 0).is_err());
    assert!(LanIpv6::new("fd00:1::10".parse().unwrap(), 64).is_ok());
}

#[tokio::test]
async fn both_listener_families_share_one_forwarding_scope_and_shutdown_receipt() {
    let ingress = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = ingress.local_addr().unwrap().port();
    let ingress_v6 = TcpListener::bind((Ipv6Addr::LOCALHOST, port))
        .await
        .unwrap();
    // The private portable fixture uses loopback listeners. Production always
    // acquires the real Wi-Fi addresses through LanLease::bind.
    let selected = scope("dual").with_ipv6(Some(LanIpv6 {
        address: Ipv6Addr::LOCALHOST,
        prefix_len: 128,
    }));
    let connector = pending_connector(selected.clone());
    let forwarder = start_forwarding(
        selected,
        ingress,
        Some(ingress_v6),
        connector.clone(),
        limits(2),
    )
    .unwrap();
    let _v4 = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    let _v6 = TcpStream::connect((Ipv6Addr::LOCALHOST, port))
        .await
        .unwrap();
    timeout(DEADLINE, async {
        while connector.calls.load(Ordering::Acquire) < 2 {
            connector.entered.notified().await;
        }
    })
    .await
    .unwrap();
    let report = timeout(DEADLINE, forwarder.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.accepted, 2);
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
    );
    assert!(
        TcpStream::connect((Ipv6Addr::LOCALHOST, port))
            .await
            .is_err()
    );
}
async fn listener() -> Arc<TcpListener> {
    Arc::new(TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap())
}
struct ListenerConnector {
    scope: LanScope,
    returned: LanScope,
    // Retain the actual target listener capability, not a reusable port number.
    target: Arc<TcpListener>,
}
impl GenerationConnector for ListenerConnector {
    fn scope(&self) -> &LanScope {
        &self.scope
    }
    fn connect(&self, requested: LanScope, revoked: LanRevocation) -> ConnectFuture {
        let actual = self.scope.clone();
        let returned = self.returned.clone();
        let target = self.target.clone();
        Box::pin(async move {
            if requested != actual || revoked.is_revoked() {
                return Err(EngineError::Cancelled);
            }
            let socket = tokio::select! {
                biased;
                _ = revoked.revoked() => return Err(EngineError::Cancelled),
                value = TcpStream::connect(target.local_addr()?) => value?,
            };
            Ok(ScopedStream::new(returned, socket))
        })
    }
}
struct DropFlag {
    dropped: Arc<AtomicBool>,
    probe: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Drop for DropFlag {
    fn drop(&mut self) {
        if let Some(probe) = &self.probe {
            probe();
        }
        self.dropped.store(true, Ordering::Release);
    }
}
struct PendingConnector {
    scope: LanScope,
    entered: Arc<Notify>,
    dropped: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    drop_probe: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl GenerationConnector for PendingConnector {
    fn scope(&self) -> &LanScope {
        &self.scope
    }
    fn connect(&self, _: LanScope, _: LanRevocation) -> ConnectFuture {
        let entered = self.entered.clone();
        let dropped = self.dropped.clone();
        let calls = self.calls.clone();
        let probe = self.drop_probe.clone();
        Box::pin(async move {
            let _drop = DropFlag { dropped, probe };
            calls.fetch_add(1, Ordering::AcqRel);
            entered.notify_one();
            pending::<Result<ScopedStream>>().await
        })
    }
}
fn pending_connector(scope: LanScope) -> Arc<PendingConnector> {
    Arc::new(PendingConnector {
        scope,
        entered: Arc::new(Notify::new()),
        dropped: Arc::new(AtomicBool::new(false)),
        calls: Arc::new(AtomicUsize::new(0)),
        drop_probe: None,
    })
}
async fn start(
    scope: LanScope,
    connector: Arc<dyn GenerationConnector>,
    limits: LanLimits,
) -> LanForwarder {
    let ingress = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    start_forwarding(scope, ingress, None, connector, limits).unwrap()
}
async fn closed_socket(socket: &mut TcpStream) {
    let mut byte = [0; 1];
    match timeout(DEADLINE, socket.read(&mut byte))
        .await
        .expect("socket must close")
    {
        Ok(count) => assert_eq!(count, 0),
        Err(error) => assert!(matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
        )),
    }
}

#[tokio::test]
async fn strict_private_address_and_mandatory_binder_run_before_any_listen() {
    for ip in [
        Ipv4Addr::UNSPECIFIED,
        Ipv4Addr::LOCALHOST,
        Ipv4Addr::new(8, 8, 8, 8),
        Ipv4Addr::new(169, 254, 1, 1),
        Ipv4Addr::new(100, 64, 0, 1),
        Ipv4Addr::new(224, 0, 0, 1),
        Ipv4Addr::BROADCAST,
    ] {
        assert!(LanScope::new("app".into(), "generation".into(), 80, 1, ip).is_err());
    }
    let mut reserved = None;
    let result = LanLease::bind(scope("one"), 0, |socket, selected| {
        assert_eq!(selected.private_address(), Ipv4Addr::new(192, 168, 1, 7));
        // This real bind can succeed only before the factory binds/listens.
        socket.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        reserved = Some(socket.local_addr()?);
        Err(EngineError::Unsupported(
            "fixture network binding refused".into(),
        ))
    });
    assert!(matches!(result, Err(EngineError::Unsupported(_))));
    // Binder failure owns/drops the unlistened socket instead of falling back.
    assert!(std::net::TcpListener::bind(reserved.unwrap()).is_ok());
}

#[tokio::test]
async fn retained_ingress_excludes_other_bind_until_terminal_cleanup() {
    let selected = scope("one");
    let connector = pending_connector(selected.clone());
    let forwarder = start(selected, connector, limits(1)).await;
    let address = forwarder.local_address();
    let revoked = forwarder.revocation();
    assert!(std::net::TcpListener::bind(address).is_err());
    let report = timeout(DEADLINE, forwarder.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.accepted, 0);
    assert!(revoked.is_revoked());
    assert!(revoked.is_closed());
    assert!(std::net::TcpListener::bind(address).is_ok());
}

#[tokio::test]
async fn normal_client_half_close_still_receives_the_full_target_response() {
    let selected = scope("one");
    let target = listener().await;
    let accepting = target.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = accepting.accept().await.unwrap();
        let mut request = Vec::new();
        stream.read_to_end(&mut request).await.unwrap();
        assert_eq!(request, b"request before client half-close");
        stream
            .write_all(b"response after observing EOF")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();
    });
    let connector = Arc::new(ListenerConnector {
        scope: selected.clone(),
        returned: selected.clone(),
        target,
    });
    let forwarder = start(selected, connector, limits(2)).await;
    let mut client = TcpStream::connect(forwarder.local_address()).await.unwrap();
    client
        .write_all(b"request before client half-close")
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    timeout(DEADLINE, client.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response, b"response after observing EOF");
    timeout(DEADLINE, server).await.unwrap().unwrap();
    let report = timeout(DEADLINE, forwarder.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.accepted, 1);
    assert_eq!(report.failed, 0);
    // Cleanup may race the final already-delivered EOF acknowledgement. Both
    // outcomes retain the required complete request/response half-close above.
    assert_eq!(report.completed + report.cancelled, 1);
}

#[tokio::test]
async fn connector_scope_mismatch_closes_the_retained_listener_without_forwarding() {
    let selected = scope("one");
    let variants = [
        scope("other-generation"),
        LanScope::new(
            "other-instance".into(),
            "one".into(),
            8080,
            1,
            selected.private_address(),
        )
        .unwrap(),
        LanScope::new(
            "lan-fixture".into(),
            "one".into(),
            8081,
            1,
            selected.private_address(),
        )
        .unwrap(),
        LanScope::new(
            "lan-fixture".into(),
            "one".into(),
            8080,
            2,
            selected.private_address(),
        )
        .unwrap(),
        LanScope::new(
            "lan-fixture".into(),
            "one".into(),
            8080,
            1,
            Ipv4Addr::new(192, 168, 1, 8),
        )
        .unwrap(),
    ];
    for mismatched in variants {
        let ingress = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = ingress.local_addr().unwrap();
        let connector = pending_connector(mismatched);
        assert!(matches!(
            start_forwarding(
                selected.clone(),
                ingress,
                None,
                connector.clone(),
                limits(1)
            ),
            Err(EngineError::Conflict(_))
        ));
        assert_eq!(connector.calls.load(Ordering::Acquire), 0);
        assert!(std::net::TcpListener::bind(address).is_ok());
    }
}

#[tokio::test]
async fn returned_wrong_generation_stream_receives_no_client_bytes() {
    let selected = scope("one");
    let target = listener().await;
    let accepting = target.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = accepting.accept().await.unwrap();
        let mut bytes = Vec::new();
        timeout(DEADLINE, stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert!(bytes.is_empty());
    });
    let connector = Arc::new(ListenerConnector {
        scope: selected.clone(),
        returned: scope("replacement"),
        target,
    });
    let forwarder = start(selected, connector, limits(1)).await;
    let mut client = TcpStream::connect(forwarder.local_address()).await.unwrap();
    let _ = client
        .write_all(b"must never cross a generation boundary")
        .await;
    timeout(DEADLINE, server).await.unwrap().unwrap();
    closed_socket(&mut client).await;
    let report = timeout(DEADLINE, forwarder.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.failed, 1);
}

#[tokio::test]
async fn revoke_cancels_pending_connector_and_awaits_its_actual_drop() {
    let selected = scope("one");
    let connector = pending_connector(selected.clone());
    let forwarder = start(selected, connector.clone(), limits(1)).await;
    let mut client = TcpStream::connect(forwarder.local_address()).await.unwrap();
    timeout(DEADLINE, connector.entered.notified())
        .await
        .unwrap();
    let revoked = forwarder.revocation();
    revoked.revoke();
    revoked.revoke();
    let report = timeout(DEADLINE, forwarder.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(connector.dropped.load(Ordering::Acquire));
    assert!(revoked.is_closed());
    assert_eq!(report.cancelled, 1);
    closed_socket(&mut client).await;
}

#[tokio::test]
async fn revoke_closes_both_directions_of_an_established_connection() {
    let selected = scope("one");
    let target = listener().await;
    let accepting = target.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = accepting.accept().await.unwrap();
        let mut request = [0; 5];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"hello");
        stream.write_all(b"ack").await.unwrap();
        closed_socket(&mut stream).await;
    });
    let connector = Arc::new(ListenerConnector {
        scope: selected.clone(),
        returned: selected.clone(),
        target,
    });
    let forwarder = start(selected, connector, limits(1)).await;
    let mut client = TcpStream::connect(forwarder.local_address()).await.unwrap();
    client.write_all(b"hello").await.unwrap();
    let mut acknowledgement = [0; 3];
    timeout(DEADLINE, client.read_exact(&mut acknowledgement))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&acknowledgement, b"ack");
    let report = timeout(DEADLINE, forwarder.shutdown())
        .await
        .unwrap()
        .unwrap();
    closed_socket(&mut client).await;
    timeout(DEADLINE, server).await.unwrap().unwrap();
    assert_eq!(report.cancelled, 1);
}

#[tokio::test]
async fn client_limit_rejects_extra_ingress_without_evicting_pending_owner() {
    let selected = scope("one");
    let connector = pending_connector(selected.clone());
    let forwarder = start(selected, connector.clone(), limits(1)).await;
    let _first = TcpStream::connect(forwarder.local_address()).await.unwrap();
    timeout(DEADLINE, connector.entered.notified())
        .await
        .unwrap();
    let mut second = TcpStream::connect(forwarder.local_address()).await.unwrap();
    closed_socket(&mut second).await;
    assert_eq!(connector.calls.load(Ordering::Acquire), 1);
    assert!(!connector.dropped.load(Ordering::Acquire));
    let report = timeout(DEADLINE, forwarder.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.accepted, 1);
    assert_eq!(report.rejected, 1);
    assert_eq!(report.cancelled, 1);
}

#[tokio::test]
async fn connect_deadline_releases_the_pending_stream_and_keeps_listener_owned() {
    let selected = scope("one");
    let connector = pending_connector(selected.clone());
    let forwarder = start(
        selected,
        connector.clone(),
        LanLimits::new(1, Duration::from_millis(20)).unwrap(),
    )
    .await;
    let address = forwarder.local_address();
    let mut client = TcpStream::connect(address).await.unwrap();
    timeout(DEADLINE, connector.entered.notified())
        .await
        .unwrap();
    closed_socket(&mut client).await;
    assert!(connector.dropped.load(Ordering::Acquire));
    assert!(std::net::TcpListener::bind(address).is_err());
    let report = timeout(DEADLINE, forwarder.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.failed, 1);
}

#[tokio::test]
async fn revoking_old_generation_does_not_close_an_independent_current_generation() {
    let first_scope = scope("one");
    let first = start(
        first_scope.clone(),
        pending_connector(first_scope),
        limits(1),
    )
    .await;
    let first_control = first.revocation();
    let old_address = first.local_address();
    let second_scope = scope("two");
    let target = listener().await;
    let accepting = target.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = accepting.accept().await.unwrap();
        stream
            .write_all(b"only the current generation")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();
    });
    let connector = Arc::new(ListenerConnector {
        scope: second_scope.clone(),
        returned: second_scope.clone(),
        target,
    });
    let second = start(second_scope, connector, limits(1)).await;
    timeout(DEADLINE, first.shutdown()).await.unwrap().unwrap();
    first_control.revoke();
    assert!(!second.revocation().is_revoked());
    assert!(TcpStream::connect(old_address).await.is_err());
    let mut client = TcpStream::connect(second.local_address()).await.unwrap();
    client.shutdown().await.unwrap();
    let mut reply = Vec::new();
    timeout(DEADLINE, client.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply, b"only the current generation");
    timeout(DEADLINE, server).await.unwrap().unwrap();
    timeout(DEADLINE, second.shutdown()).await.unwrap().unwrap();
}

#[tokio::test]
async fn dropping_handle_fences_ingress_then_real_closed_receipt_releases_port() {
    let selected = scope("one");
    let forwarder = start(selected.clone(), pending_connector(selected), limits(1)).await;
    let address = forwarder.local_address();
    let revoked = forwarder.revocation();
    drop(forwarder);
    assert!(revoked.is_revoked());
    timeout(DEADLINE, revoked.closed()).await.unwrap();
    assert!(std::net::TcpListener::bind(address).is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_shutdown_observer_does_not_release_port_or_fabricate_cleanup() {
    use std::{future::poll_fn, task::Poll};
    let selected = scope("one");
    let ingress = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = ingress.local_addr().unwrap();
    let bound_during_drop = Arc::new(AtomicBool::new(false));
    let attempted = bound_during_drop.clone();
    let mut connector = pending_connector(selected.clone());
    Arc::get_mut(&mut connector).unwrap().drop_probe = Some(Arc::new(move || {
        attempted.store(
            std::net::TcpListener::bind(address).is_ok(),
            Ordering::Release,
        );
    }));
    let forwarder =
        start_forwarding(selected, ingress, None, connector.clone(), limits(1)).unwrap();
    let mut client = TcpStream::connect(address).await.unwrap();
    timeout(DEADLINE, connector.entered.notified())
        .await
        .unwrap();
    // Client half-closes first so post-cleanup rebind does not depend on a
    // server-initiated TIME_WAIT policy; its read half still checks actual EOF.
    client.shutdown().await.unwrap();
    let revoked = forwarder.revocation();
    let mut observing = Box::pin(forwarder.shutdown());
    // Poll the observing future through its real revoke+join admission, then
    // cancel before this single-thread executor can run the cleanup task. This
    // is deterministic; an ordinary spawned observer could finish in the race.
    poll_fn(|context| match observing.as_mut().poll(context) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("cleanup task must not run inside the observer poll"),
    })
    .await;
    assert!(revoked.is_revoked());
    assert!(!revoked.is_closed());
    assert!(!connector.dropped.load(Ordering::Acquire));
    drop(observing);
    timeout(DEADLINE, revoked.closed()).await.unwrap();
    assert!(connector.dropped.load(Ordering::Acquire));
    assert!(!bound_during_drop.load(Ordering::Acquire));
    closed_socket(&mut client).await;
    drop(client);
    assert!(std::net::TcpListener::bind(address).is_ok());
}

//! Actual in-memory worker namespace/stream tests, not registry or guest mocks.
//! Binding/Android Wi-Fi acceptance is separate from these native definitions.
use super::*;
use std::{future::poll_fn, net::Ipv4Addr, task::Poll};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wasmer_wasix::virtual_net::{NetworkError, VirtualNetworking};

const DEADLINE: Duration = Duration::from_secs(3);
fn scope(generation: &str, port: u16, epoch: u64) -> LanScope {
    LanScope::new(
        "owned".into(),
        generation.into(),
        port,
        epoch,
        Ipv4Addr::new(192, 168, 1, 7),
    )
    .unwrap()
}
fn address(port: u16) -> SocketAddr {
    (Ipv4Addr::LOCALHOST, port).into()
}
fn manager(network: ServiceNetwork) -> Arc<ServiceLan> {
    ServiceLan::new(
        "owned".into(),
        "one".into(),
        BTreeSet::from([8080]),
        network,
        Handle::current(),
    )
}
fn connector(manager: &ServiceLan, scope: LanScope) -> IngressConnector {
    IngressConnector {
        ingress: manager.network.ingress(scope.guest_port()),
        scope,
        revoked: LanRevocation::new(),
        generation_open: manager.generation_open.clone(),
    }
}

#[tokio::test]
async fn actual_generation_namespace_transfers_bytes_and_preserves_half_close() {
    let network = ServiceNetwork::new(true);
    let mut guest = network
        .listen_tcp(address(8080), false, false, false)
        .await
        .unwrap();
    let other = ServiceNetwork::new(true);
    let mut unrelated = other
        .listen_tcp(address(8080), false, false, false)
        .await
        .unwrap();
    let manager = manager(network);
    let selected = scope("one", 8080, 1);
    let connector = connector(&manager, selected.clone());
    let mut client = connector
        .socket(&selected, &LanRevocation::new())
        .await
        .unwrap();
    let (mut server, _) = guest.try_accept().unwrap();
    assert!(matches!(
        unrelated.try_accept(),
        Err(NetworkError::WouldBlock)
    ));
    client
        .write_all(b"only the captured namespace")
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    let mut request = Vec::new();
    tokio::time::timeout(DEADLINE, server.read_to_end(&mut request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(request, b"only the captured namespace");
    server.write_all(b"reply after request EOF").await.unwrap();
    server.shutdown().await.unwrap();
    let mut reply = Vec::new();
    tokio::time::timeout(DEADLINE, client.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply, b"reply after request EOF");
}

#[tokio::test]
async fn wrong_scope_tuple_never_enqueues_a_guest_connection() {
    let network = ServiceNetwork::new(true);
    let mut guest = network
        .listen_tcp(address(8080), false, false, false)
        .await
        .unwrap();
    let manager = manager(network);
    let selected = scope("one", 8080, 1);
    let connector = connector(&manager, selected.clone());
    for wrong in [
        scope("two", 8080, 1),
        scope("one", 8081, 1),
        scope("one", 8080, 2),
        LanScope::new(
            "another".into(),
            "one".into(),
            8080,
            1,
            selected.private_address(),
        )
        .unwrap(),
        LanScope::new(
            "owned".into(),
            "one".into(),
            8080,
            1,
            Ipv4Addr::new(192, 168, 1, 8),
        )
        .unwrap(),
    ] {
        assert!(matches!(
            connector.socket(&wrong, &LanRevocation::new()).await,
            Err(EngineError::Conflict(_))
        ));
        assert!(matches!(guest.try_accept(), Err(NetworkError::WouldBlock)));
    }
}

#[tokio::test]
async fn missing_exact_port_never_falls_back_to_another_private_listener() {
    let network = ServiceNetwork::new(true);
    let mut guest = network
        .listen_tcp(address(8080), false, false, false)
        .await
        .unwrap();
    let manager = manager(network);
    let selected = scope("one", 8081, 1);
    let connector = connector(&manager, selected.clone());
    let cancel = LanRevocation::new();
    let mut pending = Box::pin(connector.socket(&selected, &cancel));
    poll_fn(|context| match pending.as_mut().poll(context) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("absent exact port must remain pending"),
    })
    .await;
    assert!(matches!(guest.try_accept(), Err(NetworkError::WouldBlock)));
    cancel.revoke();
    assert!(matches!(pending.await, Err(EngineError::Cancelled)));
}

#[tokio::test]
async fn both_share_and_transport_cancellation_interrupt_waiting_for_guest_port() {
    for share_cancel in [false, true] {
        let manager = manager(ServiceNetwork::new(true));
        let selected = scope("one", 8080, 1);
        let connector = connector(&manager, selected.clone());
        let transport = LanRevocation::new();
        let mut waiting = Box::pin(connector.socket(&selected, &transport));
        poll_fn(|context| match waiting.as_mut().poll(context) {
            Poll::Pending => Poll::Ready(()),
            Poll::Ready(_) => panic!("guest port is not listening"),
        })
        .await;
        if share_cancel {
            connector.revoked.revoke();
        } else {
            transport.revoke();
        }
        assert!(matches!(
            tokio::time::timeout(DEADLINE, waiting).await.unwrap(),
            Err(EngineError::Cancelled)
        ));
    }
}

#[tokio::test]
async fn old_namespace_capability_cannot_retarget_a_new_worker_generation() {
    let old_network = ServiceNetwork::new(true);
    let old = manager(old_network.clone());
    let old_scope = scope("one", 8080, 1);
    let old_connector = connector(&old, old_scope.clone());
    old.close_generation();
    old_network.close();
    let replacement = ServiceNetwork::new(true);
    let mut listener = replacement
        .listen_tcp(address(8080), false, false, false)
        .await
        .unwrap();
    assert!(matches!(
        old_connector
            .socket(&old_scope, &LanRevocation::new())
            .await,
        Err(EngineError::Cancelled)
    ));
    assert!(matches!(
        listener.try_accept(),
        Err(NetworkError::WouldBlock)
    ));
    let new_scope = scope("two", 8080, 1);
    let new = ServiceLan::new(
        "owned".into(),
        "two".into(),
        BTreeSet::from([8080]),
        replacement,
        Handle::current(),
    );
    let mut client = connector(&new, new_scope.clone())
        .socket(&new_scope, &LanRevocation::new())
        .await
        .unwrap();
    let (mut server, _) = listener.try_accept().unwrap();
    client.write_all(b"new generation only").await.unwrap();
    let mut bytes = [0; 19];
    tokio::time::timeout(DEADLINE, server.read_exact(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes, b"new generation only");
}

#[tokio::test]
async fn manager_refuses_unpublished_port_or_foreign_worker_before_platform_binding() {
    let manager = manager(ServiceNetwork::new(true));
    for selected in [
        scope("two", 8080, 1),
        scope("one", 8081, 1),
        LanScope::new(
            "foreign".into(),
            "one".into(),
            8080,
            1,
            Ipv4Addr::new(192, 168, 1, 7),
        )
        .unwrap(),
    ] {
        let called = AtomicBool::new(false);
        assert!(
            manager
                .enable(
                    selected,
                    0,
                    LanLimits::new(1, Duration::from_secs(1)).unwrap(),
                    |_, _| {
                        called.store(true, Ordering::Release);
                        Ok(())
                    }
                )
                .is_err()
        );
        assert!(!called.load(Ordering::Acquire));
    }
    assert!(manager.drained().unwrap());
}

#[tokio::test]
async fn cancellation_after_platform_binding_prevents_listen_and_requires_fresh_epoch() {
    let manager = manager(ServiceNetwork::new(true));
    let selected = scope("one", 8080, 1);
    let mut reserved = None;
    let result = manager.enable(
        selected.clone(),
        0,
        LanLimits::new(1, Duration::from_secs(1)).unwrap(),
        |socket, _| {
            socket.bind(address(0))?;
            reserved = Some(socket.local_addr()?);
            manager.revoke(&selected)?;
            Ok(())
        },
    );
    assert!(matches!(result, Err(EngineError::Cancelled)));
    assert!(std::net::TcpListener::bind(reserved.unwrap()).is_ok());
    assert!(manager.drained().unwrap());
    assert_eq!(manager.status(&selected).unwrap().phase, LanPhase::Closed);
    assert!(manager.status(&selected).unwrap().failed);
    assert!(manager.disable(&selected).await.is_err()); // Real closed-with-error receipt retained.
    let called = AtomicBool::new(false);
    assert!(matches!(
        manager.enable(
            selected,
            0,
            LanLimits::new(1, Duration::from_secs(1)).unwrap(),
            |_, _| {
                called.store(true, Ordering::Release);
                Ok(())
            }
        ),
        Err(EngineError::Conflict(_))
    ));
    assert!(!called.load(Ordering::Acquire));
    let fresh = scope("one", 8080, 2);
    assert!(
        manager
            .enable(
                fresh.clone(),
                0,
                LanLimits::new(1, Duration::from_secs(1)).unwrap(),
                |_, _| {
                    called.store(true, Ordering::Release);
                    Err(EngineError::Unsupported(
                        "fixture network unavailable".into(),
                    ))
                }
            )
            .is_err()
    );
    assert!(called.load(Ordering::Acquire));
    assert!(matches!(
        manager.revoke(&scope("one", 8080, 1)),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(manager.status(&fresh).unwrap().phase, LanPhase::Closed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abandoned_disable_observer_cannot_release_preparing_socket_ownership() {
    let manager = manager(ServiceNetwork::new(true));
    let selected = scope("one", 8080, 1);
    let actual = manager.clone();
    let preparing = selected.clone();
    let (address_tx, address_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let enable = tokio::task::spawn_blocking(move || {
        let mut address_tx = Some(address_tx);
        actual.enable(
            preparing,
            0,
            LanLimits::new(1, Duration::from_secs(1)).unwrap(),
            |socket, _| {
                socket.bind(address(0))?;
                let _ = address_tx
                    .take()
                    .expect("IPv4-only fixture")
                    .send(socket.local_addr()?);
                release_rx.recv_timeout(DEADLINE).map_err(|_| {
                    EngineError::Conflict("fixture platform binder did not finish".into())
                })?;
                Ok(())
            },
        )
    });
    let reserved = tokio::time::timeout(DEADLINE, address_rx)
        .await
        .unwrap()
        .unwrap();
    let mut observer = Box::pin(manager.disable(&selected));
    poll_fn(|context| match observer.as_mut().poll(context) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("platform preparation still owns its socket"),
    })
    .await;
    assert_eq!(manager.status(&selected).unwrap().phase, LanPhase::Revoking);
    assert!(!manager.drained().unwrap());
    drop(observer);
    assert!(std::net::TcpListener::bind(reserved).is_err());
    release_tx.send(()).unwrap();
    assert!(matches!(
        tokio::time::timeout(DEADLINE, enable)
            .await
            .unwrap()
            .unwrap(),
        Err(EngineError::Cancelled)
    ));
    assert!(manager.drained().unwrap());
    assert_eq!(manager.status(&selected).unwrap().phase, LanPhase::Closed);
    assert!(std::net::TcpListener::bind(reserved).is_ok());
}

#[tokio::test]
async fn generation_close_revokes_preparation_and_fences_new_enables() {
    let manager = manager(ServiceNetwork::new(true));
    let selected = scope("one", 8080, 1);
    assert!(matches!(
        manager.enable(
            selected,
            0,
            LanLimits::new(1, Duration::from_secs(1)).unwrap(),
            |_, _| {
                manager.close_generation();
                Ok(())
            }
        ),
        Err(EngineError::Cancelled)
    ));
    assert!(manager.drained().unwrap());
    let called = AtomicBool::new(false);
    assert!(matches!(
        manager.enable(
            scope("one", 8080, 2),
            0,
            LanLimits::new(1, Duration::from_secs(1)).unwrap(),
            |_, _| {
                called.store(true, Ordering::Release);
                Ok(())
            }
        ),
        Err(EngineError::Conflict(_))
    ));
    assert!(!called.load(Ordering::Acquire));
}

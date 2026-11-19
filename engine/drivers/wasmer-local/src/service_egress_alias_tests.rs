//! Actual route-table replacement/lease tests; no platform DNS or policy claim.
use super::*;
use wasmdroid_engine::network::grants::GrantKey;

fn config() -> WorkerNetwork {
    WorkerNetwork {
        schema_version: 1,
        revision: 1,
        stack_id: "group".into(),
        instance_id: "owned".into(),
        generation: "generation".into(),
        ip: Ipv4Addr::new(10, 203, 0, 2),
        alias: "web".into(),
        incoming: Vec::new(),
        outgoing: vec![PeerGrant {
            instance_id: "peer".into(),
            generation: "peer-generation".into(),
            alias: "database".into(),
            ip: Ipv4Addr::new(10, 203, 0, 3),
            key: GrantKey::try_from("01".repeat(32)).unwrap(),
        }],
    }
}
fn routes(session: &EngineSession, config: WorkerNetwork) -> Routes {
    config.validate("owned", "generation").unwrap();
    let reserved = reserve_aliases(&BTreeSet::new(), &config).unwrap();
    let outgoing = config
        .outgoing
        .iter()
        .map(|peer| (peer.ip, Arc::new(GrantLease::new(peer.clone()))))
        .collect();
    let incoming = config
        .incoming
        .iter()
        .map(|peer| (peer.ip, Arc::new(GrantLease::new(peer.clone()))))
        .collect();
    Routes {
        table: Mutex::new(Table {
            config,
            incoming,
            outgoing,
            reserved,
        }),
        root: session.root().into(),
        executor: tokio::runtime::Handle::current(),
        _owner: session.ownership(),
        outgoing_active: Arc::new(AtomicUsize::new(0)),
    }
}

#[tokio::test]
async fn removed_and_renamed_private_aliases_remain_reserved_for_generation() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let original = config();
    let routes = routes(&session, original.clone());
    assert!(routes.resolve("database").is_some());
    let mut removed = original.clone();
    removed.revision = 2;
    removed.outgoing.clear();
    routes.update(removed).unwrap();
    assert_eq!(routes.resolve("database"), None);
    assert!(routes.reserved_name("DATABASE."));
    let mut renamed = original;
    renamed.revision = 3;
    renamed.outgoing[0].alias = "new-database".into();
    routes.update(renamed).unwrap();
    assert!(routes.resolve("new-database").is_some());
    assert_eq!(routes.resolve("database"), None);
    assert!(routes.reserved_name("database"));
    assert!(routes.reserved_name("new-database"));
    assert!(routes.reserved_name("web"));
    assert!(!routes.reserved_name("public.invalid"));
}

#[tokio::test]
async fn expired_private_lease_does_not_make_alias_public() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let routes = routes(&session, config());
    let lease = routes
        .table
        .lock()
        .unwrap()
        .outgoing
        .values()
        .next()
        .unwrap()
        .clone();
    lease.revoke();
    assert_eq!(routes.resolve("database"), None);
    assert!(routes.reserved_name("database"));
    assert!(routes.reserved_name("web"));
}

#[tokio::test]
async fn alias_history_capacity_failure_preserves_last_authorized_table_without_eviction() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let mut config = config();
    let routes = routes(&session, config.clone());
    for index in 0..MAX_RESERVED_ALIASES - 2 {
        config.revision += 1;
        config.outgoing[0].alias = format!("peer-{index}");
        routes.update(config.clone()).unwrap();
    }
    let previous = config.clone();
    config.revision += 1;
    config.outgoing[0].alias = "overflow".into();
    assert!(routes.update(config).is_err());
    assert_eq!(
        routes.table.lock().unwrap().config.revision,
        previous.revision
    );
    assert!(routes.resolve(&previous.outgoing[0].alias).is_some());
    assert!(routes.reserved_name("database"));
    assert!(!routes.reserved_name("overflow"));
}

#[derive(Debug)]
struct NamePolicy(crate::service_egress::EgressScope);
impl crate::service_egress::DestinationPolicy for NamePolicy {
    fn authorize_name(&self, scope: &crate::service_egress::EgressScope, host: &str) -> Result<()> {
        if scope == &self.0 && host == "late-name" {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    fn authorize_ip(&self, scope: &crate::service_egress::EgressScope, ip: IpAddr) -> Result<()> {
        if scope == &self.0 && ip == "203.0.113.7".parse::<IpAddr>().unwrap() {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    fn authorize_tcp(
        &self,
        _scope: &crate::service_egress::EgressScope,
        _peer: SocketAddr,
    ) -> Result<()> {
        Err(invalid())
    }
}
#[derive(Debug)]
struct NameNetwork {
    scope: crate::service_egress::EgressScope,
    started: tokio::sync::Notify,
    answer: Mutex<Option<tokio::sync::oneshot::Receiver<Vec<IpAddr>>>>,
}
impl crate::service_egress::SelectedNetwork for NameNetwork {
    fn scope(&self) -> &crate::service_egress::EgressScope {
        &self.scope
    }
    fn current(&self) -> bool {
        true
    }
    fn invalidated(&self) -> crate::service_egress::InvalidationFuture<'_> {
        Box::pin(std::future::pending())
    }
    fn bind<'a>(
        &'a self,
        _scope: &'a crate::service_egress::EgressScope,
        _socket: &'a tokio::net::TcpSocket,
    ) -> crate::service_egress::BindFuture<'a> {
        Box::pin(async { Err(invalid()) })
    }
    fn resolve<'a>(
        &'a self,
        scope: &'a crate::service_egress::EgressScope,
        _host: &'a str,
        _port: Option<u16>,
    ) -> crate::service_egress::ResolveFuture<'a> {
        Box::pin(async move {
            assert_eq!(scope, &self.scope);
            let receive = self.answer.lock().unwrap().take().unwrap();
            self.started.notify_one();
            Ok(crate::service_egress::EgressResolution {
                scope: self.scope.clone(),
                addresses: receive.await.unwrap(),
            })
        })
    }
}

#[tokio::test]
async fn alias_becoming_private_during_real_retained_dns_work_rejects_late_public_answer() {
    use crate::service_egress::{EgressLimits, EgressScope, ServiceEgress};
    use wasmer_wasix::virtual_net::VirtualNetworking;
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let mut config = config();
    let routes = Arc::new(routes(&session, config.clone()));
    let namespace = ServiceNetwork::new(true);
    namespace.install_routes(routes.clone()).unwrap();
    let scope = EgressScope::new("owned".into(), "generation".into(), 0, 1, 1).unwrap();
    let (answer, receive) = tokio::sync::oneshot::channel();
    let selected = Arc::new(NameNetwork {
        scope: scope.clone(),
        started: tokio::sync::Notify::new(),
        answer: Mutex::new(Some(receive)),
    });
    let limits =
        EgressLimits::new(1, 1, 1, Duration::from_secs(3), Duration::from_secs(3)).unwrap();
    let manager = ServiceEgress::new(
        selected.clone(),
        Arc::new(NamePolicy(scope)),
        session.ownership(),
        tokio::runtime::Handle::current(),
        limits,
    )
    .unwrap();
    namespace
        .install_egress(("owned", "generation", 0), manager.clone())
        .unwrap();
    let observer = {
        let namespace = namespace.clone();
        tokio::spawn(async move { namespace.resolve("late-name", None, None).await })
    };
    selected.started.notified().await;
    config.revision += 1;
    config.outgoing[0].alias = "late-name".into();
    routes.update(config).unwrap();
    answer.send(vec!["203.0.113.7".parse().unwrap()]).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), observer)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert_eq!(
        routes.resolve("late-name"),
        Some(Ipv4Addr::new(10, 203, 0, 3).into())
    );
    manager.shutdown().await.unwrap();
    namespace.close();
}

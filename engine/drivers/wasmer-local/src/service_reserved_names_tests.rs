//! Real native inventory + namespace ordering; provider is controlled in memory.
//! No actual external DNS, TCP, Android permission or OS routing is exercised.
use super::*;
use crate::service_egress::{
    BindFuture, DestinationPolicy, EgressLimits, EgressResolution, EgressScope, InvalidationFuture,
    ResolveFuture, SelectedNetwork, ServiceEgress,
};
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference},
    network::grants::topology,
    stacks::StackManager,
    store::Digest,
};
use wasmer_wasix::virtual_net::VirtualNetworking;

#[derive(Debug)]
struct CountNetwork {
    scope: EgressScope,
    calls: AtomicUsize,
}
impl SelectedNetwork for CountNetwork {
    fn scope(&self) -> &EgressScope {
        &self.scope
    }
    fn current(&self) -> bool {
        true
    }
    fn invalidated(&self) -> InvalidationFuture<'_> {
        Box::pin(std::future::pending())
    }
    fn bind<'a>(
        &'a self,
        _scope: &'a EgressScope,
        _socket: &'a tokio::net::TcpSocket,
    ) -> BindFuture<'a> {
        Box::pin(async { Err(invalid()) })
    }
    fn resolve<'a>(
        &'a self,
        _scope: &'a EgressScope,
        _host: &'a str,
        _port: Option<u16>,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(EgressResolution {
                scope: self.scope.clone(),
                addresses: vec!["8.8.8.8".parse().unwrap()],
            })
        })
    }
}
#[derive(Debug)]
struct FixturePolicy(EgressScope);
impl DestinationPolicy for FixturePolicy {
    fn authorize_name(&self, scope: &EgressScope, _host: &str) -> Result<()> {
        if *scope == self.0 {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    fn authorize_ip(&self, scope: &EgressScope, _address: IpAddr) -> Result<()> {
        self.authorize_name(scope, "")
    }
    fn authorize_tcp(&self, scope: &EgressScope, _peer: SocketAddr) -> Result<()> {
        self.authorize_name(scope, "")
    }
}
fn actual_config(session: &EngineSession) -> WorkerNetwork {
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse("wasmer.io/example/reserved@1.0.0").unwrap(),
            digest: Digest::of(b"metadata"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        })
        .unwrap();
    StackManager::new(session).create("group","services: {web: {image: wasmer.io/example/reserved@1.0.0}, database: {image: wasmer.io/example/reserved@1.0.0}, queue: {image: wasmer.io/example/reserved@1.0.0}}").unwrap();
    let members = session.state().stack_members("group").unwrap();
    let id = members
        .iter()
        .find(|member| member["service"] == "web")
        .unwrap()["instance_id"]
        .as_str()
        .unwrap();
    topology(
        session.state(),
        "group",
        &BTreeMap::from([(id.to_owned(), "generation".into())]),
        &BTreeMap::new(),
        1,
    )
    .unwrap()
    .remove(id)
    .unwrap()
}
fn routes(session: &EngineSession, config: WorkerNetwork) -> Routes {
    let inventory = config.reserved_aliases(session.state()).unwrap();
    let reserved = reserve_inventory(&BTreeSet::new(), &config, inventory).unwrap();
    Routes {
        table: Mutex::new(Table {
            config,
            incoming: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            reserved,
        }),
        root: session.root().into(),
        executor: tokio::runtime::Handle::current(),
        _owner: session.ownership(),
        outgoing_active: Arc::new(AtomicUsize::new(0)),
    }
}
#[tokio::test]
async fn never_granted_group_names_make_zero_external_dns_calls_while_public_lookup_works() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let config = actual_config(&session);
    assert!(config.outgoing.is_empty());
    let namespace = ServiceNetwork::new(true);
    namespace
        .install_routes(Arc::new(routes(&session, config.clone())))
        .unwrap();
    let scope = EgressScope::new(
        config.instance_id.clone(),
        config.generation.clone(),
        0,
        1,
        1,
    )
    .unwrap();
    let selected = Arc::new(CountNetwork {
        scope: scope.clone(),
        calls: AtomicUsize::new(0),
    });
    let manager = ServiceEgress::new(
        selected.clone(),
        Arc::new(FixturePolicy(scope)),
        session.ownership(),
        tokio::runtime::Handle::current(),
        EgressLimits::new(1, 1, 1, Duration::from_secs(3), Duration::from_secs(3)).unwrap(),
    )
    .unwrap();
    namespace
        .install_egress(
            (&config.instance_id, &config.generation, 0),
            manager.clone(),
        )
        .unwrap();
    for name in ["database", "DATABASE.", "queue", "queue."] {
        assert!(namespace.resolve(name, None, None).await.is_err());
    }
    assert_eq!(selected.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        namespace
            .resolve("public.example", None, None)
            .await
            .unwrap(),
        vec!["8.8.8.8".parse::<IpAddr>().unwrap()]
    );
    assert_eq!(selected.calls.load(Ordering::SeqCst), 1);
    manager.shutdown().await.unwrap();
    namespace.close();
}
#[tokio::test]
async fn complete_inventory_reservation_keeps_retired_names_and_refuses_partial_updates() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let config = actual_config(&session);
    let routes = routes(&session, config.clone());
    assert!(routes.reserved_name("database"));
    assert!(routes.resolve("database").is_none());
    let mut next = config.clone();
    next.revision += 1;
    routes
        .update_with_aliases(next.clone(), ["web".into(), "new-database".into()].into())
        .unwrap();
    assert!(routes.reserved_name("database"));
    assert!(routes.reserved_name("new-database"));
    next.revision += 1;
    assert!(
        routes
            .update_with_aliases(next, ["untrusted".into()].into())
            .is_err()
    );
    assert!(routes.reserved_name("database"));
    assert!(routes.reserved_name("new-database"));
}

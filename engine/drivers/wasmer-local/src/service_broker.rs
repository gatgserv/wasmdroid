//! Authenticated private Unix transport. No guest-selected host socket address,
//! file path, key or raw host networking implementation is exposed.
use crate::http_execution::ForwardStop;
use crate::service_network::{Binding, Connection, ServiceNetwork, Socket};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};
use virtual_mio::InterestType;
use wasmdroid_engine::{
    EngineError, Result,
    network::grants::{PeerGrant, WorkerNetwork, endpoint_name},
    session::{EngineOwnership, EngineSession},
};
use wasmer_wasix::virtual_net::{self, InterestHandler, NetworkError, SocketStatus};

#[derive(Debug)]
pub(super) struct Opening(Mutex<Pending>);
#[derive(Debug)]
struct Pending {
    status: SocketStatus,
    error: Option<NetworkError>,
    wakers: Vec<Waker>,
    handler: Option<Box<dyn InterestHandler + Send + Sync>>,
}
impl Opening {
    pub(super) fn new() -> Self {
        Self(Mutex::new(Pending {
            status: SocketStatus::Opening,
            error: None,
            wakers: Vec::new(),
            handler: None,
        }))
    }
    pub fn ready(&self, cx: &mut Context<'_>) -> Poll<virtual_net::Result<()>> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        match state.status {
            SocketStatus::Opened => Poll::Ready(Ok(())),
            SocketStatus::Opening => {
                if !state.wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
                    if state.wakers.len() >= 32 {
                        return Poll::Ready(Err(NetworkError::TooManyOpenFiles));
                    }
                    state.wakers.push(cx.waker().clone());
                }
                Poll::Pending
            }
            _ => Poll::Ready(Err(state.error.unwrap_or(NetworkError::ConnectionAborted))),
        }
    }
    pub fn status(&self) -> SocketStatus {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).status
    }
    pub fn error(&self) -> Option<NetworkError> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).error
    }
    pub fn check(&self) -> virtual_net::Result<()> {
        match self.status() {
            SocketStatus::Opening => Err(NetworkError::WouldBlock),
            SocketStatus::Opened => Ok(()),
            _ => Err(self.error().unwrap_or(NetworkError::ConnectionAborted)),
        }
    }
    pub fn handler(
        &self,
        handler: Box<dyn InterestHandler + Send + Sync>,
    ) -> Option<Box<dyn InterestHandler + Send + Sync>> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.status == SocketStatus::Opening {
            state.handler = Some(handler);
            None
        } else {
            Some(handler)
        }
    }
    pub(super) fn finish(&self, pair: &Connection, error: Option<NetworkError>) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.status != SocketStatus::Opening {
            return;
        }
        if let Some(mut handler) = state.handler.take() {
            if error.is_none() {
                let _ = pair.set_handler(0, handler);
            } else {
                handler.push_interest(InterestType::Error);
                handler.push_interest(InterestType::Closed);
            }
        }
        state.status = if error.is_none() {
            SocketStatus::Opened
        } else {
            SocketStatus::Failed
        };
        state.error = error;
        let wakers = std::mem::take(&mut state.wakers);
        drop(state);
        for waker in wakers {
            waker.wake();
        }
    }
}
#[derive(Debug)]
pub(super) struct GrantLease {
    pub grant: PeerGrant,
    active: AtomicBool,
    connections: Mutex<Vec<Weak<Connection>>>,
}
impl GrantLease {
    fn new(grant: PeerGrant) -> Self {
        Self {
            grant,
            active: AtomicBool::new(true),
            connections: Mutex::new(Vec::new()),
        }
    }
    pub fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    fn attach(&self, pair: &Arc<Connection>) -> virtual_net::Result<()> {
        let mut connections = self.connections.lock().map_err(|_| NetworkError::Lock)?;
        if !self.active() {
            return Err(NetworkError::PermissionDenied);
        }
        connections.retain(|pair| pair.upgrade().is_some_and(|pair| pair.allocated()));
        if connections.len() >= 128 {
            return Err(NetworkError::TooManyOpenFiles);
        }
        connections.push(Arc::downgrade(pair));
        Ok(())
    }
    fn revoke(&self) {
        self.active.store(false, Ordering::Release);
        let connections =
            std::mem::take(&mut *self.connections.lock().unwrap_or_else(|p| p.into_inner()));
        for pair in connections.into_iter().filter_map(|pair| pair.upgrade()) {
            pair.close();
        }
    }
}
#[derive(Debug)]
struct Table {
    config: WorkerNetwork,
    incoming: BTreeMap<Ipv4Addr, Arc<GrantLease>>,
    outgoing: BTreeMap<Ipv4Addr, Arc<GrantLease>>,
    reserved: BTreeSet<String>,
}
const MAX_RESERVED_ALIASES: usize = 128;
#[cfg(test)]
#[path = "service_egress_alias_tests.rs"]
mod egress_alias_tests;
#[cfg(test)]
#[path = "service_reserved_names_tests.rs"]
mod reserved_names_tests;
fn reserve_aliases(prior: &BTreeSet<String>, config: &WorkerNetwork) -> Result<BTreeSet<String>> {
    let mut names = prior.clone();
    for alias in std::iter::once(&config.alias).chain(
        config
            .incoming
            .iter()
            .chain(&config.outgoing)
            .map(|peer| &peer.alias),
    ) {
        names.insert(
            alias
                .strip_suffix('.')
                .unwrap_or(alias)
                .to_ascii_lowercase(),
        );
        if names.len() > MAX_RESERVED_ALIASES {
            return Err(EngineError::Conflict(
                "private alias history capacity exceeded".into(),
            ));
        }
    }
    Ok(names)
}
fn reserve_inventory(
    prior: &BTreeSet<String>,
    config: &WorkerNetwork,
    inventory: BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    if inventory.is_empty()
        || inventory.len() > 8
        || !inventory.contains(&config.alias.to_ascii_lowercase())
        || config
            .incoming
            .iter()
            .chain(&config.outgoing)
            .any(|peer| !inventory.contains(&peer.alias.to_ascii_lowercase()))
        || inventory.iter().any(|name| {
            name.is_empty()
                || name.len() > 63
                || name == "localhost"
                || name.ends_with('.')
                || name.parse::<IpAddr>().is_ok()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        })
    {
        return Err(invalid());
    }
    let mut reserved = reserve_aliases(prior, config)?;
    reserved.extend(inventory);
    if reserved.len() > MAX_RESERVED_ALIASES {
        return Err(EngineError::Conflict(
            "private alias history capacity exceeded".into(),
        ));
    }
    Ok(reserved)
}
pub(super) struct Routes {
    table: Mutex<Table>,
    root: PathBuf,
    executor: tokio::runtime::Handle,
    _owner: EngineOwnership,
    outgoing_active: Arc<AtomicUsize>,
}
struct OutgoingIo {
    count: Arc<AtomicUsize>,
    _owner: EngineOwnership,
}
impl Drop for OutgoingIo {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}
impl std::fmt::Debug for Routes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateRoutes")
    }
}
impl Routes {
    // Existing synthetic lease fixtures do not manufacture native membership.
    // Production callers must use a validated complete inventory below.
    #[cfg(test)]
    pub fn update(&self, config: WorkerNetwork) -> Result<()> {
        self.update_inner(config, None)
    }
    pub fn update_with_aliases(
        &self,
        config: WorkerNetwork,
        aliases: BTreeSet<String>,
    ) -> Result<()> {
        self.update_inner(config, Some(aliases))
    }
    fn update_inner(&self, config: WorkerNetwork, aliases: Option<BTreeSet<String>>) -> Result<()> {
        let mut table = self.table.lock().map_err(|_| invalid())?;
        config.validate(&table.config.instance_id, &table.config.generation)?;
        if config.stack_id != table.config.stack_id
            || config.alias != table.config.alias
            || config.ip != table.config.ip
            || config.revision <= table.config.revision
        {
            return Err(invalid());
        }
        let reserved = match aliases {
            Some(aliases) => reserve_inventory(&table.reserved, &config, aliases)?,
            None => reserve_aliases(&table.reserved, &config)?,
        };
        let mut revoked = Vec::new();
        fn replace(
            old: &mut BTreeMap<Ipv4Addr, Arc<GrantLease>>,
            peers: &[PeerGrant],
            revoked: &mut Vec<Arc<GrantLease>>,
        ) {
            let mut next = BTreeMap::new();
            for peer in peers {
                let lease = old
                    .remove(&peer.ip)
                    .filter(|lease| {
                        if lease.grant.same(peer) && lease.active() {
                            true
                        } else {
                            revoked.push(lease.clone());
                            false
                        }
                    })
                    .unwrap_or_else(|| Arc::new(GrantLease::new(peer.clone())));
                next.insert(peer.ip, lease);
            }
            revoked.extend(std::mem::take(old).into_values());
            *old = next;
        }
        replace(&mut table.incoming, &config.incoming, &mut revoked);
        replace(&mut table.outgoing, &config.outgoing, &mut revoked);
        table.config = config;
        table.reserved = reserved;
        drop(table);
        for lease in revoked {
            lease.revoke();
        }
        Ok(())
    }
    pub fn resolve(&self, name: &str) -> Option<IpAddr> {
        let table = self.table.lock().ok()?;
        let name = name.strip_suffix('.').unwrap_or(name);
        if table.config.alias.eq_ignore_ascii_case(name) {
            return Some(Ipv4Addr::LOCALHOST.into());
        }
        if let Ok(ip) = name.parse::<Ipv4Addr>() {
            return table
                .outgoing
                .get(&ip)
                .filter(|lease| lease.active())
                .map(|_| ip.into());
        }
        table
            .outgoing
            .values()
            .find(|lease| lease.active() && lease.grant.alias.eq_ignore_ascii_case(name))
            .map(|lease| IpAddr::V4(lease.grant.ip))
    }
    pub fn reserved_name(&self, name: &str) -> bool {
        // Lock failure cannot authorize a fallback into external DNS.
        self.table
            .lock()
            .map(|table| {
                table
                    .reserved
                    .contains(&name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase())
            })
            .unwrap_or(true)
    }
    pub fn open(
        &self,
        network: &ServiceNetwork,
        binding: Arc<Binding>,
        peer: SocketAddr,
        ttl: u32,
    ) -> virtual_net::Result<Socket> {
        #[cfg(unix)]
        {
            if !binding.address.is_ipv4() {
                return Err(NetworkError::InvalidInput);
            }
            let IpAddr::V4(ip) = peer.ip() else {
                return Err(NetworkError::PermissionDenied);
            };
            if peer.port() == 0 {
                return Err(NetworkError::InvalidInput);
            }
            let table = self.table.lock().map_err(|_| NetworkError::Lock)?;
            let grant = table
                .outgoing
                .get(&ip)
                .filter(|lease| lease.active())
                .ok_or(NetworkError::PermissionDenied)?
                .clone();
            let source = SocketAddr::new(table.config.ip.into(), binding.address.port());
            drop(table);
            let opening = Arc::new(Opening::new());
            let (client, bridge) =
                network.remote_pair(binding, source, peer, ttl, opening.clone())?;
            grant.attach(&client.pair)?;
            let pair = client.pair.clone();
            self.outgoing_active.fetch_add(1, Ordering::AcqRel);
            let outgoing = OutgoingIo {
                count: self.outgoing_active.clone(),
                _owner: self._owner.clone(),
            };
            let path = self.root.join(endpoint_name(
                &grant.grant.instance_id,
                &grant.grant.generation,
            ));
            self.executor.spawn(async move {
                let _outgoing = outgoing;
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let result = tokio::time::timeout(Duration::from_secs(3), async {
                    let mut stream = tokio::net::UnixStream::connect(path)
                        .await
                        .map_err(|_| NetworkError::ConnectionRefused)?;
                    let mut header = [0u8; 12];
                    header[..4].copy_from_slice(b"WDP1");
                    header[4..8].copy_from_slice(
                        &source
                            .ip()
                            .to_canonical()
                            .to_string()
                            .parse::<Ipv4Addr>()
                            .map_err(|_| NetworkError::InvalidInput)?
                            .octets(),
                    );
                    header[8..10].copy_from_slice(&source.port().to_be_bytes());
                    header[10..12].copy_from_slice(&peer.port().to_be_bytes());
                    stream
                        .write_all(&header)
                        .await
                        .map_err(|_| NetworkError::ConnectionAborted)?;
                    stream
                        .write_all(&grant.grant.key.tag(&header))
                        .await
                        .map_err(|_| NetworkError::ConnectionAborted)?;
                    let ack = stream
                        .read_u8()
                        .await
                        .map_err(|_| NetworkError::ConnectionRefused)?;
                    if ack != 1 || !grant.active() {
                        return Err(NetworkError::ConnectionRefused);
                    }
                    Ok(stream)
                })
                .await
                .unwrap_or(Err(NetworkError::TimedOut));
                match result {
                    Ok(mut stream) => {
                        opening.finish(&pair, None);
                        let mut bridge: Box<dyn virtual_net::VirtualTcpSocket + Sync> =
                            Box::new(bridge);
                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut bridge).await;
                    }
                    Err(error) => {
                        opening.finish(&pair, Some(error));
                        pair.close();
                    }
                }
            });
            Ok(client)
        }
        #[cfg(not(unix))]
        {
            let _ = (network, binding, peer, ttl);
            Err(NetworkError::Unsupported)
        }
    }
}
fn invalid() -> EngineError {
    EngineError::Invalid("invalid private network operation".into())
}

pub(super) fn install(
    network: &ServiceNetwork,
    session: &EngineSession,
    config: WorkerNetwork,
    executor: tokio::runtime::Handle,
) -> Result<(
    tokio::sync::oneshot::Sender<ForwardStop>,
    tokio::task::JoinHandle<bool>,
)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        let aliases = config.reserved_aliases(session.state())?;
        let root = session.root().join("network");
        match std::fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Err(error) = std::fs::DirBuilder::new().mode(0o700).create(&root) {
                    if error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(error.into());
                    }
                    let metadata = std::fs::symlink_metadata(&root)?;
                    if !metadata.is_dir() || metadata.file_type().is_symlink() {
                        return Err(invalid());
                    }
                }
            }
            _ => return Err(invalid()),
        }
        let path = root.join(endpoint_name(&config.instance_id, &config.generation));
        if path.as_os_str().as_encoded_bytes().len() > 100 {
            return Err(EngineError::Unsupported(
                "private endpoint path exceeds platform limit".into(),
            ));
        }
        let listener = tokio::net::UnixListener::bind(&path).map_err(|_| invalid())?;
        let metadata = std::fs::symlink_metadata(&path)?;
        let endpoint = Endpoint {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        let routes = Arc::new(Routes {
            root,
            executor: executor.clone(),
            _owner: session.ownership(),
            outgoing_active: Arc::new(AtomicUsize::new(0)),
            table: Mutex::new(Table {
                reserved: reserve_inventory(&BTreeSet::new(), &config, aliases)?,
                incoming: config
                    .incoming
                    .iter()
                    .map(|grant| (grant.ip, Arc::new(GrantLease::new(grant.clone()))))
                    .collect(),
                outgoing: config
                    .outgoing
                    .iter()
                    .map(|grant| (grant.ip, Arc::new(GrantLease::new(grant.clone()))))
                    .collect(),
                config,
            }),
        });
        network.install_routes(routes.clone())?;
        let (sender, mut stop) = tokio::sync::oneshot::channel();
        let network = network.clone();
        let task=executor.spawn(async move {
            use tokio::io::{AsyncReadExt,AsyncWriteExt};
            let _endpoint=endpoint;let mut connections=tokio::task::JoinSet::new();
            let shutdown=loop {tokio::select! {
                command=&mut stop=>break command.unwrap_or(ForwardStop::Abort),
                incoming=listener.accept()=>{
                    let Ok((mut stream,_))=incoming else{break ForwardStop::Abort};
                    if connections.len()>=32{continue;}
                    let routes=routes.clone();let network=network.clone();
                    connections.spawn(async move {
                        let opened=tokio::time::timeout(Duration::from_secs(3),async {
                            let mut header=[0u8;12];let mut tag=[0u8;32];
                            stream.read_exact(&mut header).await.ok()?;stream.read_exact(&mut tag).await.ok()?;
                            if &header[..4]!=b"WDP1"{return None;}
                            let ip=Ipv4Addr::new(header[4],header[5],header[6],header[7]);
                            let port=u16::from_be_bytes([header[10],header[11]]);let source_port=u16::from_be_bytes([header[8],header[9]]);
                            if port==0||source_port==0{return None;}
                            let lease=routes.table.lock().ok()?.incoming.get(&ip).cloned()?;
                            if !lease.active()||!lease.grant.key.verify(&header,&tag){return None;}
                            let socket=network.routed_ingress(SocketAddr::new(ip.into(),source_port),port).ok()?;
                            lease.attach(&socket.pair).ok()?;Some(socket)
                        }).await.ok().flatten();
                        if let Some(socket)=opened {
                            if stream.write_all(&[1]).await.is_ok() {
                                let mut socket:Box<dyn virtual_net::VirtualTcpSocket+Sync>=Box::new(socket);
                                let _=tokio::io::copy_bidirectional(&mut stream,&mut socket).await;
                            }
                        } else {let _=stream.write_all(&[0]).await;}
                    });
                },
                _=connections.join_next(),if !connections.is_empty()=>{}
            }};
            drop(listener);
            let drained=matches!(shutdown,ForwardStop::Drain)&&tokio::time::timeout(Duration::from_secs(2),async {
                while connections.join_next().await.is_some(){}
                while routes.outgoing_active.load(Ordering::Acquire)!=0{tokio::time::sleep(Duration::from_millis(10)).await;}
            }).await.is_ok();
            connections.abort_all();while connections.join_next().await.is_some(){}drained
        });
        Ok((sender, task))
    }
    #[cfg(not(unix))]
    {
        let _ = (network, session, config, executor);
        Err(EngineError::Unsupported(
            "private worker networking requires Unix".into(),
        ))
    }
}
#[cfg(unix)]
struct Endpoint {
    path: PathBuf,
    device: u64,
    inode: u64,
}
#[cfg(unix)]
impl Drop for Endpoint {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path) {
            if metadata.dev() == self.device
                && metadata.ino() == self.inode
                && metadata.file_type().is_socket()
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use virtual_net::{VirtualConnectedSocketExt, VirtualNetworking};
    use wasmdroid_engine::{
        api::{Artifact, ArtifactKind, ArtifactReference},
        network::grants::{GrantKey, topology},
        stacks::StackManager,
        store::Digest,
    };
    #[tokio::test]
    async fn authentication_opening_state_exact_delivery_and_revocation_use_real_unix_io() {
        let temp = tempfile::tempdir().unwrap();
        let session = EngineSession::open(temp.path()).unwrap();
        session
            .state()
            .put_artifact(&Artifact {
                reference: ArtifactReference::parse("wasmer.io/example/broker-fixture@1.0.0")
                    .unwrap(),
                digest: Digest::of(b"metadata fixture"),
                kind: ArtifactKind::Webc,
                driver: "wasmer-local".into(),
                platform: "wasm32-wasix".into(),
                commands: vec!["run".into()],
                size_bytes: 0,
                labels: BTreeMap::new(),
            })
            .unwrap();
        let document = r#"{"services":{"a":{"image":"wasmer.io/example/broker-fixture@1.0.0"},"b":{"image":"wasmer.io/example/broker-fixture@1.0.0"}}}"#;
        let definition = StackManager::new(&session)
            .create("broker", document)
            .unwrap();
        let workers = definition
            .instances
            .iter()
            .enumerate()
            .map(|(index, member)| (member.id.clone(), format!("run-{index}")))
            .collect();
        let configs = topology(session.state(), "broker", &workers, &BTreeMap::new(), 1).unwrap();
        let a = configs.values().find(|config| config.alias == "a").unwrap();
        let b = configs.values().find(|config| config.alias == "b").unwrap();
        let first = ServiceNetwork::new(true);
        let second = ServiceNetwork::new(true);
        let (stop_a, task_a) = install(
            &first,
            &session,
            a.clone(),
            tokio::runtime::Handle::current(),
        )
        .unwrap();
        let (stop_b, task_b) = install(
            &second,
            &session,
            b.clone(),
            tokio::runtime::Handle::current(),
        )
        .unwrap();
        let mut listener = second
            .listen_tcp("127.0.0.1:8012".parse().unwrap(), false, false, false)
            .await
            .unwrap();
        let path = session
            .root()
            .join("network")
            .join(endpoint_name(&b.instance_id, &b.generation));
        let mut attacker = tokio::net::UnixStream::connect(path).await.unwrap();
        let mut request = [0u8; 44];
        request[..4].copy_from_slice(b"WDP1");
        request[4..8].copy_from_slice(&a.ip.octets());
        request[8..10].copy_from_slice(&50000u16.to_be_bytes());
        request[10..12].copy_from_slice(&8012u16.to_be_bytes());
        attacker.write_all(&request).await.unwrap();
        assert_eq!(0, attacker.read_u8().await.unwrap());
        assert!(matches!(
            listener.try_accept(),
            Err(NetworkError::WouldBlock)
        ));
        let mut client = first
            .connect_tcp(
                "0.0.0.0:0".parse().unwrap(),
                SocketAddr::new(b.ip.into(), 8012),
            )
            .await
            .unwrap();
        assert_eq!(client.status().unwrap(), SocketStatus::Opening);
        tokio::time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|cx| client.poll_write_ready(cx)),
        )
        .await
        .unwrap()
        .unwrap();
        let (mut server, peer) = listener.try_accept().unwrap();
        assert_eq!(peer.ip(), IpAddr::V4(a.ip));
        client.send(b"auth").await.unwrap();
        let mut value = [0; 4];
        server.read_exact(&mut value).await.unwrap();
        assert_eq!(&value, b"auth");
        assert!(
            first
                .update_routes(a.clone(), a.reserved_aliases(session.state()).unwrap())
                .is_err()
        ); // Stale epoch cannot replace live authority.
        let mut changed = a.clone();
        changed.revision = 2;
        changed.outgoing[0].key = GrantKey::try_from("01".repeat(32)).unwrap();
        let aliases = changed.reserved_aliases(session.state()).unwrap();
        first.update_routes(changed, aliases).unwrap();
        assert!(client.is_closed());
        let mut rejected = first
            .connect_tcp(
                "0.0.0.0:0".parse().unwrap(),
                SocketAddr::new(b.ip.into(), 8012),
            )
            .await
            .unwrap();
        let failure = tokio::time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|cx| rejected.poll_write_ready(cx)),
        )
        .await
        .unwrap();
        assert!(failure.is_err());
        assert_eq!(rejected.status().unwrap(), SocketStatus::Failed);
        assert!(matches!(
            listener.try_accept(),
            Err(NetworkError::WouldBlock)
        ));
        first.close();
        second.close();
        let _ = stop_a.send(ForwardStop::Abort);
        let _ = stop_b.send(ForwardStop::Abort);
        task_a.await.unwrap();
        task_b.await.unwrap();
    }
}

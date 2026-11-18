//! Private TCP namespace for one service generation. Bind/listen remain virtual;
//! external connect/resolve require an explicitly installed private capability.
//! Android workers install a native-authorized capability; no ambient fallback exists.
use std::{
    collections::{BTreeMap, VecDeque},
    mem::MaybeUninit,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr},
    sync::{Arc, Mutex, MutexGuard, Weak},
    task::{Context, Poll, Waker},
    time::Duration,
};
use wasmer_wasix::virtual_net::{
    self, InterestHandler, IpCidr, NetworkError, SocketStatus, VirtualConnectedSocket,
    VirtualIoSource, VirtualNetworking, VirtualSocket, VirtualTcpBoundSocket, VirtualTcpListener,
    VirtualTcpSocket, tcp_pair::TcpSocketHalf,
};

const MAX_PORTS: usize = 128;
const MAX_CONNECTIONS: usize = 128;
const MAX_BACKLOG: usize = 32;
const BUFFER_BYTES: usize = 64 * 1024;
const MAX_WAKERS: usize = 32;

#[cfg(test)]
#[path = "service_egress_regrant_tests.rs"]
mod egress_regrant_tests;
#[cfg(test)]
#[path = "service_egress_tests.rs"]
mod egress_tests;

fn lock<T>(mutex: &Mutex<T>) -> virtual_net::Result<MutexGuard<'_, T>> {
    mutex.lock().map_err(|_| NetworkError::Lock)
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceNetwork(Arc<Mutex<Namespace>>);
#[derive(Debug)]
struct Namespace {
    closed: bool,
    ports: BTreeMap<SocketAddr, Weak<Binding>>,
    connections: Vec<Weak<Connection>>,
    ephemeral: u16,
    routes: Option<Arc<crate::service_broker::Routes>>,
    egress: Option<Arc<crate::service_egress::ServiceEgress>>,
    egress_fenced: bool,
}
#[derive(Debug)]
pub(super) struct Binding {
    pub address: SocketAddr,
    dual_stack: bool,
    network: ServiceNetwork,
    listener: Mutex<Weak<Mutex<ListenerState>>>,
}
#[derive(Debug)]
struct ListenerState {
    closed: bool,
    backlog: VecDeque<Socket>,
    handler: Option<Box<dyn InterestHandler + Send + Sync>>,
    wakers: Vec<Waker>,
    ttl: u8,
}
pub(super) struct Connection(
    [Mutex<Option<TcpSocketHalf>>; 2],
    wasmdroid_engine::network::lan::LanRevocation,
);
impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Connection").field(&self.0).finish()
    }
}
impl Connection {
    pub(super) fn client_present(&self) -> bool {
        !self.1.is_revoked()
    }
    pub(super) async fn client_closed(&self) {
        self.1.revoked().await;
    }
    pub(super) fn allocated(&self) -> bool {
        self.0
            .iter()
            .any(|half| half.lock().map(|value| value.is_some()).unwrap_or(true))
    }
    fn close_half(&self, index: usize) {
        // Full close/drop only. shutdown(Write) intentionally never signals it.
        if index == 0 {
            self.1.revoke();
        }
        let half = self.0[index]
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(half) = half {
            let _ = half.close();
        }
    }
    pub fn close(&self) {
        self.close_half(0);
        self.close_half(1);
    }
    pub fn set_handler(
        &self,
        index: usize,
        handler: Box<dyn InterestHandler + Send + Sync>,
    ) -> virtual_net::Result<()> {
        lock(&self.0[index])?
            .as_mut()
            .ok_or(NetworkError::NotConnected)?
            .set_handler(handler)
    }
}

impl ServiceNetwork {
    pub fn new(enabled: bool) -> Self {
        Self(Arc::new(Mutex::new(Namespace {
            closed: !enabled,
            ports: BTreeMap::new(),
            connections: Vec::new(),
            ephemeral: 49152,
            routes: None,
            egress: None,
            egress_fenced: false,
        })))
    }
    fn reserve(&self, address: SocketAddr) -> virtual_net::Result<Arc<Binding>> {
        self.reserve_tcp(address, true)
    }
    fn reserve_tcp(&self, address: SocketAddr, only_v6: bool) -> virtual_net::Result<Arc<Binding>> {
        if only_v6 && mapped_ipv4(address).is_some() {
            return Err(NetworkError::InvalidInput);
        }
        let dual_stack = !only_v6 && address.ip() == IpAddr::V6(Ipv6Addr::UNSPECIFIED);
        let mut address = normalize(address, true)?;
        let keys = |address: SocketAddr| {
            let ipv4_alias =
                dual_stack.then(|| SocketAddr::new(Ipv4Addr::LOCALHOST.into(), address.port()));
            [Some(canonical_address(address)), ipv4_alias]
                .into_iter()
                .flatten()
        };
        let mut state = lock(&self.0)?;
        if state.closed {
            return Err(NetworkError::PermissionDenied);
        }
        state.ports.retain(|_, binding| binding.strong_count() != 0);
        if state.ports.len() + keys(address).count() > MAX_PORTS {
            return Err(NetworkError::TooManyOpenFiles);
        }
        if address.port() == 0 {
            let start = state.ephemeral;
            loop {
                address.set_port(state.ephemeral);
                state.ephemeral = if state.ephemeral == u16::MAX {
                    49152
                } else {
                    state.ephemeral + 1
                };
                if keys(address).all(|key| !state.ports.contains_key(&key)) {
                    break;
                }
                if state.ephemeral == start {
                    return Err(NetworkError::AddressInUse);
                }
            }
        } else if keys(address).any(|key| state.ports.contains_key(&key)) {
            return Err(NetworkError::AddressInUse);
        }
        let binding = Arc::new(Binding {
            address,
            dual_stack,
            network: self.clone(),
            listener: Mutex::new(Weak::new()),
        });
        for key in keys(address) {
            state.ports.insert(key, Arc::downgrade(&binding));
        }
        Ok(binding)
    }
    fn connect_from(
        &self,
        binding: Arc<Binding>,
        peer: SocketAddr,
        ttl: u32,
    ) -> virtual_net::Result<Socket> {
        if normalize(peer, false).is_err() {
            let (routes, egress) = {
                let state = lock(&self.0)?;
                if state.closed {
                    return Err(NetworkError::PermissionDenied);
                }
                (state.routes.clone(), state.egress.clone())
            };
            if reserved_destination(peer.ip()) {
                return routes
                    .ok_or(NetworkError::PermissionDenied)?
                    .open(self, binding, peer, ttl);
            }
            return egress
                .ok_or(NetworkError::PermissionDenied)?
                .open(self, binding, peer, ttl);
        }
        self.connect_local(binding, peer, ttl, None)
    }
    fn connect_local(
        &self,
        binding: Arc<Binding>,
        peer: SocketAddr,
        ttl: u32,
        shown_source: Option<SocketAddr>,
    ) -> virtual_net::Result<Socket> {
        let peer = normalize(peer, false)?;
        if binding.address.is_ipv4() != peer.is_ipv4() {
            return Err(NetworkError::InvalidInput);
        }
        let target = canonical_address(peer);
        let mut source = canonical_address(binding.address);
        if target.is_ipv4() && binding.dual_stack {
            source.set_ip(Ipv4Addr::LOCALHOST.into());
        }
        if source.is_ipv4() != target.is_ipv4() {
            return Err(NetworkError::InvalidInput);
        }
        let mut network = lock(&self.0)?;
        if network.closed {
            return Err(NetworkError::PermissionDenied);
        }
        let destination = network
            .ports
            .get(&target)
            .and_then(Weak::upgrade)
            .ok_or(NetworkError::ConnectionRefused)?;
        let listener = lock(&destination.listener)?
            .upgrade()
            .ok_or(NetworkError::ConnectionRefused)?;
        let mut state = lock(&listener)?;
        if state.closed {
            return Err(NetworkError::ConnectionRefused);
        }
        if state.backlog.len() >= MAX_BACKLOG {
            return Err(NetworkError::ConnectionRefused);
        }
        network
            .connections
            .retain(|socket| socket.upgrade().is_some_and(|socket| socket.allocated()));
        if network.connections.len() >= MAX_CONNECTIONS {
            return Err(NetworkError::TooManyOpenFiles);
        }
        let (mut client, mut server) = TcpSocketHalf::channel_with_level_triggered_write(
            BUFFER_BYTES,
            shown_source.unwrap_or(source),
            target,
        );
        client.set_ttl(ttl)?;
        server.set_ttl(u32::from(state.ttl))?;
        let pair = Arc::new(Connection(
            [Mutex::new(Some(client)), Mutex::new(Some(server))],
            wasmdroid_engine::network::lan::LanRevocation::new(),
        ));
        network.connections.push(Arc::downgrade(&pair));
        state.backlog.push_back(Socket {
            pair: pair.clone(),
            index: 1,
            reservation: None,
            opening: None,
            mapped_v4: target.is_ipv4() && destination.address.is_ipv6(),
        });
        let wakers = std::mem::take(&mut state.wakers);
        // Keep the handler installed: taking it here loses the WASIX poll source
        // registration before the next accept. Wake after releasing namespace.
        drop(network);
        if let Some(handler) = state.handler.as_mut() {
            handler.push_interest(virtual_mio::InterestType::Readable);
        }
        drop(state);
        for waker in wakers {
            waker.wake();
        }
        Ok(Socket {
            pair,
            index: 0,
            reservation: Some(binding),
            opening: None,
            mapped_v4: target.is_ipv4() && peer.is_ipv6(),
        })
    }
    pub fn install_routes(
        &self,
        routes: Arc<crate::service_broker::Routes>,
    ) -> wasmdroid_engine::Result<()> {
        let mut state = lock(&self.0).map_err(|_| {
            wasmdroid_engine::EngineError::Conflict("network state lock failed".into())
        })?;
        if state.closed || state.routes.is_some() {
            return Err(wasmdroid_engine::EngineError::Conflict(
                "network cannot accept routes".into(),
            ));
        }
        state.routes = Some(routes);
        Ok(())
    }
    /// Private host factory only; saved specs/labels cannot install a manager.
    pub(crate) fn install_egress(
        &self,
        expected: (&str, &str, u64),
        manager: Arc<crate::service_egress::ServiceEgress>,
    ) -> wasmdroid_engine::Result<()> {
        let scope = manager.scope();
        if (
            scope.instance_id(),
            scope.generation(),
            scope.spec_revision(),
        ) != expected
        {
            return Err(wasmdroid_engine::EngineError::Conflict(
                "outbound capability belongs to another execution".into(),
            ));
        }
        // Injected platform validity is checked outside the namespace mutex.
        manager.check_current()?;
        let mut state = lock(&self.0).map_err(|_| {
            wasmdroid_engine::EngineError::Conflict("network state lock failed".into())
        })?;
        if state.closed || state.egress_fenced || !manager.admission_open() {
            return Err(wasmdroid_engine::EngineError::Conflict(
                "network cannot accept outbound capability".into(),
            ));
        }
        if let Some(previous) = &state.egress {
            let prior = previous.scope();
            if (prior.instance_id(), prior.generation()) != (expected.0, expected.1)
                || scope.spec_revision() < prior.spec_revision()
                || scope.grant_epoch() <= prior.grant_epoch()
                || !previous.drained()?
            {
                return Err(wasmdroid_engine::EngineError::Conflict(
                    "prior outbound capture is not replaceable".into(),
                ));
            }
        }
        state.egress = Some(manager);
        Ok(())
    }
    pub(crate) fn revoke_egress(&self) {
        let manager = {
            let mut state = self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.egress_fenced = true;
            state.egress.clone()
        };
        if let Some(manager) = manager {
            manager.revoke();
        }
    }
    pub(crate) fn egress_manager(
        &self,
    ) -> wasmdroid_engine::Result<Option<Arc<crate::service_egress::ServiceEgress>>> {
        Ok(lock(&self.0)
            .map_err(|_| {
                wasmdroid_engine::EngineError::Conflict("network state lock failed".into())
            })?
            .egress
            .clone())
    }
    pub(crate) fn replace_egress(
        &self,
        previous: &Arc<crate::service_egress::ServiceEgress>,
        manager: Arc<crate::service_egress::ServiceEgress>,
    ) -> wasmdroid_engine::Result<()> {
        // Caller rechecks injected validity BEFORE its final native guard.
        // This local CAS must never call a foreign capability under that guard.
        let mut state = lock(&self.0).map_err(|_| {
            wasmdroid_engine::EngineError::Conflict("network state lock failed".into())
        })?;
        let prior = previous.scope();
        let next = manager.scope();
        if state.closed
            || state.egress_fenced
            || !state
                .egress
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, previous))
            || !manager.admission_open()
            || !previous.drained()?
            || prior.instance_id() != next.instance_id()
            || prior.generation() != next.generation()
            || next.grant_epoch() <= prior.grant_epoch()
            || next.spec_revision() < prior.spec_revision()
        {
            return Err(wasmdroid_engine::EngineError::Conflict(
                "outbound manager replacement changed".into(),
            ));
        }
        state.egress = Some(manager);
        Ok(())
    }
    /// Interactive Ctrl-C cancels this capture without withdrawing the saved
    /// permission. A later fresh explicit capture still requires actual drain.
    pub(crate) fn revoke_current_egress(&self) {
        let manager = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .egress
            .clone();
        if let Some(manager) = manager {
            manager.revoke();
        }
    }
    pub(crate) fn egress_drained(&self) -> wasmdroid_engine::Result<bool> {
        let manager = lock(&self.0)
            .map_err(|_| {
                wasmdroid_engine::EngineError::Conflict("network state lock failed".into())
            })?
            .egress
            .clone();
        match manager {
            Some(manager) => manager.drained(),
            None => Ok(true),
        }
    }
    pub fn update_routes(
        &self,
        config: wasmdroid_engine::network::grants::WorkerNetwork,
        aliases: std::collections::BTreeSet<String>,
    ) -> wasmdroid_engine::Result<()> {
        let state = lock(&self.0).map_err(|_| {
            wasmdroid_engine::EngineError::Conflict("network state lock failed".into())
        })?;
        if state.closed {
            return Err(wasmdroid_engine::EngineError::Conflict(
                "network generation ended".into(),
            ));
        }
        let routes = state
            .routes
            .clone()
            .ok_or_else(|| wasmdroid_engine::EngineError::Unsupported("no stack network".into()))?;
        drop(state);
        routes.update_with_aliases(config, aliases)
    }
    pub fn remote_pair(
        &self,
        binding: Arc<Binding>,
        source: SocketAddr,
        peer: SocketAddr,
        ttl: u32,
        opening: Arc<crate::service_broker::Opening>,
    ) -> virtual_net::Result<(Socket, Socket)> {
        let mut state = lock(&self.0)?;
        if state.closed {
            return Err(NetworkError::PermissionDenied);
        }
        state
            .connections
            .retain(|pair| pair.upgrade().is_some_and(|pair| pair.allocated()));
        if state.connections.len() >= MAX_CONNECTIONS {
            return Err(NetworkError::TooManyOpenFiles);
        }
        let (mut client, bridge) =
            TcpSocketHalf::channel_with_level_triggered_write(BUFFER_BYTES, source, peer);
        client.set_ttl(ttl)?;
        let pair = Arc::new(Connection(
            [Mutex::new(Some(client)), Mutex::new(Some(bridge))],
            wasmdroid_engine::network::lan::LanRevocation::new(),
        ));
        state.connections.push(Arc::downgrade(&pair));
        Ok((
            Socket {
                pair: pair.clone(),
                index: 0,
                reservation: Some(binding),
                opening: Some(opening),
                mapped_v4: false,
            },
            Socket {
                pair,
                index: 1,
                reservation: None,
                opening: None,
                mapped_v4: false,
            },
        ))
    }
    pub fn routed_ingress(&self, source: SocketAddr, port: u16) -> virtual_net::Result<Socket> {
        self.connect_local(
            self.reserve((Ipv4Addr::LOCALHOST, 0).into())?,
            (Ipv4Addr::LOCALHOST, port).into(),
            64,
            Some(source),
        )
    }
    pub fn ingress(&self, port: u16) -> ServiceIngress {
        ServiceIngress {
            network: self.clone(),
            port,
        }
    }
    /// Stop admission/listening while accepted connections can still drain.
    pub fn fence(&self) {
        let (bindings, egress) = {
            let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
            state.closed = true;
            let bindings = state
                .ports
                .iter()
                .filter_map(|(key, binding)| {
                    binding
                        .upgrade()
                        .filter(|binding| canonical_address(binding.address) == *key)
                })
                .collect::<Vec<_>>();
            state.ports.clear();
            (bindings, state.egress.clone())
        };
        if let Some(egress) = egress {
            egress.revoke();
        }
        for binding in bindings {
            let listener = binding
                .listener
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .upgrade();
            if let Some(listener) = listener {
                close_listener(&listener);
            }
        }
    }
    /// Revoke all remaining sockets, including accepted sockets whose listener
    /// was already closed. Never touches another service namespace.
    pub fn close(&self) {
        self.fence();
        let connections = {
            let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
            let connections = state
                .connections
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>();
            state.connections.clear();
            connections
        };
        for connection in connections {
            connection.close();
        }
    }
}

fn normalize(address: SocketAddr, binding: bool) -> virtual_net::Result<SocketAddr> {
    if !binding && address.port() == 0 {
        return Err(NetworkError::InvalidInput);
    }
    if let SocketAddr::V6(v6) = address {
        if v6.scope_id() != 0 || v6.flowinfo() != 0 {
            return Err(NetworkError::PermissionDenied);
        }
        if let Some(ip) = v6.ip().to_ipv4_mapped()
            && (ip == Ipv4Addr::LOCALHOST || (binding && ip.is_unspecified()))
        {
            return Ok(SocketAddr::new(
                Ipv4Addr::LOCALHOST.to_ipv6_mapped().into(),
                address.port(),
            ));
        }
    }
    let ip = match address.ip() {
        IpAddr::V4(ip) if ip == Ipv4Addr::LOCALHOST || (binding && ip.is_unspecified()) => {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        }
        IpAddr::V6(ip) if ip == Ipv6Addr::LOCALHOST || (binding && ip.is_unspecified()) => {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        }
        _ => return Err(NetworkError::PermissionDenied),
    };
    Ok(SocketAddr::new(ip, address.port()))
}

fn mapped_ipv4(address: SocketAddr) -> Option<Ipv4Addr> {
    match address.ip() {
        IpAddr::V6(ip) => ip.to_ipv4_mapped(),
        IpAddr::V4(_) => None,
    }
}

fn canonical_address(address: SocketAddr) -> SocketAddr {
    SocketAddr::new(address.ip().to_canonical(), address.port())
}

fn reserved_destination(ip: IpAddr) -> bool {
    let ip = ip.to_canonical();
    ip.is_loopback()
        || ip.is_unspecified()
        || matches!(ip,IpAddr::V4(ip) if ip.octets()[..3]==[10,203,0])
}

#[async_trait::async_trait]
impl VirtualNetworking for ServiceNetwork {
    async fn bind_tcp(
        &self,
        address: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        _reuse_addr: bool,
    ) -> virtual_net::Result<Box<dyn VirtualTcpBoundSocket + Sync>> {
        if reuse_port {
            return Err(NetworkError::Unsupported);
        }
        Ok(Box::new(Bound {
            binding: Some(self.reserve_tcp(address, only_v6)?),
            ttl: 64,
        }))
    }
    async fn listen_tcp(
        &self,
        address: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> virtual_net::Result<Box<dyn VirtualTcpListener + Sync>> {
        self.bind_tcp(address, only_v6, reuse_port, reuse_addr)
            .await?
            .listen()
    }
    async fn connect_tcp(
        &self,
        address: SocketAddr,
        peer: SocketAddr,
    ) -> virtual_net::Result<Box<dyn VirtualTcpSocket + Sync>> {
        // Both direct and explicit-bound connect use the same exact routing gate.
        let mapped_loopback = mapped_ipv4(peer) == Some(Ipv4Addr::LOCALHOST);
        Ok(Box::new(self.connect_from(
            self.reserve_tcp(address, !mapped_loopback)?,
            peer,
            64,
        )?))
    }
    async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> virtual_net::Result<Vec<IpAddr>> {
        if lock(&self.0)?.closed || dns_server.is_some() {
            return Err(NetworkError::PermissionDenied);
        }
        if host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("localhost.") {
            return Ok(vec![Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()]);
        }
        let (routes, egress) = {
            let state = lock(&self.0)?;
            if state.closed {
                return Err(NetworkError::PermissionDenied);
            }
            (state.routes.clone(), state.egress.clone())
        };
        if let Some(ip) = routes.as_ref().and_then(|routes| routes.resolve(host)) {
            return Ok(vec![ip]);
        }
        if host
            .trim_end_matches('.')
            .to_ascii_lowercase()
            .ends_with(".localhost")
            || routes
                .as_ref()
                .is_some_and(|routes| routes.reserved_name(host))
        {
            return Err(NetworkError::PermissionDenied);
        }
        match host.parse::<IpAddr>() {
            Ok(ip)
                if ip == IpAddr::V4(Ipv4Addr::LOCALHOST)
                    || ip == IpAddr::V6(Ipv6Addr::LOCALHOST) =>
            {
                Ok(vec![ip])
            }
            Ok(ip) if reserved_destination(ip) => Err(NetworkError::PermissionDenied),
            Ok(ip) => egress.ok_or(NetworkError::PermissionDenied)?.resolve_ip(ip),
            Err(_) => {
                let manager = egress.ok_or(NetworkError::PermissionDenied)?;
                let addresses = manager.resolve(host, port).await?;
                // A peer/alias may have become private while the platform DNS
                // request was pending. Its old public answer grants no route.
                let routes = {
                    let state = lock(&self.0)?;
                    if state.closed
                        || state.egress_fenced
                        || !state
                            .egress
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &manager))
                    {
                        return Err(NetworkError::PermissionDenied);
                    }
                    state.routes.clone()
                };
                if routes
                    .as_ref()
                    .is_some_and(|routes| routes.reserved_name(host))
                {
                    return Err(NetworkError::PermissionDenied);
                }
                manager
                    .check_current()
                    .map_err(|_| NetworkError::PermissionDenied)?;
                if addresses.iter().any(|ip| reserved_destination(*ip)) {
                    return Err(NetworkError::PermissionDenied);
                }
                Ok(addresses)
            }
        }
    }
    async fn ip_list(&self) -> virtual_net::Result<Vec<IpCidr>> {
        if lock(&self.0)?.closed {
            return Err(NetworkError::PermissionDenied);
        }
        Ok(vec![
            IpCidr {
                ip: Ipv4Addr::LOCALHOST.into(),
                prefix: 32,
            },
            IpCidr {
                ip: Ipv6Addr::LOCALHOST.into(),
                prefix: 128,
            },
        ])
    }
}

#[derive(Debug)]
struct Bound {
    binding: Option<Arc<Binding>>,
    ttl: u32,
}
impl VirtualTcpBoundSocket for Bound {
    fn addr_local(&self) -> virtual_net::Result<SocketAddr> {
        Ok(self
            .binding
            .as_ref()
            .ok_or(NetworkError::InvalidFd)?
            .address)
    }
    fn listen(&mut self) -> virtual_net::Result<Box<dyn VirtualTcpListener + Sync>> {
        let binding = self.binding.as_ref().ok_or(NetworkError::InvalidFd)?;
        let network = lock(&binding.network.0)?;
        if network.closed {
            return Err(NetworkError::PermissionDenied);
        }
        let state = Arc::new(Mutex::new(ListenerState {
            closed: false,
            backlog: VecDeque::new(),
            handler: None,
            wakers: Vec::new(),
            ttl: self.ttl as u8,
        }));
        *lock(&binding.listener)? = Arc::downgrade(&state);
        drop(network);
        Ok(Box::new(Listener {
            binding: self.binding.take().ok_or(NetworkError::InvalidFd)?,
            state,
        }))
    }
    fn connect(
        &mut self,
        peer: SocketAddr,
    ) -> virtual_net::Result<Box<dyn VirtualTcpSocket + Sync>> {
        let binding = self
            .binding
            .as_ref()
            .ok_or(NetworkError::InvalidFd)?
            .clone();
        let socket = binding
            .network
            .clone()
            .connect_from(binding, peer, self.ttl)?;
        self.binding.take();
        Ok(Box::new(socket))
    }
    fn set_ttl(&mut self, ttl: u32) -> virtual_net::Result<()> {
        if ttl > 255 {
            return Err(NetworkError::InvalidInput);
        }
        self.ttl = ttl;
        Ok(())
    }
    fn ttl(&self) -> virtual_net::Result<u32> {
        Ok(self.ttl)
    }
}

#[derive(Debug)]
struct Listener {
    binding: Arc<Binding>,
    state: Arc<Mutex<ListenerState>>,
}
fn close_listener(listener: &Mutex<ListenerState>) {
    let (backlog, wakers, handler) = {
        let mut state = listener.lock().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        (
            std::mem::take(&mut state.backlog),
            std::mem::take(&mut state.wakers),
            state.handler.take(),
        )
    };
    drop(backlog);
    if let Some(mut handler) = handler {
        handler.push_interest(virtual_mio::InterestType::Closed);
    }
    for waker in wakers {
        waker.wake();
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        close_listener(&self.state);
    }
}
impl VirtualIoSource for Listener {
    fn remove_handler(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.handler.take();
        }
    }
    fn poll_read_ready(&mut self, cx: &mut Context<'_>) -> Poll<virtual_net::Result<usize>> {
        let mut state = match lock(&self.state) {
            Ok(state) => state,
            Err(error) => return Poll::Ready(Err(error)),
        };
        if state.closed {
            return Poll::Ready(Err(NetworkError::ConnectionAborted));
        }
        if !state.backlog.is_empty() {
            return Poll::Ready(Ok(state.backlog.len()));
        }
        if !state.wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
            if state.wakers.len() >= MAX_WAKERS {
                return Poll::Ready(Err(NetworkError::TooManyOpenFiles));
            }
            state.wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
    fn poll_write_ready(&mut self, _: &mut Context<'_>) -> Poll<virtual_net::Result<usize>> {
        Poll::Ready(Err(NetworkError::Unsupported))
    }
}
impl VirtualTcpListener for Listener {
    fn try_accept(
        &mut self,
    ) -> virtual_net::Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        let mut state = lock(&self.state)?;
        if state.closed {
            return Err(NetworkError::ConnectionAborted);
        }
        let socket = state.backlog.pop_front().ok_or(NetworkError::WouldBlock)?;
        let peer = socket.addr_peer()?;
        Ok((Box::new(socket), peer))
    }
    fn set_handler(
        &mut self,
        mut handler: Box<dyn InterestHandler + Send + Sync>,
    ) -> virtual_net::Result<()> {
        let mut state = lock(&self.state)?;
        if state.closed {
            handler.push_interest(virtual_mio::InterestType::Closed);
        } else if !state.backlog.is_empty() {
            handler.push_interest(virtual_mio::InterestType::Readable);
        }
        state.handler = Some(handler);
        Ok(())
    }
    fn addr_local(&self) -> virtual_net::Result<SocketAddr> {
        Ok(self.binding.address)
    }
    fn set_ttl(&mut self, ttl: u8) -> virtual_net::Result<()> {
        lock(&self.state)?.ttl = ttl;
        Ok(())
    }
    fn ttl(&self) -> virtual_net::Result<u8> {
        Ok(lock(&self.state)?.ttl)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceIngress {
    network: ServiceNetwork,
    port: u16,
}
impl crate::http_execution::ForwardTarget for ServiceIngress {
    fn connect_forwarder(&self) -> Option<Box<dyn VirtualTcpSocket + Sync>> {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            let source = self.network.reserve(SocketAddr::new(ip, 0)).ok()?;
            if let Ok(socket) =
                self.network
                    .connect_from(source, SocketAddr::new(ip, self.port), 64)
            {
                return Some(Box::new(socket));
            }
        }
        None
    }
}

#[derive(Debug)]
pub(super) struct Socket {
    pub pair: Arc<Connection>,
    index: usize,
    reservation: Option<Arc<Binding>>,
    opening: Option<Arc<crate::service_broker::Opening>>,
    mapped_v4: bool,
}
impl Socket {
    fn call<T>(
        &self,
        op: impl FnOnce(&mut TcpSocketHalf) -> virtual_net::Result<T>,
    ) -> virtual_net::Result<T> {
        op(lock(&self.pair.0[self.index])?
            .as_mut()
            .ok_or(NetworkError::NotConnected)?)
    }
    fn display_address(&self, address: SocketAddr) -> SocketAddr {
        match address {
            SocketAddr::V4(v4) if self.mapped_v4 => {
                SocketAddr::new(v4.ip().to_ipv6_mapped().into(), v4.port())
            }
            _ => address,
        }
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        self.pair.close_half(self.index);
    }
}
impl VirtualIoSource for Socket {
    fn remove_handler(&mut self) {
        let _ = self.call(|socket| {
            socket.remove_handler();
            Ok(())
        });
    }
    fn poll_read_ready(&mut self, cx: &mut Context<'_>) -> Poll<virtual_net::Result<usize>> {
        if let Some(opening) = &self.opening {
            match opening.ready(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                _ => {}
            }
        }
        self.call(|socket| Ok(socket.poll_read_ready(cx)))
            .unwrap_or_else(|error| Poll::Ready(Err(error)))
    }
    fn poll_write_ready(&mut self, cx: &mut Context<'_>) -> Poll<virtual_net::Result<usize>> {
        if let Some(opening) = &self.opening {
            match opening.ready(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                _ => {}
            }
        }
        self.call(|socket| Ok(socket.poll_write_ready(cx)))
            .unwrap_or_else(|error| Poll::Ready(Err(error)))
    }
}
impl VirtualSocket for Socket {
    fn set_ttl(&mut self, ttl: u32) -> virtual_net::Result<()> {
        if ttl > 255 {
            return Err(NetworkError::InvalidInput);
        }
        self.call(|socket| socket.set_ttl(ttl))
    }
    fn ttl(&self) -> virtual_net::Result<u32> {
        self.call(|socket| socket.ttl())
    }
    fn addr_local(&self) -> virtual_net::Result<SocketAddr> {
        self.call(|socket| socket.addr_local())
            .map(|address| self.display_address(address))
    }
    fn status(&self) -> virtual_net::Result<SocketStatus> {
        if let Some(opening) = &self.opening {
            if opening.status() != SocketStatus::Opened {
                return Ok(opening.status());
            }
        }
        self.call(|socket| socket.status()).or_else(|error| {
            if error == NetworkError::NotConnected {
                Ok(SocketStatus::Closed)
            } else {
                Err(error)
            }
        })
    }
    fn set_handler(
        &mut self,
        handler: Box<dyn InterestHandler + Send + Sync>,
    ) -> virtual_net::Result<()> {
        let handler = if let Some(opening) = &self.opening {
            let Some(mut handler) = opening.handler(handler) else {
                return Ok(());
            };
            if opening.status() == SocketStatus::Failed {
                handler.push_interest(virtual_mio::InterestType::Error);
                handler.push_interest(virtual_mio::InterestType::Closed);
                return Ok(());
            }
            handler
        } else {
            handler
        };
        self.call(|socket| socket.set_handler(handler))
    }
    fn last_error(&self) -> virtual_net::Result<Option<NetworkError>> {
        Ok(self.opening.as_ref().and_then(|opening| opening.error()))
    }
}
impl VirtualConnectedSocket for Socket {
    fn set_linger(&mut self, linger: Option<Duration>) -> virtual_net::Result<()> {
        self.call(|socket| socket.set_linger(linger))
    }
    fn linger(&self) -> virtual_net::Result<Option<Duration>> {
        self.call(|socket| socket.linger())
    }
    fn try_send(&mut self, data: &[u8]) -> virtual_net::Result<usize> {
        if let Some(opening) = &self.opening {
            opening.check()?;
        }
        self.call(|socket| socket.try_send(data))
    }
    fn try_flush(&mut self) -> virtual_net::Result<()> {
        if let Some(opening) = &self.opening {
            opening.check()?;
        }
        self.call(|socket| socket.try_flush())
    }
    fn close(&mut self) -> virtual_net::Result<()> {
        self.pair.close_half(self.index);
        self.reservation.take();
        Ok(())
    }
    fn try_recv(&mut self, data: &mut [MaybeUninit<u8>], peek: bool) -> virtual_net::Result<usize> {
        if let Some(opening) = &self.opening {
            opening.check()?;
        }
        self.call(|socket| socket.try_recv(data, peek))
    }
}
impl VirtualTcpSocket for Socket {
    fn set_recv_buf_size(&mut self, size: usize) -> virtual_net::Result<()> {
        // Keep the actual bounded capacity; upstream shrinking silently loses
        // already buffered bytes. Always report the actual capacity on get.
        buffer_size(size)?;
        self.call(|_| Ok(()))
    }
    fn recv_buf_size(&self) -> virtual_net::Result<usize> {
        self.call(|socket| socket.recv_buf_size())
    }
    fn set_send_buf_size(&mut self, size: usize) -> virtual_net::Result<()> {
        buffer_size(size)?;
        self.call(|_| Ok(()))
    }
    fn send_buf_size(&self) -> virtual_net::Result<usize> {
        self.call(|socket| socket.send_buf_size())
    }
    fn set_nodelay(&mut self, value: bool) -> virtual_net::Result<()> {
        self.call(|socket| socket.set_nodelay(value))
    }
    fn nodelay(&self) -> virtual_net::Result<bool> {
        self.call(|socket| socket.nodelay())
    }
    fn set_keepalive(&mut self, value: bool) -> virtual_net::Result<()> {
        self.call(|socket| socket.set_keepalive(value))
    }
    fn keepalive(&self) -> virtual_net::Result<bool> {
        self.call(|socket| socket.keepalive())
    }
    fn set_dontroute(&mut self, value: bool) -> virtual_net::Result<()> {
        self.call(|socket| socket.set_dontroute(value))
    }
    fn dontroute(&self) -> virtual_net::Result<bool> {
        self.call(|socket| socket.dontroute())
    }
    fn addr_peer(&self) -> virtual_net::Result<SocketAddr> {
        self.call(|socket| socket.addr_peer())
            .map(|address| self.display_address(address))
    }
    fn shutdown(&mut self, how: Shutdown) -> virtual_net::Result<()> {
        self.call(|socket| socket.shutdown(how))
    }
    fn is_closed(&self) -> bool {
        self.call(|socket| Ok(socket.is_closed())).unwrap_or(true)
    }
}
fn buffer_size(size: usize) -> virtual_net::Result<()> {
    if size == 0 || size > BUFFER_BYTES {
        Err(NetworkError::InvalidInput)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        task::Wake,
    };
    use wasmer_wasix::virtual_net::VirtualConnectedSocketExt;

    fn address(port: u16) -> SocketAddr {
        (Ipv4Addr::LOCALHOST, port).into()
    }

    #[derive(Default)]
    struct WakeFlag(AtomicBool);

    impl Wake for WakeFlag {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    fn established_pair() -> (Socket, Socket) {
        let network = ServiceNetwork::new(true);
        let opening = Arc::new(crate::service_broker::Opening::new());
        let binding = network.reserve(address(0)).unwrap();
        let source = binding.address;
        let pair = network
            .remote_pair(binding, source, address(8080), 64, opening.clone())
            .unwrap();
        opening.finish(&pair.0.pair, None);
        pair
    }

    #[test]
    fn connected_tcp_reports_writable_on_repeated_checks_until_full() {
        let (mut client, mut peer) = established_pair();
        let wake = Arc::new(WakeFlag::default());
        let waker = Waker::from(wake.clone());
        let mut context = Context::from_waker(&waker);

        for _ in 0..2 {
            assert_eq!(
                client.poll_write_ready(&mut context),
                Poll::Ready(Ok(BUFFER_BYTES))
            );
        }
        assert_eq!(
            client.try_send(&vec![0; BUFFER_BYTES]).unwrap(),
            BUFFER_BYTES
        );
        assert!(client.poll_write_ready(&mut context).is_pending());
        assert!(!wake.0.load(Ordering::Relaxed));

        assert_eq!(
            peer.try_recv(&mut [MaybeUninit::uninit(); 4], false)
                .unwrap(),
            4
        );
        assert!(wake.0.load(Ordering::Relaxed));
        for _ in 0..2 {
            assert_eq!(client.poll_write_ready(&mut context), Poll::Ready(Ok(4)));
        }
    }

    #[test]
    fn partial_tcp_write_wakes_the_waiting_reader() {
        let (mut client, mut peer) = established_pair();
        let wake = Arc::new(WakeFlag::default());
        let waker = Waker::from(wake.clone());
        let mut context = Context::from_waker(&waker);

        assert!(peer.poll_read_ready(&mut context).is_pending());
        assert_eq!(
            client.try_send(&vec![0; BUFFER_BYTES + 1]).unwrap(),
            BUFFER_BYTES
        );
        assert!(wake.0.load(Ordering::Relaxed));
        assert_eq!(
            peer.poll_read_ready(&mut context),
            Poll::Ready(Ok(BUFFER_BYTES))
        );
    }

    #[tokio::test]
    async fn exact_routes_and_bound_connect_never_reach_another_namespace() {
        let one = ServiceNetwork::new(true);
        let two = ServiceNetwork::new(true);
        let mut a = one
            .listen_tcp(address(8080), false, false, false)
            .await
            .unwrap();
        let mut b = one
            .listen_tcp(address(8081), false, false, false)
            .await
            .unwrap();
        assert!(one.connect_tcp(address(0), address(8082)).await.is_err());
        assert!(two.connect_tcp(address(0), address(8080)).await.is_err());
        for peer in ["192.0.2.1:8080", "10.0.0.1:8080", "127.0.0.2:8080"] {
            assert!(
                one.connect_tcp(address(0), peer.parse().unwrap())
                    .await
                    .is_err()
            );
        }
        let mut source = one.bind_tcp(address(0), false, false, false).await.unwrap();
        let reserved = source.addr_local().unwrap();
        assert_ne!(0, reserved.port());
        assert!(source.connect(address(8082)).is_err());
        let mut client = source.connect(address(8081)).unwrap();
        assert!(matches!(a.try_accept(), Err(NetworkError::WouldBlock)));
        let (mut server, peer) = b.try_accept().unwrap();
        assert_eq!(reserved, peer);
        client.send(b"private").await.unwrap();
        let mut bytes = [MaybeUninit::uninit(); 16];
        let count = server.recv(&mut bytes, false).await.unwrap();
        // SAFETY: recv initialized exactly count bytes, verified by its contract.
        assert_eq!(
            unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), count) },
            b"private"
        );
        assert!(one.bind_tcp(reserved, false, false, false).await.is_err());
        client.close().unwrap();
        assert!(one.bind_tcp(reserved, false, false, false).await.is_ok());
        assert!(
            two.listen_tcp(address(8080), false, false, false)
                .await
                .is_ok()
        );
        assert_eq!(
            vec![
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(Ipv6Addr::LOCALHOST)
            ],
            one.resolve("LOCALHOST.", None, None).await.unwrap()
        );
        assert!(one.resolve("example.com", Some(443), None).await.is_err());
        assert!(one.bind_udp(address(0), false, false).await.is_err());
    }
    #[tokio::test]
    async fn dual_stack_wildcard_accepts_both_families_on_one_private_port() {
        let network = ServiceNetwork::new(true);
        let v6_wildcard = (Ipv6Addr::UNSPECIFIED, 0).into();
        let mut listener = network
            .listen_tcp(v6_wildcard, false, false, false)
            .await
            .unwrap();
        let v6 = listener.addr_local().unwrap();
        assert_eq!(v6.ip(), Ipv6Addr::LOCALHOST);
        assert_ne!(v6.port(), 0);
        let v4 = address(v6.port());
        let mapped = SocketAddr::new(Ipv4Addr::LOCALHOST.to_ipv6_mapped().into(), v6.port());

        let mut client4 = network.connect_tcp(address(0), v4).await.unwrap();
        let mut client6 = network.connect_tcp(v6_wildcard, v6).await.unwrap();
        let (mut server4, peer4) = listener.try_accept().unwrap();
        let (mut server6, peer6) = listener.try_accept().unwrap();
        assert_eq!(client4.addr_peer().unwrap(), v4);
        assert_eq!(client6.addr_peer().unwrap(), v6);
        assert_eq!(server4.addr_local().unwrap(), mapped);
        assert_eq!(mapped_ipv4(peer4), Some(Ipv4Addr::LOCALHOST));
        assert_eq!(peer4.port(), client4.addr_local().unwrap().port());
        assert_eq!(server4.addr_peer().unwrap(), peer4);
        assert_eq!(server6.addr_local().unwrap(), v6);
        assert_eq!(peer6, client6.addr_local().unwrap());
        assert!(matches!(
            listener.try_accept(),
            Err(NetworkError::WouldBlock)
        ));

        for (client, server, payload) in [
            (&mut client4, &mut server4, b"four"),
            (&mut client6, &mut server6, b"six!"),
        ] {
            client.send(payload).await.unwrap();
            let mut bytes = [MaybeUninit::uninit(); 4];
            assert_eq!(server.recv(&mut bytes, false).await.unwrap(), payload.len());
            // SAFETY: recv reported that all four bytes were initialized.
            assert_eq!(bytes.map(|byte| unsafe { byte.assume_init() }), *payload);
        }

        let other = ServiceNetwork::new(true);
        assert!(other.connect_tcp(address(0), v4).await.is_err());
        assert!(other.connect_tcp(v6_wildcard, v6).await.is_err());
        network.fence();
        assert!(matches!(
            listener.try_accept(),
            Err(NetworkError::ConnectionAborted)
        ));
        network.close();
        assert!(client4.is_closed() && client6.is_closed());
        assert!(server4.is_closed() && server6.is_closed());
    }

    #[tokio::test]
    async fn dual_stack_reservation_conflicts_atomically_and_releases_both_families() {
        let network = ServiceNetwork::new(true);
        let v4 = address(8080);
        let v6: SocketAddr = (Ipv6Addr::LOCALHOST, 8080).into();
        let wildcard: SocketAddr = (Ipv6Addr::UNSPECIFIED, 8080).into();

        let v4_bound = network.bind_tcp(v4, false, false, false).await.unwrap();
        assert!(matches!(
            network.bind_tcp(wildcard, false, false, false).await,
            Err(NetworkError::AddressInUse)
        ));
        let v6_bound = network.bind_tcp(v6, true, false, false).await.unwrap();
        drop(v4_bound);
        assert!(matches!(
            network.bind_tcp(wildcard, false, false, false).await,
            Err(NetworkError::AddressInUse)
        ));
        assert!(network.bind_tcp(v4, false, false, false).await.is_ok());
        drop(v6_bound);

        let mut bound = network
            .bind_tcp(wildcard, false, false, false)
            .await
            .unwrap();
        let listener = bound.listen().unwrap();
        assert!(matches!(
            network.bind_tcp(v4, false, false, false).await,
            Err(NetworkError::AddressInUse)
        ));
        assert!(matches!(
            network.bind_tcp(v6, true, false, false).await,
            Err(NetworkError::AddressInUse)
        ));
        drop(listener);
        assert!(network.bind_tcp(v4, false, false, false).await.is_ok());
        assert!(network.bind_tcp(v6, true, false, false).await.is_ok());

        let v6_only = network
            .listen_tcp(wildcard, true, false, false)
            .await
            .unwrap();
        assert!(network.connect_tcp(address(0), v4).await.is_err());
        drop(v6_only);
        let explicit_v6 = network.listen_tcp(v6, false, false, false).await.unwrap();
        assert!(network.connect_tcp(address(0), v4).await.is_err());
        assert!(network.bind_tcp(v4, false, false, false).await.is_ok());
        drop(explicit_v6);
        assert!(matches!(
            network.bind_tcp(wildcard, false, true, false).await,
            Err(NetworkError::Unsupported)
        ));
    }

    #[tokio::test]
    async fn mapped_ipv4_connect_and_bind_are_limited_to_private_ipv4_loopback() {
        let network = ServiceNetwork::new(true);
        let v4 = address(8080);
        let v6_wildcard = (Ipv6Addr::UNSPECIFIED, 0).into();
        let v6_target = (Ipv6Addr::LOCALHOST, v4.port()).into();
        let mapped = SocketAddr::new(Ipv4Addr::LOCALHOST.to_ipv6_mapped().into(), v4.port());
        let mut server = network.listen_tcp(v4, false, false, false).await.unwrap();
        let mut client = network.connect_tcp(v6_wildcard, mapped).await.unwrap();
        let (accepted, peer) = server.try_accept().unwrap();
        assert_eq!(client.addr_peer().unwrap(), mapped);
        assert_eq!(
            mapped_ipv4(client.addr_local().unwrap()),
            Some(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(peer.ip(), Ipv4Addr::LOCALHOST);
        assert_eq!(client.addr_local().unwrap().port(), peer.port());
        assert_eq!(accepted.addr_local().unwrap(), v4);
        client.close().unwrap();

        let mut only_v6 = network
            .bind_tcp(v6_wildcard, true, false, false)
            .await
            .unwrap();
        assert!(only_v6.connect(mapped).is_err());
        let mut dual = network
            .bind_tcp(v6_wildcard, false, false, false)
            .await
            .unwrap();
        assert!(dual.connect(mapped).is_ok());
        drop(server);

        let mut mapped_listener = network
            .listen_tcp(mapped, false, false, false)
            .await
            .unwrap();
        assert_eq!(mapped_listener.addr_local().unwrap(), mapped);
        assert!(network.connect_tcp(v6_wildcard, v6_target).await.is_err());
        assert!(network.connect_tcp(address(0), v4).await.is_ok());
        assert!(
            matches!(mapped_listener.try_accept(), Ok((_, peer)) if mapped_ipv4(peer) == Some(Ipv4Addr::LOCALHOST))
        );

        for destination in ["[::ffff:127.0.0.2]:8080", "[::ffff:192.0.2.1]:8080"] {
            let destination = destination.parse().unwrap();
            assert!(
                network
                    .bind_tcp(destination, false, false, false)
                    .await
                    .is_err()
            );
            assert!(network.connect_tcp(v6_wildcard, destination).await.is_err());
        }
    }

    #[tokio::test]
    async fn wildcard_ephemeral_rebind_and_stop_keep_socket_ownership_exact() {
        let network = ServiceNetwork::new(true);
        let mut listener = network
            .listen_tcp((Ipv4Addr::UNSPECIFIED, 0).into(), false, false, true)
            .await
            .unwrap();
        let target = listener.addr_local().unwrap();
        assert_eq!(target.ip(), Ipv4Addr::LOCALHOST);
        assert_ne!(target.port(), 0);
        let mut client = network.connect_tcp(address(0), target).await.unwrap();
        let (mut server, _) = listener.try_accept().unwrap();
        drop(listener);
        let _new = network
            .listen_tcp(target, false, false, true)
            .await
            .unwrap();
        client.send(b"retained bytes").await.unwrap();
        // Buffer options remain advisory within the fixed 64 KiB bound: reducing
        // a request cannot trigger upstream's lossy resize of queued bytes.
        server.set_recv_buf_size(1).unwrap();
        assert_eq!(BUFFER_BYTES, server.recv_buf_size().unwrap());
        assert!(server.set_recv_buf_size(BUFFER_BYTES + 1).is_err());
        let mut bytes = [MaybeUninit::uninit(); 32];
        assert_eq!(14, server.recv(&mut bytes, false).await.unwrap());
        network.fence();
        client.send(b"drain").await.unwrap();
        assert_eq!(5, server.recv(&mut bytes, false).await.unwrap());
        assert!(network.connect_tcp(address(0), target).await.is_err());
        network.close();
        assert!(client.is_closed());
        assert!(server.is_closed());
        assert!(
            network
                .bind_tcp(address(0), false, false, false)
                .await
                .is_err()
        );
        assert!(network.resolve("localhost", None, None).await.is_err());
        assert!(client.try_send(b"after stop").is_err());
        let disabled = ServiceNetwork::new(false);
        assert!(
            disabled
                .bind_tcp(address(0), false, false, false)
                .await
                .is_err()
        );
        assert!(disabled.resolve("127.0.0.1", None, None).await.is_err());
    }
    #[tokio::test]
    async fn backlogs_connections_ports_and_buffers_have_bounded_admission() {
        let network = ServiceNetwork::new(true);
        let mut listener = network
            .listen_tcp(address(8080), false, false, false)
            .await
            .unwrap();
        let mut clients = Vec::new();
        for _ in 0..MAX_BACKLOG {
            clients.push(
                network
                    .connect_tcp(address(0), address(8080))
                    .await
                    .unwrap(),
            );
        }
        assert!(
            network
                .connect_tcp(address(0), address(8080))
                .await
                .is_err()
        );
        let (accepted, _) = listener.try_accept().unwrap();
        assert!(network.connect_tcp(address(0), address(8080)).await.is_ok());
        drop(accepted);
        drop(clients);
        drop(listener);
        let mut ports = Vec::new();
        for _ in 0..MAX_PORTS {
            ports.push(
                network
                    .bind_tcp(address(0), false, false, false)
                    .await
                    .unwrap(),
            );
        }
        assert!(
            network
                .bind_tcp(address(0), false, false, false)
                .await
                .is_err()
        );
        ports.pop();
        assert!(
            network
                .bind_tcp(address(0), false, false, false)
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn retained_accepted_peers_cannot_bypass_the_connection_budget() {
        let network = ServiceNetwork::new(true);
        let mut listener = network
            .listen_tcp(address(8080), false, false, false)
            .await
            .unwrap();
        let mut peers = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            let mut client = network
                .connect_tcp(address(0), address(8080))
                .await
                .unwrap();
            peers.push(listener.try_accept().unwrap().0);
            client.close().unwrap();
        }
        assert!(
            network
                .connect_tcp(address(0), address(8080))
                .await
                .is_err()
        );
        peers[0].close().unwrap(); // No live channel buffers remain in that pair.
        assert!(network.connect_tcp(address(0), address(8080)).await.is_ok());
    }
}

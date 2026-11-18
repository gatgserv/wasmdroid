//! Transient Wi-Fi ingress, separate from the local loopback port contract.
//! The trusted platform owner must attest the selected private interface, bind
//! its unconnected socket to that network, and revoke on generation/network/
//! permission change. This primitive grants no Android permission or guest egress.
use crate::{EngineError, Result, api::validate_name};
use std::{
    future::Future,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, copy_bidirectional},
    net::{TcpListener, TcpSocket, TcpStream},
    sync::Notify,
    task::{JoinHandle, JoinSet},
};

const MAX_CLIENTS: usize = 32;
const MAX_CONNECT_TIME: Duration = Duration::from_secs(30);

/// Private owner capability metadata, never a public-FFI authorization recipe.
/// No mutable address, persistent grant, wildcard, DNS or endpoint fallback.
#[derive(Clone, PartialEq, Eq)]
pub struct LanScope {
    instance_id: String,
    generation: String,
    guest_port: u16,
    interface_epoch: u64,
    private_address: Ipv4Addr,
    ipv6: Option<LanIpv6>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LanIpv6 {
    address: Ipv6Addr,
    prefix_len: u8,
}
impl LanIpv6 {
    pub fn new(address: Ipv6Addr, prefix_len: u8) -> Result<Self> {
        let first = address.octets()[0];
        if !(first & 0xe0 == 0x20 || first & 0xfe == 0xfc) || !(64..128).contains(&prefix_len) {
            return Err(EngineError::Unsupported(
                "Wi-Fi IPv6 sharing requires an on-link unicast address and prefix".into(),
            ));
        }
        Ok(Self {
            address,
            prefix_len,
        })
    }
    pub fn address(self) -> Ipv6Addr {
        self.address
    }
    fn allows_peer(self, address: Ipv6Addr) -> bool {
        let shift = 128 - self.prefix_len;
        u128::from(address) >> shift == u128::from(self.address) >> shift
    }
}
impl std::fmt::Debug for LanScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LanScope(<private generation/interface scope>)")
    }
}
impl LanScope {
    pub fn new(
        instance_id: String,
        generation: String,
        guest_port: u16,
        interface_epoch: u64,
        private_address: Ipv4Addr,
    ) -> Result<Self> {
        validate_name(&instance_id)?;
        validate_name(&generation)?;
        if guest_port == 0 || interface_epoch == 0 {
            return Err(EngineError::Invalid("invalid LAN endpoint scope".into()));
        }
        if !private_address.is_private() {
            return Err(EngineError::Unsupported(
                "LAN ingress requires an explicitly selected RFC1918 IPv4 address".into(),
            ));
        }
        Ok(Self {
            instance_id,
            generation,
            guest_port,
            interface_epoch,
            private_address,
            ipv6: None,
        })
    }
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn guest_port(&self) -> u16 {
        self.guest_port
    }
    pub fn interface_epoch(&self) -> u64 {
        self.interface_epoch
    }
    pub fn private_address(&self) -> Ipv4Addr {
        self.private_address
    }
    pub fn with_ipv6(mut self, ipv6: Option<LanIpv6>) -> Self {
        self.ipv6 = ipv6;
        self
    }
    pub fn ipv6(&self) -> Option<LanIpv6> {
        self.ipv6
    }
}

/// Explicit technical bounds. No product policy or resource-limit claim.
#[derive(Clone, Copy, Debug)]
pub struct LanLimits {
    clients: usize,
    connect_timeout: Duration,
}
impl LanLimits {
    pub fn new(clients: usize, connect_timeout: Duration) -> Result<Self> {
        if !(1..=MAX_CLIENTS).contains(&clients)
            || connect_timeout.is_zero()
            || connect_timeout > MAX_CONNECT_TIME
        {
            return Err(EngineError::Invalid(
                "LAN transport bounds are unsupported".into(),
            ));
        }
        Ok(Self {
            clients,
            connect_timeout,
        })
    }
}

/// A listener is retained from bind through terminal forwarding cleanup. The
/// network binder is mandatory, runs BEFORE bind/listen, and must fail closed.
pub struct LanLease {
    scope: LanScope,
    listener: TcpListener,
    ipv6_listener: Option<TcpListener>,
}
impl LanLease {
    pub fn bind(
        scope: LanScope,
        host_port: u16,
        mut bind_selected_network: impl FnMut(&TcpSocket, &LanScope) -> Result<()>,
    ) -> Result<Self> {
        require_executor()?;
        let socket = TcpSocket::new_v4()?;
        bind_selected_network(&socket, &scope)?;
        socket.bind(SocketAddr::from((scope.private_address, host_port)))?;
        let listener = socket.listen(MAX_CLIENTS as u32)?;
        if listener.local_addr()?.ip() != scope.private_address {
            return Err(EngineError::Conflict("LAN listener address changed".into()));
        }
        let ipv6_listener = if let Some(ipv6) = scope.ipv6 {
            let socket = TcpSocket::new_v6()?;
            bind_selected_network(&socket, &scope)?;
            let address = SocketAddr::from((ipv6.address, listener.local_addr()?.port()));
            socket.bind(address)?;
            let listener = socket.listen(MAX_CLIENTS as u32)?;
            if listener.local_addr()? != address {
                return Err(EngineError::Conflict(
                    "LAN IPv6 listener address changed".into(),
                ));
            }
            Some(listener)
        } else {
            None
        };
        Ok(Self {
            scope,
            listener,
            ipv6_listener,
        })
    }
    pub fn local_address(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }
    pub fn forward(
        self,
        connector: Arc<dyn GenerationConnector>,
        limits: LanLimits,
    ) -> Result<LanForwarder> {
        start_forwarding(
            self.scope,
            self.listener,
            self.ipv6_listener,
            connector,
            limits,
        )
    }
}

pub trait DuplexStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> DuplexStream for T {}

/// The connector's actual private capability must own this exact generation and
/// guest port. It must not resolve a remembered host port or detach network work.
pub struct ScopedStream {
    scope: LanScope,
    stream: Box<dyn DuplexStream>,
}
impl ScopedStream {
    pub fn new(scope: LanScope, stream: impl DuplexStream + 'static) -> Self {
        Self {
            scope,
            stream: Box::new(stream),
        }
    }
}
pub type ConnectFuture = Pin<Box<dyn Future<Output = Result<ScopedStream>> + Send + 'static>>;
pub trait GenerationConnector: Send + Sync {
    fn scope(&self) -> &LanScope;
    /// Dropping the future must release its pending connection and stream.
    /// No blocking polling, detached tasks or reference-counted escape of the
    /// private stream is permitted; propagate the supplied revocation token.
    fn connect(&self, scope: LanScope, revoked: LanRevocation) -> ConnectFuture;
}

struct Gate {
    revoked: AtomicBool,
    closed: AtomicBool,
    changed: Notify,
    finished: Notify,
}
#[derive(Clone)]
pub struct LanRevocation(Arc<Gate>);
impl Default for LanRevocation {
    fn default() -> Self {
        Self::new()
    }
}
impl LanRevocation {
    /// Cancellation only: this grants no listener, scope, stream or permission.
    pub fn new() -> Self {
        Self(Arc::new(Gate {
            revoked: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            changed: Notify::new(),
            finished: Notify::new(),
        }))
    }
    pub fn revoke(&self) {
        if !self.0.revoked.swap(true, Ordering::AcqRel) {
            self.0.changed.notify_waiters();
        }
    }
    pub fn is_revoked(&self) -> bool {
        self.0.revoked.load(Ordering::Acquire)
    }
    pub fn is_closed(&self) -> bool {
        self.0.closed.load(Ordering::Acquire)
    }
    pub async fn revoked(&self) {
        loop {
            let wait = self.0.changed.notified();
            tokio::pin!(wait);
            wait.as_mut().enable();
            if self.is_revoked() {
                return;
            }
            wait.await;
        }
    }
    /// Only completed listener/connection cleanup signals this receipt. A panic
    /// does not fabricate closure; the owner must keep uncertain resources owned.
    pub async fn closed(&self) {
        loop {
            let wait = self.0.finished.notified();
            tokio::pin!(wait);
            wait.as_mut().enable();
            if self.is_closed() {
                return;
            }
            wait.await;
        }
    }
    fn finish(&self) {
        self.0.closed.store(true, Ordering::Release);
        self.0.finished.notify_waiters();
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LanReport {
    pub accepted: u64,
    pub rejected: u64,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
}

pub struct LanForwarder {
    scope: LanScope,
    address: SocketAddr,
    revoked: LanRevocation,
    task: Option<JoinHandle<Result<LanReport>>>,
}
impl LanForwarder {
    pub fn scope(&self) -> &LanScope {
        &self.scope
    }
    pub fn local_address(&self) -> SocketAddr {
        self.address
    }
    pub fn revocation(&self) -> LanRevocation {
        self.revoked.clone()
    }
    /// The owner retains this private future until actual terminal cleanup.
    /// Cancelling a UI observer must never complete or drop this owner operation.
    pub async fn shutdown(mut self) -> Result<LanReport> {
        self.revoked.revoke();
        self.task
            .take()
            .expect("forwarder task is owned")
            .await
            .map_err(|_| EngineError::Conflict("LAN transport cleanup was not confirmed".into()))?
    }
}
impl Drop for LanForwarder {
    fn drop(&mut self) {
        // The detached owner task observes revocation and awaits every child.
        // A retained revocation handle can still await the real closed receipt.
        self.revoked.revoke();
    }
}

fn require_executor() -> Result<tokio::runtime::Handle> {
    tokio::runtime::Handle::try_current()
        .map_err(|_| EngineError::Unsupported("LAN transport requires an async executor".into()))
}
fn start_forwarding(
    scope: LanScope,
    listener: TcpListener,
    ipv6_listener: Option<TcpListener>,
    connector: Arc<dyn GenerationConnector>,
    limits: LanLimits,
) -> Result<LanForwarder> {
    let executor = require_executor()?;
    if connector.scope() != &scope {
        return Err(EngineError::Conflict("LAN connector scope changed".into()));
    }
    let address = listener.local_addr()?;
    let revoked = LanRevocation::new();
    let running = revoked.clone();
    let target = scope.clone();
    let task = executor.spawn(async move {
        let result = serve(listener, ipv6_listener, connector, target, limits, &running).await;
        running.finish();
        result
    });
    Ok(LanForwarder {
        scope,
        address,
        revoked,
        task: Some(task),
    })
}

enum ConnectionOutcome {
    Completed,
    Failed,
    Cancelled,
}
fn account(
    report: &mut LanReport,
    outcome: std::result::Result<ConnectionOutcome, tokio::task::JoinError>,
) {
    match outcome {
        Ok(ConnectionOutcome::Completed) => report.completed = report.completed.saturating_add(1),
        Ok(ConnectionOutcome::Cancelled) => report.cancelled = report.cancelled.saturating_add(1),
        Err(error) if error.is_cancelled() => report.cancelled = report.cancelled.saturating_add(1),
        _ => report.failed = report.failed.saturating_add(1),
    }
}
async fn serve(
    listener: TcpListener,
    ipv6_listener: Option<TcpListener>,
    connector: Arc<dyn GenerationConnector>,
    scope: LanScope,
    limits: LanLimits,
    revoked: &LanRevocation,
) -> Result<LanReport> {
    let mut report = LanReport::default();
    let mut connections = JoinSet::new();
    let mut listener_error = None;
    loop {
        tokio::select! {
            biased;
            _ = revoked.revoked() => break,
            result = connections.join_next(), if !connections.is_empty() => {
                if let Some(result) = result { account(&mut report, result); }
            }
            accepted = accept(&listener, ipv6_listener.as_ref()) => {
                let (host, peer) = match accepted {
                    Ok(value) => value,
                    Err(error) => { listener_error = Some(error); break; }
                };
                if revoked.is_revoked() { break; }
                let peer_allowed = match peer {
                    SocketAddr::V4(_) => true,
                    SocketAddr::V6(address) => scope.ipv6.is_some_and(|ip| ip.allows_peer(*address.ip())),
                };
                if !peer_allowed || connections.len() >= limits.clients {
                    report.rejected = report.rejected.saturating_add(1);
                    continue;
                }
                report.accepted = report.accepted.saturating_add(1);
                connections.spawn(bridge(host, connector.clone(), scope.clone(), revoked.clone(), limits.connect_timeout));
            }
        }
    }
    // No new accept, connect or copy survives the cleanup receipt. Explicit
    // cancellation aborts streams; ordinary EOF uses TCP half-close below.
    revoked.revoke();
    connections.abort_all();
    while let Some(result) = connections.join_next().await {
        account(&mut report, result);
    }
    // Keep the port reservation through the final child destructor. A new
    // listener cannot take this endpoint during old-generation cleanup.
    drop(listener);
    drop(ipv6_listener);
    if let Some(error) = listener_error {
        return Err(error.into());
    }
    Ok(report)
}

async fn accept(
    listener: &TcpListener,
    ipv6_listener: Option<&TcpListener>,
) -> std::io::Result<(TcpStream, SocketAddr)> {
    if let Some(ipv6_listener) = ipv6_listener {
        tokio::select! {
            value = listener.accept() => value,
            value = ipv6_listener.accept() => value,
        }
    } else {
        listener.accept().await
    }
}
async fn bridge(
    mut host: TcpStream,
    connector: Arc<dyn GenerationConnector>,
    scope: LanScope,
    revoked: LanRevocation,
    connect_timeout: Duration,
) -> ConnectionOutcome {
    let mut target = tokio::select! {
        biased;
        _ = revoked.revoked() => return ConnectionOutcome::Cancelled,
        result = tokio::time::timeout(connect_timeout, connector.connect(scope.clone(), revoked.clone())) => {
            match result {
                Ok(Ok(stream)) => stream,
                _ => return ConnectionOutcome::Failed,
            }
        }
    };
    if target.scope != scope {
        return ConnectionOutcome::Failed;
    }
    if revoked.is_revoked() {
        return ConnectionOutcome::Cancelled;
    }
    tokio::select! {
        biased;
        _ = revoked.revoked() => ConnectionOutcome::Cancelled,
        result = copy_bidirectional(&mut host, &mut target.stream) => {
            if result.is_ok() { ConnectionOutcome::Completed } else { ConnectionOutcome::Failed }
        }
    }
}

#[cfg(test)]
#[path = "lan_tests.rs"]
mod tests;

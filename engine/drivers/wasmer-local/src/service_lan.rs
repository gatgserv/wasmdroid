//! Actual worker namespace ingress. Public addresses never select a guest;
//! immutable worker identity and the original published TCP guest ports do.
use crate::{
    http_execution::ForwardTarget,
    service_network::{ServiceIngress, ServiceNetwork},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpSocket, runtime::Handle, sync::Notify};
use wasmdroid_engine::{
    EngineError, Result,
    network::lan::{
        ConnectFuture, GenerationConnector, LanLease, LanLimits, LanReport, LanRevocation,
        LanScope, ScopedStream,
    },
};
use wasmer_wasix::virtual_net::VirtualTcpSocket;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanPhase {
    Preparing,
    Active,
    Revoking,
    Closed,
    Uncertain,
}
#[derive(Debug)]
pub struct LanStatus {
    pub scope: LanScope,
    pub address: Option<SocketAddr>,
    pub phase: LanPhase,
    pub failed: bool,
}

struct Completion {
    report: Option<LanReport>,
    closed: bool,
    failed: bool,
}
struct ShareState {
    address: Option<SocketAddr>,
    transport: Option<LanRevocation>,
    completion: Option<Completion>,
}
struct Share {
    scope: LanScope,
    revoked: LanRevocation,
    state: Mutex<ShareState>,
    changed: Notify,
}
impl Share {
    fn new(scope: LanScope) -> Self {
        Self {
            scope,
            revoked: LanRevocation::new(),
            state: Mutex::new(ShareState {
                address: None,
                transport: None,
                completion: None,
            }),
            changed: Notify::new(),
        }
    }
    fn revoke(&self) {
        self.revoked.revoke();
        // Poison recovery is cleanup-only; it never grants new admission.
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(transport) = &state.transport {
            transport.revoke();
        }
    }
    async fn wait(&self) -> Result<LanReport> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let state = self.state.lock().map_err(|_| lock_error())?;
                if let Some(completion) = &state.completion {
                    if completion.failed || !completion.closed {
                        return Err(EngineError::Conflict(
                            "LAN cleanup failed or was not confirmed".into(),
                        ));
                    }
                    return completion.report.ok_or_else(|| {
                        EngineError::Conflict("LAN cleanup receipt is missing".into())
                    });
                }
            }
            notified.await;
        }
    }
}
struct State {
    shares: BTreeMap<u16, Arc<Share>>,
    epochs: BTreeMap<u16, u64>,
    completed: BTreeMap<u16, Arc<Share>>,
}

pub(crate) struct ServiceLan {
    id: String,
    generation: String,
    ports: BTreeSet<u16>,
    network: ServiceNetwork,
    executor: Handle,
    generation_open: Arc<AtomicBool>,
    state: Mutex<State>,
}
impl ServiceLan {
    pub(crate) fn new(
        id: String,
        generation: String,
        ports: BTreeSet<u16>,
        network: ServiceNetwork,
        executor: Handle,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            generation,
            ports,
            network,
            executor,
            generation_open: Arc::new(AtomicBool::new(true)),
            state: Mutex::new(State {
                shares: BTreeMap::new(),
                epochs: BTreeMap::new(),
                completed: BTreeMap::new(),
            }),
        })
    }
    fn check_scope(&self, scope: &LanScope) -> Result<()> {
        if scope.instance_id() != self.id || scope.generation() != self.generation {
            return Err(EngineError::Conflict(
                "LAN scope does not own this worker generation".into(),
            ));
        }
        if !self.ports.contains(&scope.guest_port()) {
            return Err(EngineError::Unsupported(
                "LAN sharing requires a published TCP guest port".into(),
            ));
        }
        Ok(())
    }
    fn find(&self, scope: &LanScope) -> Result<Arc<Share>> {
        self.check_scope(scope)?;
        let state = self.state.lock().map_err(|_| lock_error())?;
        let share = state
            .shares
            .get(&scope.guest_port())
            .or_else(|| state.completed.get(&scope.guest_port()))
            .ok_or_else(|| EngineError::NotFound("LAN share".into()))?;
        if share.scope != *scope {
            return Err(EngineError::Conflict("LAN share scope changed".into()));
        }
        Ok(share.clone())
    }
    pub(crate) fn enable(
        self: &Arc<Self>,
        scope: LanScope,
        host_port: u16,
        limits: LanLimits,
        mut bind_selected_network: impl FnMut(&TcpSocket, &LanScope) -> Result<()>,
    ) -> Result<SocketAddr> {
        self.check_scope(&scope)?;
        let share = Arc::new(Share::new(scope.clone()));
        {
            let mut state = self.state.lock().map_err(|_| lock_error())?;
            if !self.generation_open.load(Ordering::Acquire)
                || state.shares.contains_key(&scope.guest_port())
            {
                return Err(EngineError::Conflict(
                    "LAN generation is closed or this port is owned".into(),
                ));
            }
            if state
                .epochs
                .get(&scope.guest_port())
                .is_some_and(|previous| *previous >= scope.interface_epoch())
            {
                return Err(EngineError::Conflict(
                    "LAN grant epoch must increase for every enable".into(),
                ));
            }
            state
                .epochs
                .insert(scope.guest_port(), scope.interface_epoch());
            state.completed.remove(&scope.guest_port());
            state.shares.insert(scope.guest_port(), share.clone());
        }
        // No driver/manager lock is held across the platform network binder.
        let prepared = (|| {
            let _entered = self.executor.enter();
            let lease = LanLease::bind(scope.clone(), host_port, |socket, selected| {
                if share.revoked.is_revoked() || !self.generation_open.load(Ordering::Acquire) {
                    return Err(EngineError::Cancelled);
                }
                bind_selected_network(socket, selected)?;
                if share.revoked.is_revoked() || !self.generation_open.load(Ordering::Acquire) {
                    return Err(EngineError::Cancelled);
                }
                Ok(())
            })?;
            if share.revoked.is_revoked() || !self.generation_open.load(Ordering::Acquire) {
                return Err(EngineError::Cancelled);
            }
            let connector = Arc::new(IngressConnector {
                scope,
                ingress: self.network.ingress(share.scope.guest_port()),
                revoked: share.revoked.clone(),
                generation_open: self.generation_open.clone(),
            });
            lease.forward(connector, limits)
        })();
        let forwarder = match prepared {
            Ok(value) => value,
            Err(error) => {
                // No transport task escaped: the local lease/socket has dropped.
                self.finish(&share, None, true, true);
                return Err(error);
            }
        };
        let address = forwarder.local_address();
        let transport = forwarder.revocation();
        {
            let mut state = share
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.address = Some(address);
            state.transport = Some(transport.clone());
        }
        let owner = self.clone();
        let actual = share.clone();
        // The private task retains the real forwarder even if a caller abandons
        // enable/disable. Only an actual closed receipt removes ownership.
        self.executor.spawn(async move {
            tokio::select! { biased; _ = actual.revoked.revoked() => {}, _ = transport.closed() => {} }
            let result = forwarder.shutdown().await;
            let closed = transport.is_closed();
            match result {
                Ok(report) => owner.finish(&actual, Some(report), closed, false),
                Err(_) => owner.finish(&actual, None, closed, true),
            }
        });
        if share.revoked.is_revoked() || !self.generation_open.load(Ordering::Acquire) {
            share.revoke();
            return Err(EngineError::Cancelled);
        }
        Ok(address)
    }
    fn finish(&self, share: &Arc<Share>, report: Option<LanReport>, closed: bool, failed: bool) {
        {
            // Publish terminal receipt and ownership release as one operation.
            // A late disable observer must not ACK before fresh enable can see
            // the port free. Lock order matches drained(): manager, then share.
            let mut manager = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let mut state = share
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.completion = Some(Completion {
                report,
                closed,
                failed,
            });
            if closed
                && manager
                    .shares
                    .get(&share.scope.guest_port())
                    .is_some_and(|current| Arc::ptr_eq(current, share))
            {
                manager.shares.remove(&share.scope.guest_port());
                manager
                    .completed
                    .insert(share.scope.guest_port(), share.clone());
            }
        }
        share.changed.notify_waiters();
    }
    pub(crate) fn revoke(&self, scope: &LanScope) -> Result<()> {
        self.find(scope)?.revoke();
        Ok(())
    }
    pub(crate) fn revoke_all(&self) {
        let shares = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .shares
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for share in shares {
            share.revoke();
        }
    }
    pub(crate) fn close_generation(&self) {
        self.generation_open.store(false, Ordering::Release);
        self.revoke_all();
    }
    pub(crate) async fn disable(&self, scope: &LanScope) -> Result<LanReport> {
        let share = self.find(scope)?;
        share.revoke();
        share.wait().await
    }
    pub(crate) fn disable_blocking(&self, scope: &LanScope) -> Result<LanReport> {
        if Handle::try_current().is_ok() {
            return Err(EngineError::Conflict(
                "blocking LAN cleanup requires a worker control thread".into(),
            ));
        }
        self.executor.block_on(self.disable(scope))
    }
    pub(crate) fn status(&self, scope: &LanScope) -> Result<LanStatus> {
        let share = self.find(scope)?;
        let state = share.state.lock().map_err(|_| lock_error())?;
        let phase = if state.completion.as_ref().is_some_and(|value| value.closed) {
            LanPhase::Closed
        } else if state.completion.as_ref().is_some_and(|value| !value.closed) {
            LanPhase::Uncertain
        } else if share.revoked.is_revoked() {
            LanPhase::Revoking
        } else if state.address.is_some() {
            LanPhase::Active
        } else {
            LanPhase::Preparing
        };
        Ok(LanStatus {
            scope: share.scope.clone(),
            address: state.address,
            phase,
            failed: state.completion.as_ref().is_some_and(|value| value.failed),
        })
    }
    pub(crate) fn drained(&self) -> Result<bool> {
        let state = self.state.lock().map_err(|_| lock_error())?;
        for share in state.shares.values() {
            if share
                .state
                .lock()
                .map_err(|_| lock_error())?
                .completion
                .as_ref()
                .is_some_and(|value| !value.closed)
            {
                return Err(EngineError::Conflict(
                    "LAN cleanup was not confirmed".into(),
                ));
            }
        }
        Ok(state.shares.is_empty())
    }
}
fn lock_error() -> EngineError {
    EngineError::Conflict("worker LAN state lock failed".into())
}

#[derive(Clone)]
struct IngressConnector {
    scope: LanScope,
    ingress: ServiceIngress,
    revoked: LanRevocation,
    generation_open: Arc<AtomicBool>,
}
impl IngressConnector {
    fn cancelled(&self, revoked: &LanRevocation) -> bool {
        revoked.is_revoked()
            || self.revoked.is_revoked()
            || !self.generation_open.load(Ordering::Acquire)
    }
    async fn socket(
        &self,
        scope: &LanScope,
        revoked: &LanRevocation,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        if scope != &self.scope {
            return Err(EngineError::Conflict("LAN connector scope changed".into()));
        }
        loop {
            if self.cancelled(revoked) {
                return Err(EngineError::Cancelled);
            }
            if let Some(mut socket) = self.ingress.connect_forwarder() {
                if self.cancelled(revoked) {
                    let _ = socket.close();
                    return Err(EngineError::Cancelled);
                }
                return Ok(socket);
            }
            tokio::select! { biased;
                _ = revoked.revoked() => return Err(EngineError::Cancelled),
                _ = self.revoked.revoked() => return Err(EngineError::Cancelled),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {},
            }
        }
    }
}
impl GenerationConnector for IngressConnector {
    fn scope(&self) -> &LanScope {
        &self.scope
    }
    fn connect(&self, scope: LanScope, revoked: LanRevocation) -> ConnectFuture {
        let connector = self.clone();
        Box::pin(async move {
            Ok(ScopedStream::new(
                scope.clone(),
                connector.socket(&scope, &revoked).await?,
            ))
        })
    }
}

#[cfg(test)]
#[path = "service_lan_tests.rs"]
mod tests;

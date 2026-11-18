//! Private injected outbound TCP mechanism, NOT product network authorization.
//! No ambient policy, resolver, Network, persistent grant or protocol fallback.
use crate::{
    service_broker::Opening,
    service_network::{Binding, Connection, ServiceNetwork, Socket},
};
use std::{
    collections::BTreeMap,
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{
    net::TcpSocket,
    runtime::Handle,
    sync::{Notify, mpsc, oneshot},
    task::JoinSet,
    time::Instant,
};
use wasmdroid_engine::{
    EngineError, Result,
    api::{InstanceSpec, InstanceState, NetworkMode, validate_name},
    network::lan::LanRevocation,
    session::{EngineOwnership, EngineSession},
};
use wasmer_wasix::virtual_net::{self, NetworkError, VirtualTcpSocket};

/// Possession of a clone preserves the private capture identity. Reconstructing
/// matching strings/numbers does NOT recreate this authority; it is not serde.
#[derive(Clone)]
pub struct EgressScope {
    instance_id: String,
    generation: String,
    spec_revision: u64,
    grant_epoch: u64,
    network_epoch: u64,
    seal: Arc<ScopeSeal>,
}
struct ScopeSeal {
    native: Option<NativeCapture>,
}
struct NativeCapture {
    root: std::path::PathBuf,
    specification: String,
    authority: String,
    owner: EngineOwnership,
    identity: String,
    phase: NativePhase,
    effective: Arc<wasmdroid_engine::state::EffectiveWorkerSpec>,
    stop: LanRevocation,
}
#[derive(Clone, Copy)]
enum NativePhase {
    Starting,
    Running,
}
impl std::fmt::Debug for EgressScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EgressScope(<private capability>)")
    }
}
impl PartialEq for EgressScope {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.seal, &other.seal)
            && self.instance_id == other.instance_id
            && self.generation == other.generation
            && self.spec_revision == other.spec_revision
            && self.grant_epoch == other.grant_epoch
            && self.network_epoch == other.network_epoch
    }
}
impl Eq for EgressScope {}
impl EgressScope {
    /// Trusted host capture only. No public-FFI request may manufacture this.
    pub fn new(
        instance_id: String,
        generation: String,
        spec_revision: u64,
        grant_epoch: u64,
        network_epoch: u64,
    ) -> Result<Self> {
        validate_name(&instance_id)?;
        validate_name(&generation)?;
        if grant_epoch == 0
            || network_epoch == 0
            || grant_epoch > i64::MAX as u64
            || network_epoch > i64::MAX as u64
            || spec_revision > i64::MAX as u64
        {
            return Err(invalid());
        }
        Ok(Self {
            instance_id,
            generation,
            spec_revision,
            grant_epoch,
            network_epoch,
            seal: Arc::new(ScopeSeal { native: None }),
        })
    }
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn spec_revision(&self) -> u64 {
        self.spec_revision
    }
    pub fn grant_epoch(&self) -> u64 {
        self.grant_epoch
    }
    pub fn network_epoch(&self) -> u64 {
        self.network_epoch
    }
    pub(crate) fn capture(
        session: &EngineSession,
        id: &str,
        generation: &str,
        spec: &InstanceSpec,
        grant: u64,
        network: u64,
    ) -> Result<Self> {
        let record = session.state().instance(id)?;
        let specification = serde_json::to_string(spec)?;
        let authority = session.state().run_authority(id)?;
        let effective = Arc::new(session.state().effective_worker_spec(id)?);
        effective.validate_starting(session.state(), generation)?;
        if record.spec.endpoint_id != "local"
            || (!effective.internet() && !effective.lan())
            || spec.network == NetworkMode::None
            || !matches!(record.state, InstanceState::Created | InstanceState::Exited)
            || serde_json::to_string(&record.spec)? != specification
            || !authority.desired_run
            || authority.generation.as_deref() != Some(generation)
            || authority.outcome != wasmdroid_engine::state::RunOutcome::Starting
        {
            return Err(EngineError::Conflict(
                "outbound startup authority is absent or stale".into(),
            ));
        }
        let mut scope = Self::new(id.into(), generation.into(), record.version, grant, network)?;
        scope.seal = Arc::new(ScopeSeal {
            native: Some(NativeCapture {
                root: session.root().into(),
                specification,
                authority: serde_json::to_string(&authority)?,
                owner: session.ownership(),
                identity: session.state().browser_identity(id)?,
                phase: NativePhase::Starting,
                effective,
                stop: LanRevocation::new(),
            }),
        });
        Ok(scope)
    }
    pub(crate) fn native_owner(&self) -> Result<EngineOwnership> {
        self.seal
            .native
            .as_ref()
            .map(|native| native.owner.clone())
            .ok_or_else(|| {
                EngineError::Conflict("outbound scope lacks native startup authority".into())
            })
    }
    pub(crate) fn validate_native(
        &self,
        session: &EngineSession,
        spec: &InstanceSpec,
    ) -> Result<()> {
        let native = self.seal.native.as_ref().ok_or_else(|| {
            EngineError::Conflict("outbound scope lacks native startup authority".into())
        })?;
        match native.phase {
            NativePhase::Starting => native
                .effective
                .validate_starting(session.state(), &self.generation)?,
            NativePhase::Running => native
                .effective
                .validate_running(session.state(), &self.generation)?,
        }
        self.validate_snapshot(session, spec)
    }
    fn validate_snapshot(&self, session: &EngineSession, spec: &InstanceSpec) -> Result<()> {
        let native = self
            .seal
            .native
            .as_ref()
            .ok_or_else(|| EngineError::Conflict("native outbound proof is absent".into()))?;
        let current = session.state().instance(&self.instance_id)?;
        if session.root() != native.root
            || current.version != self.spec_revision
            || !(match native.phase {
                NativePhase::Starting => matches!(
                    current.state,
                    InstanceState::Created | InstanceState::Exited
                ),
                NativePhase::Running => current.state == InstanceState::Running,
            })
            || native.stop.is_revoked()
            || session.state().browser_identity(&self.instance_id)? != native.identity
            || serde_json::to_string(spec)? != native.specification
            || serde_json::to_string(&current.spec)? != native.specification
            || serde_json::to_string(&session.state().run_authority(&self.instance_id)?)?
                != native.authority
        {
            return Err(EngineError::Conflict(
                "outbound startup capture changed".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn publish_running<T>(
        &self,
        session: &EngineSession,
        spec: &InstanceSpec,
        publish: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let native = self
            .seal
            .native
            .as_ref()
            .ok_or_else(|| EngineError::Conflict("native outbound proof is absent".into()))?;
        if !matches!(native.phase, NativePhase::Running) {
            return Err(EngineError::Conflict(
                "running outbound proof is required".into(),
            ));
        }
        native
            .effective
            .with_running(session.state(), &self.generation, || {
                self.validate_snapshot(session, spec)?;
                publish()
            })
    }
    pub fn internet(&self) -> Result<bool> {
        self.seal
            .native
            .as_ref()
            .map(|native| native.effective.internet())
            .ok_or_else(|| EngineError::Conflict("native outbound proof is absent".into()))
    }
    pub fn lan(&self) -> Result<bool> {
        self.seal
            .native
            .as_ref()
            .map(|native| native.effective.lan())
            .ok_or_else(|| EngineError::Conflict("native outbound proof is absent".into()))
    }
    pub(crate) fn bind_stop(&mut self, stop: LanRevocation) -> Result<()> {
        let seal = Arc::get_mut(&mut self.seal).ok_or_else(|| {
            EngineError::Conflict("outbound scope was shared before fencing".into())
        })?;
        let native = seal
            .native
            .as_mut()
            .ok_or_else(|| EngineError::Conflict("native outbound proof is absent".into()))?;
        native.stop = stop;
        Ok(())
    }
    pub(crate) fn stopped(&self) -> bool {
        self.seal
            .native
            .as_ref()
            .is_some_and(|native| native.stop.is_revoked())
    }
    async fn stop_signal(&self) {
        if let Some(native) = &self.seal.native {
            native.stop.revoked().await;
        } else {
            std::future::pending::<()>().await;
        }
    }
    pub(crate) fn capture_running(
        &self,
        session: &EngineSession,
        spec: &InstanceSpec,
        grant: u64,
        network: u64,
    ) -> Result<Self> {
        let initial = self
            .seal
            .native
            .as_ref()
            .ok_or_else(|| EngineError::Conflict("native outbound proof is absent".into()))?;
        initial
            .effective
            .validate_running(session.state(), &self.generation)?;
        let current = session.state().instance(&self.instance_id)?;
        if initial.stop.is_revoked()
            || current.state != InstanceState::Running
            || session.root() != initial.root
            || session.state().browser_identity(&self.instance_id)? != initial.identity
            || serde_json::to_string(spec)? != initial.specification
            || serde_json::to_string(&current.spec)? != initial.specification
        {
            return Err(EngineError::Conflict(
                "running outbound authority changed".into(),
            ));
        }
        let mut scope = Self::new(
            self.instance_id.clone(),
            self.generation.clone(),
            current.version,
            grant,
            network,
        )?;
        scope.seal = Arc::new(ScopeSeal {
            native: Some(NativeCapture {
                root: initial.root.clone(),
                specification: initial.specification.clone(),
                authority: serde_json::to_string(
                    &session.state().run_authority(&self.instance_id)?,
                )?,
                owner: session.ownership(),
                identity: initial.identity.clone(),
                phase: NativePhase::Running,
                effective: initial.effective.clone(),
                stop: initial.stop.clone(),
            }),
        });
        Ok(scope)
    }
}

/// One-use private startup handoff. No serialization or public constructor.
pub struct PreparedEgress {
    scope: EgressScope,
    selected: Arc<dyn SelectedNetwork>,
    policy: Arc<dyn DestinationPolicy>,
    limits: EgressLimits,
    stop: LanRevocation,
    cancel: LanRevocation,
}
impl PreparedEgress {
    pub(crate) fn new(
        selected: Arc<dyn SelectedNetwork>,
        policy: Arc<dyn DestinationPolicy>,
        limits: EgressLimits,
        gates: (LanRevocation, LanRevocation),
    ) -> Result<Self> {
        let scope = selected.scope().clone();
        scope.native_owner()?;
        let value = Self {
            scope,
            selected,
            policy,
            limits,
            stop: gates.0,
            cancel: gates.1,
        };
        value.current()?;
        Ok(value)
    }
    pub(crate) fn scope(&self) -> &EgressScope {
        &self.scope
    }
    pub(crate) fn validate(&self, session: &EngineSession, spec: &InstanceSpec) -> Result<()> {
        self.scope.validate_native(session, spec)?;
        self.current()
    }
    fn current(&self) -> Result<()> {
        if self.stop.is_revoked()
            || self.cancel.is_revoked()
            || !self.selected.current()
            || self.selected.scope() != &self.scope
        {
            return Err(EngineError::Conflict(
                "outbound startup capability was revoked".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn install(
        &self,
        session: &EngineSession,
        spec: &InstanceSpec,
        network: &ServiceNetwork,
        executor: Handle,
    ) -> Result<Arc<ServiceEgress>> {
        self.validate(session, spec)?;
        let manager = ServiceEgress::new_with_revocation(
            self.selected.clone(),
            self.policy.clone(),
            session.ownership(),
            executor,
            self.limits,
            self.cancel.clone(),
        )?;
        network.install_egress(
            (
                self.scope.instance_id(),
                self.scope.generation(),
                self.scope.spec_revision(),
            ),
            manager.clone(),
        )?;
        Ok(manager)
    }
}

pub type BindFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
pub type ResolveFuture<'a> = Pin<Box<dyn Future<Output = Result<EgressResolution>> + Send + 'a>>;
pub type InvalidationFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
#[derive(Debug)]
pub struct EgressResolution {
    pub scope: EgressScope,
    pub addresses: Vec<IpAddr>,
}

/// Mandatory private permission capability. No default allow implementation.
pub trait DestinationPolicy: std::fmt::Debug + Send + Sync + 'static {
    fn authorize_name(&self, scope: &EgressScope, host: &str) -> Result<()>;
    fn authorize_ip(&self, scope: &EgressScope, address: IpAddr) -> Result<()>;
    fn authorize_tcp(&self, scope: &EgressScope, peer: SocketAddr) -> Result<()>;
}
/// Mandatory actual platform capture. Bind borrows an UNCONNECTED socket;
/// resolution uses this same Network. A returned future must retain ownership
/// of all provider work until that work really ends, not detach on cancellation.
pub trait SelectedNetwork: std::fmt::Debug + Send + Sync + 'static {
    fn scope(&self) -> &EgressScope;
    fn current(&self) -> bool;
    /// Irreversible capture-loss signal; this observes state, performs no IO
    /// and must recheck validity before waiting so notifications cannot be lost.
    fn invalidated(&self) -> InvalidationFuture<'_>;
    fn bind<'a>(&'a self, scope: &'a EgressScope, socket: &'a TcpSocket) -> BindFuture<'a>;
    fn resolve<'a>(
        &'a self,
        scope: &'a EgressScope,
        host: &'a str,
        port: Option<u16>,
    ) -> ResolveFuture<'a>;
}

/// Explicit technical admission only. No selected policy or product default.
#[derive(Clone, Copy, Debug)]
pub struct EgressLimits {
    clients: usize,
    resolvers: usize,
    answers: usize,
    connect: Duration,
    resolve: Duration,
}
impl EgressLimits {
    pub fn new(
        clients: usize,
        resolvers: usize,
        answers: usize,
        connect: Duration,
        resolve: Duration,
    ) -> Result<Self> {
        if !(1..=32).contains(&clients)
            || !(1..=8).contains(&resolvers)
            || !(1..=64).contains(&answers)
            || connect.is_zero()
            || resolve.is_zero()
            || connect > Duration::from_secs(30)
            || resolve > Duration::from_secs(30)
        {
            return Err(invalid());
        }
        Ok(Self {
            clients,
            resolvers,
            answers,
            connect,
            resolve,
        })
    }
}
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct EgressStatus {
    pub revoked: bool,
    pub closed: bool,
    pub uncertain: bool,
    pub tcp_active: usize,
    pub dns_active: usize,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
}
struct Receipt {
    state: Mutex<EgressStatus>,
    changed: Notify,
    owner: Mutex<Option<tokio::task::JoinHandle<()>>>,
}
/// A read-only completion observation, not a permission/ownership capability.
#[derive(Clone)]
pub struct EgressReceipt(Arc<Receipt>);
impl EgressReceipt {
    pub fn status(&self) -> Result<EgressStatus> {
        let mut status = self.0.state.lock().map_err(|_| lock_error())?;
        if !status.closed
            && !status.uncertain
            && self
                .0
                .owner
                .lock()
                .map_err(|_| lock_error())?
                .as_ref()
                .is_some_and(|owner| owner.is_finished())
        {
            status.uncertain = true;
            self.0.changed.notify_waiters();
        }
        Ok(*status)
    }
    pub async fn wait(&self) -> Result<EgressStatus> {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let status = self.status()?;
            if status.uncertain {
                return Err(EngineError::Conflict(
                    "outbound cleanup is uncertain".into(),
                ));
            }
            if status.closed {
                return Ok(status);
            }
            changed.await;
        }
    }
}
enum WorkKind {
    Tcp {
        pair: Arc<Connection>,
        opening: Arc<Opening>,
    },
    Resolve {
        answer: Mutex<Option<oneshot::Sender<virtual_net::Result<Vec<IpAddr>>>>>,
    },
}
struct Work {
    id: u64,
    kind: WorkKind,
    deadline: Instant,
}
impl Work {
    fn client_present(&self) -> bool {
        match &self.kind {
            WorkKind::Tcp { pair, .. } => pair.client_present(),
            WorkKind::Resolve { .. } => true,
        }
    }
    async fn client_closed(&self) {
        match &self.kind {
            WorkKind::Tcp { pair, .. } => pair.client_closed().await,
            WorkKind::Resolve { .. } => std::future::pending::<()>().await,
        }
    }
    fn tcp(&self) -> bool {
        matches!(self.kind, WorkKind::Tcp { .. })
    }
    fn cancel(&self, error: NetworkError) {
        match &self.kind {
            WorkKind::Tcp { pair, opening } => {
                opening.finish(pair, Some(error));
                pair.close();
            }
            WorkKind::Resolve { answer } => {
                if let Some(answer) = answer.lock().unwrap_or_else(|p| p.into_inner()).take() {
                    let _ = answer.send(Err(error));
                }
            }
        }
    }
    fn answer(&self, value: virtual_net::Result<Vec<IpAddr>>) {
        if let WorkKind::Resolve { answer } = &self.kind {
            if let Some(answer) = answer.lock().unwrap_or_else(|p| p.into_inner()).take() {
                let _ = answer.send(value);
            }
        }
    }
}
struct State {
    next: u64,
    work: BTreeMap<u64, Weak<Work>>,
}
struct Inner {
    scope: EgressScope,
    policy: Arc<dyn DestinationPolicy>,
    network: Arc<dyn SelectedNetwork>,
    limits: EgressLimits,
    revoked: LanRevocation,
    state: Mutex<State>,
    receipt: Arc<Receipt>,
    owner: Mutex<Option<EngineOwnership>>,
}
impl Inner {
    fn check(&self) -> virtual_net::Result<()> {
        if self.revoked.is_revoked() || self.scope.stopped() {
            return Err(NetworkError::ConnectionAborted);
        }
        if self.network.scope() != &self.scope || !self.network.current() {
            return Err(NetworkError::PermissionDenied);
        }
        Ok(())
    }
    fn revoke(&self) {
        let work = {
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            self.revoked.revoke();
            state
                .work
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        self.receipt
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .revoked = true;
        for work in work {
            work.cancel(NetworkError::ConnectionAborted);
        }
    }
    fn reserve(self: &Arc<Self>, kind: WorkKind) -> virtual_net::Result<WorkOwner> {
        let mut state = self.state.lock().map_err(|_| NetworkError::Lock)?;
        if self.revoked.is_revoked() {
            return Err(NetworkError::ConnectionAborted);
        }
        let tcp = matches!(kind, WorkKind::Tcp { .. });
        let count = state
            .work
            .values()
            .filter_map(Weak::upgrade)
            .filter(|work| work.tcp() == tcp)
            .count();
        if count
            >= if tcp {
                self.limits.clients
            } else {
                self.limits.resolvers
            }
        {
            return Err(NetworkError::TooManyOpenFiles);
        }
        state.next = state
            .next
            .checked_add(1)
            .ok_or(NetworkError::TooManyOpenFiles)?;
        let deadline = Instant::now()
            + if tcp {
                self.limits.connect
            } else {
                self.limits.resolve
            };
        let work = Arc::new(Work {
            id: state.next,
            kind,
            deadline,
        });
        state.work.insert(work.id, Arc::downgrade(&work));
        let mut status = self.receipt.state.lock().map_err(|_| NetworkError::Lock)?;
        if tcp {
            status.tcp_active += 1
        } else {
            status.dns_active += 1
        }
        Ok(WorkOwner {
            inner: self.clone(),
            work,
            outcome: None,
        })
    }
    fn opened(&self, work: &Work) -> virtual_net::Result<()> {
        let _state = self.state.lock().map_err(|_| NetworkError::Lock)?;
        if self.revoked.is_revoked() || !work.client_present() {
            return Err(NetworkError::ConnectionAborted);
        }
        let WorkKind::Tcp { pair, opening } = &work.kind else {
            return Err(NetworkError::InvalidInput);
        };
        opening.finish(pair, None);
        Ok(())
    }
    fn answer(&self, work: &Work, result: virtual_net::Result<Vec<IpAddr>>) {
        let _state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        work.answer(if self.revoked.is_revoked() {
            Err(NetworkError::ConnectionAborted)
        } else {
            result
        });
    }
}
#[derive(Clone, Copy)]
enum Outcome {
    Completed,
    Failed,
    Cancelled,
}
struct WorkOwner {
    inner: Arc<Inner>,
    work: Arc<Work>,
    outcome: Option<Outcome>,
}
impl WorkOwner {
    fn finish(mut self, outcome: Outcome) {
        self.outcome = Some(outcome);
    }
}
impl Drop for WorkOwner {
    fn drop(&mut self) {
        if !matches!(self.outcome, Some(Outcome::Completed)) {
            self.work.cancel(NetworkError::ConnectionAborted)
        }
        let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        state.work.remove(&self.work.id);
        let mut status = self
            .inner
            .receipt
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if self.work.tcp() {
            status.tcp_active -= 1
        } else {
            status.dns_active -= 1
        }
        match self.outcome {
            Some(Outcome::Completed) => status.completed += 1,
            Some(Outcome::Cancelled) => status.cancelled += 1,
            _ => status.failed += 1,
        }
    }
}
enum Command {
    Tcp {
        owner: WorkOwner,
        bridge: Socket,
        peer: SocketAddr,
    },
    Resolve {
        owner: WorkOwner,
        host: String,
        port: Option<u16>,
    },
}
impl Command {
    fn cancel(self) {
        match self {
            Self::Tcp { owner, .. } | Self::Resolve { owner, .. } => {
                owner.finish(Outcome::Cancelled)
            }
        }
    }
}

/// Not installed by any public/product API. The factory requires actual private
/// capabilities and EngineOwnership; matching request metadata is insufficient.
pub struct ServiceEgress {
    inner: Arc<Inner>,
    commands: mpsc::Sender<Command>,
}
impl std::fmt::Debug for ServiceEgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceEgress(<private generation>)")
    }
}
impl ServiceEgress {
    pub fn new(
        network: Arc<dyn SelectedNetwork>,
        policy: Arc<dyn DestinationPolicy>,
        owner: EngineOwnership,
        executor: Handle,
        limits: EgressLimits,
    ) -> Result<Arc<Self>> {
        Self::new_with_revocation(
            network,
            policy,
            owner,
            executor,
            limits,
            LanRevocation::new(),
        )
    }
    pub(crate) fn new_with_revocation(
        network: Arc<dyn SelectedNetwork>,
        policy: Arc<dyn DestinationPolicy>,
        owner: EngineOwnership,
        executor: Handle,
        limits: EgressLimits,
        revoked: LanRevocation,
    ) -> Result<Arc<Self>> {
        if !network.current() {
            return Err(EngineError::Conflict(
                "outbound network capture is not current".into(),
            ));
        }
        let receipt = Arc::new(Receipt {
            state: Mutex::new(EgressStatus::default()),
            changed: Notify::new(),
            owner: Mutex::new(None),
        });
        let inner = Arc::new(Inner {
            scope: network.scope().clone(),
            policy,
            network,
            limits,
            revoked,
            state: Mutex::new(State {
                next: 0,
                work: BTreeMap::new(),
            }),
            receipt,
            owner: Mutex::new(Some(owner)),
        });
        let (commands, receive) = mpsc::channel(limits.clients + limits.resolvers);
        // The private runner retains all commands, children and ownership. An
        // observer never receives its JoinHandle and cannot abort that owner.
        let owner = executor.spawn(run_owner(inner.clone(), receive));
        *inner
            .receipt
            .owner
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(owner);
        Ok(Arc::new(Self { inner, commands }))
    }
    pub fn scope(&self) -> &EgressScope {
        &self.inner.scope
    }
    pub(super) fn check_current(&self) -> Result<()> {
        self.inner
            .check()
            .map_err(|_| EngineError::Conflict("outbound capture is no longer current".into()))
    }
    pub(crate) fn admission_open(&self) -> bool {
        !self.inner.revoked.is_revoked()
            && !self.inner.scope.stopped()
            && self
                .receipt()
                .status()
                .is_ok_and(|report| !report.closed && !report.uncertain && !report.revoked)
    }
    pub fn receipt(&self) -> EgressReceipt {
        EgressReceipt(self.inner.receipt.clone())
    }
    pub fn revoke(&self) {
        self.inner.revoke()
    }
    pub fn drained(&self) -> Result<bool> {
        let status = self.receipt().status()?;
        if status.uncertain {
            return Err(EngineError::Conflict(
                "outbound cleanup is uncertain".into(),
            ));
        }
        Ok(status.closed)
    }
    pub async fn shutdown(&self) -> Result<EgressStatus> {
        self.revoke();
        self.receipt().wait().await
    }
    pub(super) fn open(
        &self,
        namespace: &ServiceNetwork,
        binding: Arc<Binding>,
        peer: SocketAddr,
        ttl: u32,
    ) -> virtual_net::Result<Socket> {
        self.inner.check()?;
        if peer.port() == 0
            || binding.address.is_ipv4() != peer.is_ipv4()
            || matches!(peer,SocketAddr::V6(peer) if peer.scope_id()!=0||peer.flowinfo()!=0)
        {
            return Err(NetworkError::InvalidInput);
        }
        self.inner
            .policy
            .authorize_tcp(self.scope(), peer)
            .map_err(|_| NetworkError::PermissionDenied)?;
        self.inner.check()?;
        let source = binding.address;
        let opening = Arc::new(Opening::new());
        let (client, bridge) =
            namespace.remote_pair(binding, source, peer, ttl, opening.clone())?;
        let owner = self.inner.reserve(WorkKind::Tcp {
            pair: client.pair.clone(),
            opening,
        })?;
        if let Err(error) = self.commands.try_send(Command::Tcp {
            owner,
            bridge,
            peer,
        }) {
            drop(error.into_inner());
            return Err(NetworkError::TooManyOpenFiles);
        }
        Ok(client)
    }
    pub(super) async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
    ) -> virtual_net::Result<Vec<IpAddr>> {
        self.inner.check()?;
        if !canonical_dns_name(host) || port == Some(0) {
            return Err(NetworkError::InvalidInput);
        }
        self.inner
            .policy
            .authorize_name(self.scope(), host)
            .map_err(|_| NetworkError::PermissionDenied)?;
        let (answer, receive) = oneshot::channel();
        let owner = self.inner.reserve(WorkKind::Resolve {
            answer: Mutex::new(Some(answer)),
        })?;
        if let Err(error) = self.commands.try_send(Command::Resolve {
            owner,
            host: host.into(),
            port,
        }) {
            drop(error.into_inner());
            return Err(NetworkError::TooManyOpenFiles);
        }
        let result = receive
            .await
            .unwrap_or(Err(NetworkError::ConnectionAborted));
        // The answer may have been queued just before revoke/replacement and
        // observed only later; publication is not a permanent network grant.
        self.inner.check()?;
        result
    }
    pub(super) fn resolve_ip(&self, address: IpAddr) -> virtual_net::Result<Vec<IpAddr>> {
        self.inner.check()?;
        self.inner
            .policy
            .authorize_ip(self.scope(), address)
            .map_err(|_| NetworkError::PermissionDenied)?;
        self.inner.check()?;
        Ok(vec![address])
    }
}
impl Drop for ServiceEgress {
    fn drop(&mut self) {
        self.revoke()
    }
}

struct RunnerGuard {
    inner: Arc<Inner>,
    complete: bool,
}
impl Drop for RunnerGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.inner.revoke();
            self.inner
                .receipt
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .uncertain = true;
            self.inner.receipt.changed.notify_waiters();
        }
    }
}
async fn run_owner(inner: Arc<Inner>, mut receive: mpsc::Receiver<Command>) {
    let mut guard = RunnerGuard {
        inner: inner.clone(),
        complete: false,
    };
    let mut tasks = JoinSet::new();
    let mut draining = false;
    let mut uncertain = false;
    loop {
        if draining && receive.is_empty() && tasks.is_empty() {
            break;
        }
        tokio::select! {
            _=inner.revoked.revoked(),if !draining=>{draining=true;receive.close();}
            _=inner.scope.stop_signal(),if !draining=>{inner.revoke();draining=true;receive.close();}
            _=inner.network.invalidated(),if !draining=>{inner.revoke();draining=true;receive.close();}
            command=receive.recv(),if !receive.is_closed()||!receive.is_empty()=>{
                match command {
                    Some(command) if !inner.revoked.is_revoked()=>{let inner=inner.clone();tasks.spawn(async move{run_command(&inner,command).await});}
                    Some(command)=>command.cancel(),
                    None=>{inner.revoke();draining=true;receive.close();}
                }
            }
            result=tasks.join_next(),if !tasks.is_empty()=>{if result.is_some_and(|result|result.is_err()){uncertain=true;inner.revoke();draining=true;receive.close();}}
        }
    }
    // All child futures/streams and queued commands have really terminated.
    // Release the private common-owner lease BEFORE publishing terminal ACK.
    if !uncertain {
        drop(inner.owner.lock().unwrap_or_else(|p| p.into_inner()).take());
    }
    {
        let mut status = inner
            .receipt
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        status.revoked = true;
        status.closed = !uncertain;
        status.uncertain = uncertain;
    }
    guard.complete = true;
    inner.receipt.changed.notify_waiters();
}
async fn run_command(inner: &Arc<Inner>, command: Command) {
    match command {
        Command::Tcp {
            owner,
            bridge,
            peer,
        } => {
            let result = tcp(inner, &owner.work, bridge, peer).await;
            let completion = outcome(inner, &owner.work, &result);
            if let Err(error) = &result {
                owner.work.cancel(*error);
            }
            owner.finish(completion);
        }
        Command::Resolve { owner, host, port } => {
            let result = resolve(inner, &owner.work, &host, port).await;
            let outcome = outcome(inner, &owner.work, &result);
            inner.answer(&owner.work, result);
            owner.finish(outcome);
        }
    }
}
fn outcome<T>(inner: &Inner, work: &Work, result: &virtual_net::Result<T>) -> Outcome {
    if inner.revoked.is_revoked() || !work.client_present() {
        Outcome::Cancelled
    } else if result.is_ok() {
        Outcome::Completed
    } else {
        Outcome::Failed
    }
}
/// A deadline/revoke ends the observation immediately, but keeps provider work
/// retained until the actual future returns. Late results are never exposed.
async fn platform<T>(
    inner: &Inner,
    work: &Work,
    deadline: Instant,
    mut future: Pin<Box<dyn Future<Output = Result<T>> + Send + '_>>,
) -> virtual_net::Result<T> {
    tokio::select! {
        biased;
        _=inner.revoked.revoked()=>{work.cancel(NetworkError::ConnectionAborted);let _=future.await;Err(NetworkError::ConnectionAborted)}
        _=work.client_closed()=>{work.cancel(NetworkError::ConnectionAborted);let _=future.await;Err(NetworkError::ConnectionAborted)}
        _=tokio::time::sleep_until(deadline)=>{work.cancel(NetworkError::TimedOut);let _=future.await;Err(NetworkError::TimedOut)}
        result=future.as_mut()=>result.map_err(|_|NetworkError::PermissionDenied),
    }
}
fn current(inner: &Inner, work: &Work, deadline: Instant) -> virtual_net::Result<()> {
    inner.check()?;
    if !work.client_present() {
        return Err(NetworkError::ConnectionAborted);
    }
    if Instant::now() >= deadline {
        return Err(NetworkError::TimedOut);
    }
    Ok(())
}
async fn tcp(
    inner: &Inner,
    work: &Work,
    bridge: Socket,
    peer: SocketAddr,
) -> virtual_net::Result<()> {
    let deadline = work.deadline;
    current(inner, work, deadline)?;
    let socket = if peer.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(|_| NetworkError::IOError)?;
    platform(
        inner,
        work,
        deadline,
        inner.network.bind(&inner.scope, &socket),
    )
    .await?;
    current(inner, work, deadline)?;
    inner
        .policy
        .authorize_tcp(&inner.scope, peer)
        .map_err(|_| NetworkError::PermissionDenied)?;
    let mut stream = tokio::select! {
        biased;
        _=inner.revoked.revoked()=>return Err(NetworkError::ConnectionAborted),
        _=work.client_closed()=>return Err(NetworkError::ConnectionAborted),
        _=tokio::time::sleep_until(deadline)=>return Err(NetworkError::TimedOut),
        result=socket.connect(peer)=>result.map_err(|_|NetworkError::ConnectionRefused)?,
    };
    current(inner, work, deadline)?;
    if stream.peer_addr().map_err(|_| NetworkError::NotConnected)? != peer {
        return Err(NetworkError::PermissionDenied);
    }
    inner
        .policy
        .authorize_tcp(&inner.scope, peer)
        .map_err(|_| NetworkError::PermissionDenied)?;
    current(inner, work, deadline)?;
    inner.opened(work)?;
    let mut bridge: Box<dyn VirtualTcpSocket + Sync> = Box::new(bridge);
    tokio::select! {
        biased;
        _=inner.revoked.revoked()=>Err(NetworkError::ConnectionAborted),
        _=work.client_closed()=>Err(NetworkError::ConnectionAborted),
        result=tokio::io::copy_bidirectional(&mut stream,&mut bridge)=>result.map(|_|()).map_err(|_|NetworkError::ConnectionAborted),
    }
}
async fn resolve(
    inner: &Inner,
    work: &Work,
    host: &str,
    port: Option<u16>,
) -> virtual_net::Result<Vec<IpAddr>> {
    let deadline = work.deadline;
    current(inner, work, deadline)?;
    let result = platform(
        inner,
        work,
        deadline,
        inner.network.resolve(&inner.scope, host, port),
    )
    .await?;
    current(inner, work, deadline)?;
    inner
        .policy
        .authorize_name(&inner.scope, host)
        .map_err(|_| NetworkError::PermissionDenied)?;
    if result.scope != inner.scope
        || result.addresses.is_empty()
        || result.addresses.len() > inner.limits.answers
    {
        return Err(NetworkError::PermissionDenied);
    }
    for address in &result.addresses {
        inner
            .policy
            .authorize_ip(&inner.scope, *address)
            .map_err(|_| NetworkError::PermissionDenied)?;
    }
    current(inner, work, deadline)?;
    Ok(result.addresses)
}
/// Explicit ASCII DNS spelling only: never let resolver IDNA/separator or
/// malformed-label normalization turn a retired private name into public DNS.
/// International names must arrive as explicit ASCII A-labels, not an ambient
/// platform transformation. One absolute-name trailing dot is supported.
fn canonical_dns_name(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || !host.is_ascii() {
        return false;
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    })
}
fn invalid() -> EngineError {
    EngineError::Invalid("invalid private outbound scope or bounds".into())
}
fn lock_error() -> EngineError {
    EngineError::Conflict("outbound ownership state unavailable".into())
}

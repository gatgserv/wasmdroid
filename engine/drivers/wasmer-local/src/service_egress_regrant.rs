//! One pending native replacement, retained cleanup and read-only observations.
use crate::{
    service_egress::{
        DestinationPolicy, EgressLimits, EgressReceipt, EgressScope, EgressStatus, SelectedNetwork,
        ServiceEgress,
    },
    service_network::ServiceNetwork,
};
use std::sync::{Arc, Mutex, Weak};
use tokio::{runtime::Handle, sync::Notify};
use wasmdroid_engine::{
    EngineError, Result, api::InstanceSpec, network::lan::LanRevocation, session::EngineConnection,
};

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressPhase {
    Absent,
    Preparing,
    Active,
    Draining,
    Closed,
    Uncertain,
    Stopped,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct EgressCaptureStatus {
    pub grant_epoch: u64,
    pub network_epoch: u64,
    pub native_revision: u64,
    pub report: EgressStatus,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct EgressRegrantStatus {
    pub phase: EgressPhase,
    pub grant_epoch: u64,
    pub network_epoch: u64,
    pub native_revision: u64,
    pub admitted: bool,
    pub terminal: bool,
    pub closed: bool,
    pub uncertain: bool,
    pub report: Option<EgressStatus>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct EgressAttachmentStatus {
    pub phase: EgressPhase,
    pub current: Option<EgressCaptureStatus>,
    pub pending: Option<EgressRegrantStatus>,
    pub closed: bool,
    pub uncertain: bool,
    pub stopped: bool,
    pub ready: bool,
}
struct AdmissionState {
    last: u64,
    pending: Option<Arc<Control>>,
    completed: bool,
}
pub(crate) struct RegrantAdmission {
    state: Mutex<AdmissionState>,
}
impl RegrantAdmission {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(AdmissionState {
                last: 0,
                pending: None,
                completed: false,
            }),
        })
    }
    pub(crate) fn startup(&self, grant: u64) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| failure())?;
        if state.completed || state.last != 0 || grant != 1 {
            return Err(failure());
        }
        state.last = grant;
        Ok(())
    }
    pub(crate) fn pending(&self) -> Result<Option<EgressRegrantStatus>> {
        let pending = self.state.lock().map_err(|_| failure())?.pending.clone();
        pending.map(|control| control.status()).transpose()
    }
    pub(crate) fn cancel_pending(&self) {
        let pending = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
            .clone();
        if let Some(pending) = pending {
            pending.cancel.revoke();
            pending.changed.notify_waiters();
        }
    }
    /// Driver state -> admission is the shared local completion lock order.
    /// The closure only polls actual execution; no foreign callback or await.
    /// Final reserve uses this same gate, so it cannot publish a pending ticket
    /// between the last cleanup check and a terminal native result.
    pub(crate) fn poll_completion<T>(
        &self,
        poll: impl FnOnce() -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        let mut state = self.state.lock().map_err(|_| failure())?;
        if let Some(pending) = &state.pending {
            let status = pending.status()?;
            if status.uncertain {
                return Err(EngineError::Conflict(
                    "outbound replacement cleanup is uncertain".into(),
                ));
            }
            if !status.closed {
                return Ok(None);
            }
        }
        let result = poll()?;
        if result.is_some() {
            state.completed = true;
        }
        Ok(result)
    }
    fn release(&self, control: &Control) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state
            .pending
            .as_ref()
            .is_some_and(|current| std::ptr::eq(current.as_ref(), control))
        {
            state.pending = None;
        }
    }
    pub(crate) fn reserve(
        self: &Arc<Self>,
        context: RegrantContext,
        network_epoch: u64,
    ) -> Result<EgressRegrantTicket> {
        let session = context.connection.connect()?;
        let last = {
            let state = self.state.lock().map_err(|_| failure())?;
            if state.completed
                || state.pending.is_some()
                || context.stop.is_revoked()
                || !context.previous.drained()?
            {
                return Err(failure());
            }
            state.last
        };
        let grant = last
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(failure)?;
        let scope = context.previous.scope().capture_running(
            &session,
            &context.spec,
            grant,
            network_epoch,
        )?;
        let internet = scope.internet()?;
        let lan = scope.lan()?;
        let mut state = self.state.lock().map_err(|_| failure())?;
        if state.completed
            || state.last != last
            || state.pending.is_some()
            || context.stop.is_revoked()
            || !context.previous.drained()?
        {
            return Err(failure());
        }
        let control = Arc::new(Control {
            state: Mutex::new(ControlState {
                scope: EgressCaptureStatus {
                    grant_epoch: grant,
                    network_epoch,
                    native_revision: scope.spec_revision(),
                    report: EgressStatus::default(),
                },
                admitted: false,
                terminal: false,
                closed: false,
                uncertain: false,
                receipt: None,
            }),
            cancel: LanRevocation::new(),
            stop: context.stop.clone(),
            changed: Notify::new(),
            admission: Arc::downgrade(self),
        });
        state.last = grant;
        state.pending = Some(control.clone());
        Ok(EgressRegrantTicket {
            scope,
            internet,
            lan,
            context,
            control,
            consumed: false,
        })
    }
}
pub(crate) struct RegrantContext {
    pub connection: EngineConnection,
    pub spec: InstanceSpec,
    pub previous: Arc<ServiceEgress>,
    pub network: ServiceNetwork,
    pub executor: Handle,
    pub stop: LanRevocation,
}
struct ControlState {
    scope: EgressCaptureStatus,
    admitted: bool,
    terminal: bool,
    closed: bool,
    uncertain: bool,
    receipt: Option<EgressReceipt>,
}
struct Control {
    state: Mutex<ControlState>,
    cancel: LanRevocation,
    stop: LanRevocation,
    changed: Notify,
    admission: Weak<RegrantAdmission>,
}
impl Control {
    fn status(&self) -> Result<EgressRegrantStatus> {
        let state = self.state.lock().map_err(|_| failure())?;
        let report = state
            .receipt
            .as_ref()
            .map(EgressReceipt::status)
            .transpose()?;
        let uncertain = state.uncertain || report.is_some_and(|report| report.uncertain);
        let closed = state.terminal
            && !uncertain
            && (state.closed || report.is_some_and(|report| report.closed));
        let phase = if self.stop.is_revoked() {
            EgressPhase::Stopped
        } else if uncertain {
            EgressPhase::Uncertain
        } else if !state.terminal {
            EgressPhase::Preparing
        } else if closed {
            EgressPhase::Closed
        } else if self.cancel.is_revoked() || report.is_some_and(|report| report.revoked) {
            EgressPhase::Draining
        } else {
            EgressPhase::Active
        };
        Ok(EgressRegrantStatus {
            phase,
            grant_epoch: state.scope.grant_epoch,
            network_epoch: state.scope.network_epoch,
            native_revision: state.scope.native_revision,
            admitted: state.admitted,
            terminal: state.terminal,
            closed,
            uncertain,
            report,
        })
    }
    fn finish(&self, admitted: bool, closed: bool, uncertain: bool) {
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.admitted = admitted;
            state.terminal = true;
            state.closed = closed;
            state.uncertain = uncertain;
        }
        // Uncertain cleanup is sticky for this driver; it cannot admit another
        // candidate merely because an observer/owner task ended unexpectedly.
        if !uncertain && let Some(admission) = self.admission.upgrade() {
            admission.release(self);
        }
        self.changed.notify_waiters();
    }
}
#[derive(Clone)]
pub struct EgressRegrantControl {
    inner: Arc<Control>,
}
impl EgressRegrantControl {
    pub fn cancel(&self) {
        self.inner.cancel.revoke();
        self.inner.changed.notify_waiters();
    }
    pub fn status(&self) -> Result<EgressRegrantStatus> {
        self.inner.status()
    }
    pub async fn wait(&self) -> Result<EgressRegrantStatus> {
        // Admission terminal includes success: admitted=true, terminal=true,
        // phase=Active, closed=false. Use wait_closed for actual resource drain.
        loop {
            let changed = self.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let status = self.status()?;
            if status.terminal || status.uncertain {
                return Ok(status);
            }
            changed.await;
        }
    }
    /// Waits for actual closure, unlike wait() which observes admission terminal
    /// (including a successfully Active manager). Neither method cancels work.
    pub async fn wait_closed(&self) -> Result<EgressRegrantStatus> {
        loop {
            let changed = self.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let status = self.status()?;
            if status.closed || status.uncertain {
                return Ok(status);
            }
            let receipt = self
                .inner
                .state
                .lock()
                .map_err(|_| failure())?
                .receipt
                .clone();
            if let Some(receipt) = receipt {
                // Closure can precede the cleanup owner's admission metadata.
                // Await that owner rather than spinning on an already-ready
                // receipt and starving its task on a single-thread runtime.
                if receipt.status()?.closed {
                    changed.await;
                } else {
                    tokio::select! {_ = receipt.wait()=>{},_ = changed.as_mut()=>{}}
                }
            } else {
                changed.await;
            }
        }
    }
}
/// No public constructor/Clone/serde. Drop before dispatch proves no admission.
pub struct EgressRegrantTicket {
    scope: EgressScope,
    internet: bool,
    lan: bool,
    context: RegrantContext,
    control: Arc<Control>,
    consumed: bool,
}
impl EgressRegrantTicket {
    pub(crate) fn belongs_to(&self, admission: &Arc<RegrantAdmission>) -> bool {
        self.control
            .admission
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, admission))
    }
    pub fn scope(&self) -> &EgressScope {
        &self.scope
    }
    pub fn internet(&self) -> bool {
        self.internet
    }
    pub fn lan(&self) -> bool {
        self.lan
    }
    pub fn control(&self) -> EgressRegrantControl {
        EgressRegrantControl {
            inner: self.control.clone(),
        }
    }
    pub(crate) fn install(
        mut self,
        selected: Arc<dyn SelectedNetwork>,
        policy: Arc<dyn DestinationPolicy>,
        limits: EgressLimits,
    ) -> Result<EgressReceipt> {
        if selected.scope() != &self.scope
            || !selected.current()
            || self.control.cancel.is_revoked()
            || self.context.stop.is_revoked()
        {
            return Err(failure());
        }
        let session = self.context.connection.connect()?;
        self.scope.validate_native(&session, &self.context.spec)?;
        let manager = ServiceEgress::new_with_revocation(
            selected,
            policy,
            session.ownership(),
            self.context.executor.clone(),
            limits,
            self.control.cancel.clone(),
        )?;
        self.consumed = true;
        let mut cleanup = CandidateCleanup {
            manager: Some(manager.clone()),
            control: self.control.clone(),
            executor: self.context.executor.clone(),
            published: false,
        };
        let receipt = manager.receipt();
        self.control.state.lock().map_err(|_| failure())?.receipt = Some(receipt.clone());
        // All injected methods finish before the native transaction/local CAS.
        manager.check_current()?;
        self.scope
            .publish_running(&session, &self.context.spec, || {
                if self.control.cancel.is_revoked() || self.context.stop.is_revoked() {
                    return Err(failure());
                }
                self.context
                    .network
                    .replace_egress(&self.context.previous, manager)?;
                // SQLite commit can still fail after the in-memory CAS. Retain the
                // historical admission fact; cleanup must revoke+await this manager.
                self.control
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .admitted = true;
                Ok(())
            })?;
        cleanup.published = true;
        cleanup.manager.take();
        self.control.finish(true, false, false);
        Ok(receipt)
    }
}
impl Drop for EgressRegrantTicket {
    fn drop(&mut self) {
        if !self.consumed {
            self.control.finish(false, true, false);
        }
    }
}
struct CandidateCleanup {
    manager: Option<Arc<ServiceEgress>>,
    control: Arc<Control>,
    executor: Handle,
    published: bool,
}
impl Drop for CandidateCleanup {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        let Some(manager) = self.manager.take() else {
            return;
        };
        manager.revoke();
        let guard = CleanupReceipt {
            control: self.control.clone(),
            done: false,
        };
        self.executor.spawn(async move {
            let mut guard = guard;
            let result = manager.receipt().wait().await;
            let closed = result
                .as_ref()
                .is_ok_and(|report| report.closed && !report.uncertain);
            drop(manager);
            let admitted = guard
                .control
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .admitted;
            guard.control.finish(admitted, closed, !closed);
            guard.done = true;
        });
    }
}
struct CleanupReceipt {
    control: Arc<Control>,
    done: bool,
}
impl Drop for CleanupReceipt {
    fn drop(&mut self) {
        if !self.done {
            let admitted = self
                .control
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .admitted;
            self.control.finish(admitted, false, true);
        }
    }
}
fn failure() -> EngineError {
    EngineError::Conflict("outbound replacement admission is unavailable or changed".into())
}

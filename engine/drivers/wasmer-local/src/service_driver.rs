//! Worker-resident implementation of the neutral driver. The common Android
//! supervisor still owns metadata, deadlines and the observed process-death gate.
use crate::{
    InputControl, TerminalHandle,
    service_execution::{ServiceExecution, ServiceInfo, validate_spec},
    webc_execution::ExecutionResult,
};
use std::sync::{Arc, Mutex, OnceLock};
use wasmdroid_engine::{
    EngineError, Result,
    api::{ArtifactKind, InstanceSpec},
    driver::{DriverCapabilities, RuntimeDriver, RuntimeStats, StopSignal},
    session::EngineSession,
};

/// Supplied only by the private worker host. It must target its own process and
/// keep its control thread responsive even when a native execution call blocks.
pub trait WorkerPlatform: Send + Sync {
    fn terminate_after(&self, grace_seconds: u64);
    fn terminate(&self);
}

struct State {
    session: Option<EngineSession>,
    spec: Option<InstanceSpec>,
    execution: Option<ServiceExecution>,
    started: bool,
    removed: bool,
    network_config: Option<wasmdroid_engine::network::grants::WorkerNetwork>,
    prepared_egress: Option<crate::service_egress::PreparedEgress>,
}

pub struct ServiceDriver {
    id: String,
    generation: String,
    state: Mutex<State>,
    input: OnceLock<Option<InputControl>>,
    terminal: OnceLock<Option<TerminalHandle>>,
    platform: Arc<dyn WorkerPlatform>,
    network: OnceLock<(crate::service_network::ServiceNetwork, Mutex<EngineSession>)>,
    lan: OnceLock<Arc<crate::service_lan::ServiceLan>>,
    startup_stop: wasmdroid_engine::network::lan::LanRevocation,
    pending_egress: OnceLock<wasmdroid_engine::network::lan::LanRevocation>,
    regrants: Arc<crate::service_egress_regrant::RegrantAdmission>,
}

impl ServiceDriver {
    pub fn new(
        session: EngineSession,
        id: String,
        generation: String,
        platform: Arc<dyn WorkerPlatform>,
    ) -> Self {
        Self::with_network(session, id, generation, platform, None)
    }
    pub fn with_network(
        session: EngineSession,
        id: String,
        generation: String,
        platform: Arc<dyn WorkerPlatform>,
        network_config: Option<wasmdroid_engine::network::grants::WorkerNetwork>,
    ) -> Self {
        Self {
            id,
            generation,
            platform,
            input: OnceLock::new(),
            terminal: OnceLock::new(),
            state: Mutex::new(State {
                session: Some(session),
                spec: None,
                execution: None,
                started: false,
                removed: false,
                network_config,
                prepared_egress: None,
            }),
            network: OnceLock::new(),
            lan: OnceLock::new(),
            startup_stop: wasmdroid_engine::network::lan::LanRevocation::new(),
            pending_egress: OnceLock::new(),
            regrants: crate::service_egress_regrant::RegrantAdmission::new(),
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Explicit configured path, not permission to execute without preparation.
    pub fn create_with_egress_spec(&self, id: &str, spec: &InstanceSpec) -> Result<()> {
        self.check_id(id)?;
        crate::service_execution::validate_spec_for_egress_configuration(spec)?;
        let mut state = self.lock()?;
        if state.spec.is_some() || state.started || state.removed || self.startup_stop.is_revoked()
        {
            return Err(EngineError::InvalidTransition(
                "worker is single-use".into(),
            ));
        }
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| EngineError::Conflict("worker session is absent".into()))?;
        if serde_json::to_value(&session.state().instance(id)?.spec)? != serde_json::to_value(spec)?
        {
            return Err(EngineError::Conflict(
                "worker specification differs from its instance".into(),
            ));
        }
        state.spec = Some(spec.clone());
        Ok(())
    }
    pub fn capture_egress_scope(
        &self,
        grant_epoch: u64,
        network_epoch: u64,
    ) -> Result<crate::service_egress::EgressScope> {
        let state = self.lock()?;
        if state.started || state.removed || self.startup_stop.is_revoked() {
            return Err(EngineError::Conflict(
                "worker startup is unavailable".into(),
            ));
        }
        let spec = state
            .spec
            .as_ref()
            .ok_or_else(|| EngineError::Conflict("create must precede capture".into()))?;
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| EngineError::Conflict("worker session is absent".into()))?;
        let mut scope = crate::service_egress::EgressScope::capture(
            session,
            &self.id,
            &self.generation,
            spec,
            grant_epoch,
            network_epoch,
        )?;
        scope.bind_stop(self.startup_stop.clone())?;
        self.regrants.startup(grant_epoch)?;
        Ok(scope)
    }
    pub fn reserve_running_egress(
        &self,
        network_epoch: u64,
    ) -> Result<crate::service_egress_regrant::EgressRegrantTicket> {
        let (executor, connection, spec, network) = {
            let state = self.lock()?;
            if state.removed || self.startup_stop.is_revoked() {
                return Err(EngineError::Conflict("worker is stopped".into()));
            }
            let execution = state
                .execution
                .as_ref()
                .ok_or_else(|| EngineError::Conflict("worker has not completed startup".into()))?;
            let (executor, connection) = execution.egress_runtime_context()?;
            (
                executor,
                connection,
                state.spec.clone().ok_or_else(|| {
                    EngineError::Conflict("worker specification is absent".into())
                })?,
                execution.network_handle(),
            )
        };
        let previous = network.egress_manager()?.ok_or_else(|| {
            EngineError::Unsupported("worker has no authorized outbound capture".into())
        })?;
        self.regrants.reserve(
            crate::service_egress_regrant::RegrantContext {
                connection,
                spec,
                previous,
                network,
                executor,
                stop: self.startup_stop.clone(),
            },
            network_epoch,
        )
    }
    pub fn install_running_egress(
        &self,
        ticket: crate::service_egress_regrant::EgressRegrantTicket,
        selected: Arc<dyn crate::service_egress::SelectedNetwork>,
        policy: Arc<dyn crate::service_egress::DestinationPolicy>,
        limits: crate::service_egress::EgressLimits,
    ) -> Result<crate::service_egress::EgressReceipt> {
        if !ticket.belongs_to(&self.regrants)
            || ticket.scope().instance_id() != self.id
            || ticket.scope().generation() != self.generation
            || self.startup_stop.is_revoked()
        {
            return Err(EngineError::Conflict(
                "outbound ticket belongs to another worker".into(),
            ));
        }
        ticket.install(selected, policy, limits)
    }
    pub fn egress_status(&self) -> Result<crate::service_egress_regrant::EgressAttachmentStatus> {
        use crate::service_egress_regrant::{
            EgressAttachmentStatus, EgressCaptureStatus, EgressPhase,
        };
        let (network, connection, spec, preparing) = {
            let state = self.lock()?;
            if let Some(execution) = &state.execution {
                let (_, connection) = execution.egress_runtime_context()?;
                (
                    Some(execution.network_handle()),
                    Some(connection),
                    state.spec.clone(),
                    false,
                )
            } else {
                (
                    None,
                    None,
                    state.spec.clone(),
                    state.prepared_egress.is_some() || self.pending_egress.get().is_some(),
                )
            }
        };
        let manager = network
            .as_ref()
            .map(|network| network.egress_manager())
            .transpose()?
            .flatten();
        let current = manager
            .as_ref()
            .map(|manager| -> Result<EgressCaptureStatus> {
                let scope = manager.scope();
                Ok(EgressCaptureStatus {
                    grant_epoch: scope.grant_epoch(),
                    network_epoch: scope.network_epoch(),
                    native_revision: scope.spec_revision(),
                    report: manager.receipt().status()?,
                })
            })
            .transpose()?;
        let pending = self.regrants.pending()?;
        let stopped = self.startup_stop.is_revoked();
        let uncertain = current
            .as_ref()
            .is_some_and(|current| current.report.uncertain)
            || pending.as_ref().is_some_and(|pending| pending.uncertain);
        let closed = !preparing
            && !uncertain
            && current.as_ref().is_none_or(|current| current.report.closed)
            && pending.as_ref().is_none_or(|pending| pending.closed);
        let ready = if closed && !stopped && pending.is_none() {
            if let (Some(manager), Some(connection), Some(spec)) =
                (manager.as_ref(), connection.as_ref(), spec.as_ref())
            {
                let session = connection.connect()?;
                manager
                    .scope()
                    .capture_running(
                        &session,
                        spec,
                        manager.scope().grant_epoch(),
                        manager.scope().network_epoch(),
                    )
                    .is_ok()
            } else {
                false
            }
        } else {
            false
        };
        let phase = if stopped {
            EgressPhase::Stopped
        } else if uncertain {
            EgressPhase::Uncertain
        } else if preparing || pending.as_ref().is_some_and(|pending| !pending.terminal) {
            EgressPhase::Preparing
        } else if closed {
            if current.is_some() {
                EgressPhase::Closed
            } else {
                EgressPhase::Absent
            }
        } else if current
            .as_ref()
            .is_some_and(|current| current.report.revoked)
        {
            EgressPhase::Draining
        } else {
            EgressPhase::Active
        };
        Ok(EgressAttachmentStatus {
            phase,
            current,
            pending,
            closed,
            uncertain,
            stopped,
            ready,
        })
    }
    pub fn prepare_egress(
        &self,
        selected: Arc<dyn crate::service_egress::SelectedNetwork>,
        policy: Arc<dyn crate::service_egress::DestinationPolicy>,
        limits: crate::service_egress::EgressLimits,
    ) -> Result<()> {
        // All injected reads happen before acquiring the driver control lock.
        let cancel = wasmdroid_engine::network::lan::LanRevocation::new();
        let prepared = crate::service_egress::PreparedEgress::new(
            selected,
            policy,
            limits,
            (self.startup_stop.clone(), cancel.clone()),
        )?;
        if prepared.scope().instance_id() != self.id
            || prepared.scope().generation() != self.generation
        {
            return Err(EngineError::Conflict(
                "outbound scope belongs to another worker".into(),
            ));
        }
        let mut state = self.lock()?;
        if state.started
            || state.removed
            || state.prepared_egress.is_some()
            || self.pending_egress.get().is_some()
            || self.startup_stop.is_revoked()
        {
            return Err(EngineError::Conflict(
                "outbound preparation is single-use".into(),
            ));
        }
        let spec = state
            .spec
            .as_ref()
            .ok_or_else(|| EngineError::Conflict("create must precede preparation".into()))?;
        prepared.scope().validate_native(
            state
                .session
                .as_ref()
                .ok_or_else(|| EngineError::Conflict("worker session is absent".into()))?,
            spec,
        )?;
        self.pending_egress.set(cancel).map_err(|_| {
            EngineError::Conflict("outbound preparation was already consumed".into())
        })?;
        state.prepared_egress = Some(prepared);
        Ok(())
    }
    /// Future private host integration only. The product preflight continues to
    /// reject Internet permission until platform policy and grants are wired.
    /// All injected capability calls occur outside the driver control mutex.
    pub fn install_egress(
        &self,
        selected: Arc<dyn crate::service_egress::SelectedNetwork>,
        policy: Arc<dyn crate::service_egress::DestinationPolicy>,
        limits: crate::service_egress::EgressLimits,
    ) -> Result<Arc<crate::service_egress::ServiceEgress>> {
        let scope = selected.scope().clone();
        if scope.instance_id() != self.id || scope.generation() != self.generation {
            return Err(EngineError::Conflict(
                "outbound capability belongs to another worker".into(),
            ));
        }
        let (handle, owner, network) = {
            let state = self.lock()?;
            if state.removed {
                return Err(EngineError::Conflict("worker was removed".into()));
            }
            let spec = state
                .spec
                .as_ref()
                .ok_or_else(|| EngineError::Conflict("worker specification is absent".into()))?;
            if (!spec.internet_access && !spec.lan_access)
                || spec.network == wasmdroid_engine::api::NetworkMode::None
            {
                return Err(EngineError::Unsupported(
                    "outbound Internet permission is absent".into(),
                ));
            }
            let execution = state
                .execution
                .as_ref()
                .ok_or_else(|| EngineError::Conflict("worker execution is absent".into()))?;
            let (handle, owner, record) = execution.egress_context(&self.id)?;
            if record.version != scope.spec_revision()
                || serde_json::to_value(&record.spec)? != serde_json::to_value(spec)?
            {
                return Err(EngineError::Conflict(
                    "outbound capability specification is stale".into(),
                ));
            }
            (handle, owner, execution.network_handle())
        };
        let manager =
            crate::service_egress::ServiceEgress::new(selected, policy, owner, handle, limits)?;
        network.install_egress(
            (&self.id, &self.generation, scope.spec_revision()),
            manager.clone(),
        )?;
        Ok(manager)
    }
    fn revoke_egress(&self) {
        self.startup_stop.revoke();
        if let Some(pending) = self.pending_egress.get() {
            pending.revoke();
        }
        if let Some((network, _)) = self.network.get() {
            network.revoke_egress();
        }
    }
    fn lan(&self) -> Result<&Arc<crate::service_lan::ServiceLan>> {
        self.lan
            .get()
            .ok_or_else(|| EngineError::Conflict("worker LAN capability is not ready".into()))
    }
    pub fn enable_lan(
        &self,
        scope: wasmdroid_engine::network::lan::LanScope,
        host_port: u16,
        limits: wasmdroid_engine::network::lan::LanLimits,
        binder: impl FnMut(
            &tokio::net::TcpSocket,
            &wasmdroid_engine::network::lan::LanScope,
        ) -> Result<()>,
    ) -> Result<std::net::SocketAddr> {
        self.lan()?.enable(scope, host_port, limits, binder)
    }
    pub fn revoke_lan(&self, scope: &wasmdroid_engine::network::lan::LanScope) -> Result<()> {
        self.lan()?.revoke(scope)
    }
    pub fn revoke_all_lan(&self) -> Result<()> {
        self.lan()?.revoke_all();
        Ok(())
    }
    pub fn disable_lan(
        &self,
        scope: &wasmdroid_engine::network::lan::LanScope,
    ) -> Result<wasmdroid_engine::network::lan::LanReport> {
        self.lan()?.disable_blocking(scope)
    }
    pub fn lan_status(
        &self,
        scope: &wasmdroid_engine::network::lan::LanScope,
    ) -> Result<crate::service_lan::LanStatus> {
        self.lan()?.status(scope)
    }
    pub fn update_network(
        &self,
        config: wasmdroid_engine::network::grants::WorkerNetwork,
    ) -> Result<()> {
        config.validate(&self.id, &self.generation)?;
        let (network, session) = self
            .network
            .get()
            .ok_or_else(|| EngineError::Conflict("worker network is not ready".into()))?;
        let aliases = config.reserved_aliases(
            session
                .lock()
                .map_err(|_| EngineError::Conflict("worker network lock failed".into()))?
                .state(),
        )?;
        network.update_routes(config, aliases)
    }
    fn check_id(&self, id: &str) -> Result<()> {
        if self.id == id {
            Ok(())
        } else {
            Err(EngineError::NotFound(
                "instance does not own this worker".into(),
            ))
        }
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_| EngineError::Conflict("worker driver lock failed".into()))
    }
    fn execution<T>(&self, call: impl FnOnce(&mut ServiceExecution) -> Result<T>) -> Result<T> {
        let mut state = self.lock()?;
        let execution = state.execution.as_mut().ok_or_else(|| {
            EngineError::InvalidTransition("worker execution is not available".into())
        })?;
        call(execution)
    }
    pub fn info(&self) -> Result<ServiceInfo> {
        self.execution(|execution| Ok(execution.info()))
    }
    pub fn poll(&self) -> Result<Option<ExecutionResult>> {
        self.execution(|execution| {
            self.regrants
                .poll_completion(|| execution.poll().map_err(runtime_error))
        })
    }
    pub fn terminal_frame(&self, query: &str) -> Result<String> {
        self.terminal
            .get()
            .and_then(Option::as_ref)
            .ok_or_else(|| EngineError::Unsupported("no terminal display".into()))?
            .frame(query)
    }
    pub fn close_stdin(&self) -> Result<()> {
        self.input
            .get()
            .and_then(Option::as_ref)
            .ok_or_else(|| EngineError::Unsupported("stdin is closed by specification".into()))?
            .eof()?;
        Ok(())
    }
    pub fn interrupt(&self) -> Result<()> {
        self.regrants.cancel_pending();
        if let Some(pending) = self.pending_egress.get() {
            pending.revoke();
        }
        if let Some((network, _)) = self.network.get() {
            network.revoke_current_egress();
        }
        if let Some(lan) = self.lan.get() {
            lan.revoke_all();
        }
        self.execution(|execution| {
            execution.interrupt();
            Ok(())
        })
    }
    pub fn resize(&self, cols: u32, rows: u32, width: u32, height: u32) -> Result<()> {
        self.execution(|execution| {
            execution.resize(cols, rows, width, height)?;
            Ok(())
        })
    }
}

impl RuntimeDriver for ServiceDriver {
    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            id: "wasmer-local".into(),
            artifact_kinds: vec![ArtifactKind::Webc, ArtifactKind::Wasm],
            pause: false,
            tty: true,
            memory_limit: false,
            cpu_shares: false,
        }
    }
    fn create(&self, id: &str, spec: &InstanceSpec) -> Result<()> {
        self.check_id(id)?;
        validate_spec(spec)?;
        self.create_with_egress_spec(id, spec)
    }
    fn start(&self, id: &str) -> Result<()> {
        self.check_id(id)?;
        let (session, spec, config, prepared) = {
            let mut state = self.lock()?;
            if state.started || state.removed || self.startup_stop.is_revoked() {
                return Err(EngineError::InvalidTransition(
                    "worker is single-use or stopped".into(),
                ));
            }
            let spec = state.spec.clone().ok_or_else(|| {
                EngineError::InvalidTransition("create must precede start".into())
            })?;
            state.started = true;
            let effective = state
                .session
                .as_ref()
                .ok_or_else(|| EngineError::Conflict("worker session is absent".into()))?
                .state()
                .effective_worker_spec(id)?;
            if (effective.internet() || effective.lan()) && state.prepared_egress.is_none() {
                return Err(EngineError::Unsupported(
                    "outbound startup capability is absent".into(),
                ));
            }
            let session = state
                .session
                .take()
                .ok_or_else(|| EngineError::InvalidTransition("worker session is absent".into()))?;
            (
                session,
                spec,
                state.network_config.take(),
                state.prepared_egress.take(),
            )
        };
        // Compilation and injected capability work never hold the control mutex.
        let network_session = session.connection()?;
        let mut execution = ServiceExecution::start_with_network_and_egress(
            session,
            id,
            &self.generation,
            &spec,
            config,
            (prepared, self.startup_stop.clone()),
        )
        .map_err(runtime_error)?;
        let _ = self
            .network
            .set((execution.network_handle(), Mutex::new(network_session)));
        let _ = self.lan.set(execution.lan_handle());
        let _ = self.input.set(execution.input());
        let _ = self.terminal.set(execution.terminal_handle());
        let cancelled = self.startup_stop.is_revoked();
        if cancelled {
            execution.request_kill();
        }
        self.lock()?.execution = Some(execution);
        if cancelled {
            Err(EngineError::Conflict("worker startup was cancelled".into()))
        } else {
            Ok(())
        }
    }
    fn stop(&self, id: &str, signal: StopSignal, timeout_seconds: u64) -> Result<()> {
        self.check_id(id)?;
        if timeout_seconds > 60 {
            return Err(EngineError::Invalid("stop grace exceeds 60 seconds".into()));
        }
        // Schedule BEFORE taking a native mutex: a wedged guest must not prevent
        // the platform from enforcing the deadline on its responsive Looper.
        self.platform.terminate_after(timeout_seconds);
        self.revoke_egress();
        if let Some(lan) = self.lan.get() {
            lan.close_generation();
        }
        self.execution(|execution| {
            match signal {
                StopSignal::Terminate => execution.request_stop(),
                StopSignal::Interrupt => execution.interrupt(),
            }
            Ok(())
        })
    }
    fn kill(&self, id: &str) -> Result<()> {
        self.check_id(id)?;
        self.revoke_egress();
        if let Some(lan) = self.lan.get() {
            lan.close_generation();
        }
        self.platform.terminate();
        Ok(())
    }
    fn wait(&self, id: &str) -> Result<i32> {
        self.check_id(id)?;
        loop {
            if let Some(result) = self.poll()? {
                return Ok(result.exit_code);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    fn attach_stdio(&self, id: &str, input: &[u8]) -> Result<()> {
        self.check_id(id)?;
        // Do not hold the execution mutex while the bounded queue applies
        // backpressure: poll, stop and process termination must stay independent.
        self.input
            .get()
            .and_then(Option::as_ref)
            .ok_or_else(|| EngineError::Unsupported("stdin is closed by specification".into()))?
            .write(input.to_vec())?;
        Ok(())
    }
    fn stats(&self, id: &str) -> Result<RuntimeStats> {
        self.check_id(id)?;
        Ok(wasmdroid_engine::process_stats::sample_worker())
    }
    fn remove(&self, id: &str) -> Result<()> {
        self.check_id(id)?;
        let mut state = self.lock()?;
        if state
            .execution
            .as_mut()
            .map(|execution| {
                self.regrants
                    .poll_completion(|| execution.poll().map_err(runtime_error))
            })
            .transpose()?
            .flatten()
            .is_none()
        {
            return Err(EngineError::InvalidTransition(
                "cannot remove unfinished native execution".into(),
            ));
        }
        state.execution.take();
        state.removed = true;
        Ok(())
    }
}

impl Drop for ServiceDriver {
    fn drop(&mut self) {
        self.revoke_egress();
        if let Some(lan) = self.lan.get() {
            lan.close_generation();
        }
    }
}

fn runtime_error(error: anyhow::Error) -> EngineError {
    EngineError::InvalidTransition(error.to_string())
}

#[cfg(test)]
#[path = "service_egress_startup_tests.rs"]
mod egress_startup_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Default)]
    struct FakeWorker {
        kills: AtomicU64,
        deadline: AtomicU64,
    }
    impl WorkerPlatform for FakeWorker {
        fn terminate_after(&self, seconds: u64) {
            self.deadline.store(seconds + 1, Ordering::SeqCst);
        }
        fn terminate(&self) {
            self.kills.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn driver(platform: Arc<FakeWorker>) -> Arc<ServiceDriver> {
        Arc::new(ServiceDriver {
            id: "owned".into(),
            generation: "one".into(),
            platform,
            network: OnceLock::new(),
            lan: OnceLock::new(),
            startup_stop: wasmdroid_engine::network::lan::LanRevocation::new(),
            pending_egress: OnceLock::new(),
            regrants: crate::service_egress_regrant::RegrantAdmission::new(),
            input: OnceLock::new(),
            terminal: OnceLock::new(),
            state: Mutex::new(State {
                session: None,
                spec: None,
                execution: None,
                started: false,
                removed: false,
                network_config: None,
                prepared_egress: None,
            }),
        })
    }
    #[test]
    fn termination_uses_the_trusted_endpoint_and_does_not_take_an_execution_lock() {
        let worker = Arc::<FakeWorker>::default();
        let driver = driver(worker.clone());
        let _held = driver.state.lock().unwrap();
        assert!(driver.kill("other").is_err());
        assert_eq!(worker.kills.load(Ordering::SeqCst), 0);
        driver.kill("owned").unwrap();
        assert_eq!(worker.kills.load(Ordering::SeqCst), 1);
        assert!(driver.stop("other", StopSignal::Terminate, 5).is_err());
        assert!(driver.stop("owned", StopSignal::Terminate, 61).is_err());
        assert_eq!(worker.deadline.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn a_blocked_execution_lock_does_not_prevent_platform_deadline_scheduling() {
        let worker = Arc::<FakeWorker>::default();
        let driver = driver(worker.clone());
        let held = driver.state.lock().unwrap();
        let other = driver.clone();
        let thread = std::thread::spawn(move || other.stop("owned", StopSignal::Terminate, 5));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while worker.deadline.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        let scheduled = worker.deadline.load(Ordering::SeqCst);
        drop(held);
        assert!(thread.join().unwrap().is_err()); // No actual execution in this fixture.
        assert_eq!(scheduled, 6);
    }
}

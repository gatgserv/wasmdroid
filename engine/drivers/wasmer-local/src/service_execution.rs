//! One package service's complete WASIX execution group. Platform controllers
//! must host this in their owned worker; OS death is the final cleanup boundary.
use crate::{
    http_execution::{ForwardStop, forward},
    service_input::{self, InputControl},
    service_network::ServiceNetwork,
    service_startup_port,
    service_stdio::ServiceOutput,
    tasks::TrackedTasks,
    volume_fs::VolumeFs,
    webc_execution::{ExecutionResult, load_verified_package},
};
use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::oneshot};
use wasmdroid_engine::{
    api::{ArtifactKind, InstanceSpec, MountSource, NetworkMode, PortBinding, Protocol},
    session::EngineSession,
    store::ContentStore,
    volumes::VolumeManager,
};
use wasmer_wasix::{
    PluggableRuntime, Runtime,
    bin_factory::spawn_exec,
    os::task::{TaskJoinHandle, process::WasiProcess},
    runners::wasi::{PackageOrHash, RuntimeOrEngine, WasiRunner},
};
use wasmer_wasix_types::wasi::Signal;

pub fn validate_spec(spec: &InstanceSpec) -> wasmdroid_engine::Result<()> {
    validate_spec_for_egress_configuration(spec)?;
    if spec.internet_access || spec.lan_access {
        return Err(wasmdroid_engine::EngineError::Unsupported(
            "internet/LAN permission requires an actual startup capability".into(),
        ));
    }
    Ok(())
}

/// Explicit platform-capable configuration validation only. This is never run
/// authorization: positive execution also needs the one-use native handoff.
pub fn validate_spec_for_egress_configuration(spec: &InstanceSpec) -> wasmdroid_engine::Result<()> {
    use wasmdroid_engine::EngineError::Unsupported;
    spec.validate()?;
    if spec.endpoint_id != "local" {
        return Err(Unsupported("remote endpoint".into()));
    }
    if spec.network == NetworkMode::Lan
        || ((spec.internet_access || spec.lan_access) && spec.network == NetworkMode::None)
    {
        return Err(Unsupported("outbound capability requires enabled private networking; LAN publication is unsupported".into()));
    }
    if spec.resources.cpu_shares.is_some() || spec.resources.memory_max_bytes.is_some() {
        return Err(Unsupported("CPU shares or memory limit".into()));
    }
    if spec
        .resources
        .timeout_seconds
        .is_some_and(|value| value > (i64::MAX as u64) / 1000)
    {
        return Err(wasmdroid_engine::EngineError::Invalid(
            "timeout exceeds platform timer range".into(),
        ));
    }
    // Restart and explicit boot opt-in are enforced by the common Android
    // coordinator, after observed worker death and durable terminal metadata.
    // They do not grant a single-use native driver permission to resurrect.
    if spec
        .mounts
        .iter()
        .any(|m| matches!(m.source, MountSource::HostPath(_)))
    {
        return Err(Unsupported("ambient host path mounts".into()));
    }
    let mut ports = BTreeSet::new();
    for port in &spec.ports {
        if spec.network == NetworkMode::None || port.protocol != Protocol::Tcp {
            return Err(Unsupported("port forwarding requires loopback TCP".into()));
        }
        if !ports.insert(port.instance_port) {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "duplicate guest TCP port".into(),
            ));
        }
    }
    service_startup_port::configured_port(spec)?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServiceInfo {
    pub ports: Vec<PortBinding>,
    pub compiled_cache: crate::compiled_cache::CacheActivity,
    pub tty: bool,
    pub stdin_open: bool,
    pub terminal_snapshot_saved: bool,
    pub forwarding_drained: bool,
}
pub struct ServiceExecution {
    executor: Option<tokio::runtime::Runtime>,
    tasks: Arc<TrackedTasks>,
    process: WasiProcess,
    execution: TaskJoinHandle,
    forwards: Vec<oneshot::Sender<ForwardStop>>,
    forward_tasks: Vec<tokio::task::JoinHandle<bool>>,
    drain_started: Option<Instant>,
    capture: ServiceOutput,
    started: Instant,
    result: Option<ExecutionResult>,
    info: ServiceInfo,
    cache: Arc<crate::compiled_cache::CacheCounters>,
    input: Option<InputControl>,
    terminal: Arc<service_input::TerminalBridge>,
    network: ServiceNetwork,
    lan: Arc<crate::service_lan::ServiceLan>,
    // Retains the same coordinator lock in this worker, including after IPC death.
    _session: EngineSession,
}
impl ServiceExecution {
    pub fn start(
        session: EngineSession,
        id: &str,
        generation: &str,
        spec: &InstanceSpec,
    ) -> Result<Self> {
        Self::start_with_network(session, id, generation, spec, None)
    }
    pub fn start_with_network(
        session: EngineSession,
        id: &str,
        generation: &str,
        spec: &InstanceSpec,
        config: Option<wasmdroid_engine::network::grants::WorkerNetwork>,
    ) -> Result<Self> {
        Self::start_with_network_and_egress(
            session,
            id,
            generation,
            spec,
            config,
            (None, wasmdroid_engine::network::lan::LanRevocation::new()),
        )
    }
    pub(crate) fn start_with_network_and_egress(
        session: EngineSession,
        id: &str,
        generation: &str,
        spec: &InstanceSpec,
        config: Option<wasmdroid_engine::network::grants::WorkerNetwork>,
        authorization: (
            Option<crate::service_egress::PreparedEgress>,
            wasmdroid_engine::network::lan::LanRevocation,
        ),
    ) -> Result<Self> {
        let (prepared, stop) = authorization;
        let effective = session.state().effective_worker_spec(id)?;
        if (effective.internet() || effective.lan()) && prepared.is_none() {
            return Err(wasmdroid_engine::EngineError::Unsupported(
                "effective outbound permission requires an actual startup capability".into(),
            )
            .into());
        }
        if let Some(prepared) = &prepared {
            validate_spec_for_egress_configuration(spec)?;
            prepared.validate(&session, spec)?;
        } else {
            validate_spec(spec)?;
        }
        anyhow::ensure!(!stop.is_revoked(), "worker startup was cancelled");
        if let Some(config) = &config {
            config.validate(id, generation)?;
            config.validate_membership(session.state())?;
        }
        anyhow::ensure!(
            serde_json::to_value(&session.state().instance(id)?.spec)?
                == serde_json::to_value(spec)?,
            "worker specification differs from its instance"
        );
        // Capture private identity/version/run authority before long package
        // preparation. Caller labels or a reused runtime ID cannot select it.
        let browser_origins = session.state().capture_browser_origins(id, spec)?;
        let output = wasmdroid_engine::logs::live::LogWriter::open(&session, id, generation)?;
        let root = session.state().artifact(&spec.artifact_ref)?;
        wasmdroid_engine::stacks::validate_worker_artifact_pin(spec, &root)?;
        anyhow::ensure!(
            matches!(root.kind, ArtifactKind::Webc | ArtifactKind::Wasm)
                && root.driver == "wasmer-local",
            "unsupported installed artifact driver/kind"
        );
        let mut images = Vec::new();
        if root.reference == wasmdroid_engine::local_import::reference(&root.digest)? {
            images = wasmdroid_engine::local_import::graph(session.state(), &root)?;
        } else {
            anyhow::ensure!(
                root.kind == ArtifactKind::Webc,
                "raw Wasm requires a local import identity"
            );
            let mut pending = vec![root.reference.clone()];
            let mut seen = BTreeSet::new();
            while let Some(reference) = pending.pop() {
                if !seen.insert(reference.as_str().to_owned()) {
                    continue;
                }
                anyhow::ensure!(seen.len() <= 32, "installed graph exceeds package limit");
                let image = session.state().artifact(&reference)?;
                if let Some(dependencies) = image
                    .labels
                    .get(wasmdroid_engine::installer::DEPENDENCIES_LABEL)
                {
                    let dependencies: Vec<wasmdroid_engine::api::ArtifactReference> =
                        serde_json::from_str(dependencies)?;
                    pending.extend(dependencies);
                }
                images.push(image);
            }
        }
        let started = Instant::now();
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let guard = executor.enter();
        let tasks = Arc::new(TrackedTasks::new(executor.handle().clone()));
        let mut runtime = PluggableRuntime::new(tasks.clone());
        let cache = crate::compiled_cache::configure(&mut runtime, &session);
        let network = ServiceNetwork::new(spec.network != NetworkMode::None);
        let broker = if let Some(config) = config {
            Some(crate::service_broker::install(
                &network,
                &session,
                config,
                executor.handle().clone(),
            )?)
        } else {
            None
        };
        runtime.set_networking_implementation(network.clone());
        let store = ContentStore::open(session.root().join("store"))?;
        let package = load_verified_package(&executor, &mut runtime, &store, &root, &images)?;
        let selected = package
            .get_command(&spec.command)
            .context("command absent from installed package")?;
        anyhow::ensure!(
            selected
                .metadata()
                .runner
                .starts_with("https://webc.org/runner/wasi"),
            "unsupported package runner"
        );
        let mut wasi = selected
            .metadata()
            .annotation("wasi")?
            .unwrap_or_else(|| webc::metadata::annotations::Wasi::new(&spec.command));
        let args = service_startup_port::runtime_arguments(spec, &mut wasi)?;
        let (stdin, terminal) = service_input::prepare(spec.tty);
        let (capture, stdout, stderr) = ServiceOutput::new(
            output,
            spec.tty,
            terminal.clone(),
            spec.stdin_open.then(|| stdin.queue()),
        );
        runtime.set_tty(terminal.clone());
        let mut runner = WasiRunner::new();
        let mut envs = spec.env.clone();
        if spec.tty {
            envs.entry("TERM".into())
                .or_insert_with(|| "xterm-256color".into());
        }
        runner
            .with_args(args)
            .with_envs(envs)
            .with_forward_host_env(false)
            .with_stdout(Box::new(stdout))
            .with_stderr(Box::new(stderr));
        if spec.stdin_open {
            runner.with_stdin(Box::new(stdin.clone()));
        } else {
            runner.with_stdin(Box::<wasmer_wasix::virtual_fs::NullFile>::default());
        }
        runner.capabilities_mut().threading.max_threads = Some(32);
        if let Some(cwd) = &spec.working_dir {
            runner.with_current_dir(cwd);
        }
        let volumes = VolumeManager::open(&session)?;
        for mount in &spec.mounts {
            let MountSource::Volume(name) = &mount.source else {
                anyhow::bail!("host path denied");
            };
            runner.with_mount(
                mount.target.clone(),
                Arc::new(VolumeFs(volumes.lease(name, mount.read_only)?)),
            );
        }
        drop(volumes);
        let runtime: Arc<dyn Runtime + Send + Sync> = Arc::new(runtime);
        let env = runner
            .prepare_webc_env(
                &spec.command,
                &wasi,
                PackageOrHash::Package(&package),
                RuntimeOrEngine::Runtime(runtime.clone()),
                None,
            )?
            .build()?;
        let process = env.process.clone();
        let input = if spec.stdin_open {
            Some(service_input::spawn(
                &executor,
                stdin,
                terminal.clone(),
                capture.clone(),
                process.clone(),
            ))
        } else {
            None
        };
        // The mandatory capture/policy is attached before any guest can issue
        // networking. Revalidate native authority after long compilation.
        anyhow::ensure!(!stop.is_revoked(), "worker startup was cancelled");
        if let Some(prepared) = &prepared {
            prepared.install(&session, spec, &network, executor.handle().clone())?;
        }
        // Reserve every listener before guest execution, preserving socket ownership.
        let mut listeners = Vec::new();
        let mut ports = Vec::new();
        let leases = wasmdroid_engine::network::PortLease::reserve_browser_origins(
            session.state(),
            &browser_origins,
        )?;
        wasmdroid_engine::network::PortLease::validate_browser_origins(
            session.state(),
            &browser_origins,
            &leases,
        )?;
        for (binding, lease) in spec.ports.iter().zip(leases) {
            let mut actual = binding.clone();
            actual.host_port = lease.host_address()?.port();
            ports.push(actual);
            let listener = lease.into_listener();
            listener.set_nonblocking(true)?;
            listeners.push((
                TcpListener::from_std(listener)?,
                network.ingress(binding.instance_port),
            ));
        }
        anyhow::ensure!(!stop.is_revoked(), "worker startup was cancelled");
        if let Some(prepared) = &prepared {
            prepared.validate(&session, spec)?;
        }
        let execution = executor.block_on(spawn_exec(package, &spec.command, env, &runtime))?;
        let mut forwards = Vec::new();
        let mut forward_tasks = Vec::new();
        if let Some((sender, task)) = broker {
            forwards.push(sender);
            forward_tasks.push(task);
        }
        for (listener, network) in listeners {
            let (send, receive) = oneshot::channel();
            forwards.push(send);
            forward_tasks.push(executor.spawn(forward(listener, network, receive)));
        }
        let lan = crate::service_lan::ServiceLan::new(
            id.into(),
            generation.into(),
            ports.iter().map(|binding| binding.instance_port).collect(),
            network.clone(),
            executor.handle().clone(),
        );
        drop(guard);
        Ok(Self {
            executor: Some(executor),
            tasks,
            process,
            execution,
            forwards,
            forward_tasks,
            drain_started: None,
            capture,
            started,
            result: None,
            info: ServiceInfo {
                ports,
                compiled_cache: cache.snapshot(),
                tty: spec.tty,
                stdin_open: spec.stdin_open,
                terminal_snapshot_saved: false,
                forwarding_drained: false,
            },
            cache,
            input,
            terminal,
            network,
            lan,
            _session: session,
        })
    }
    pub fn info(&self) -> ServiceInfo {
        ServiceInfo {
            ports: self.info.ports.clone(),
            compiled_cache: self.cache.snapshot(),
            tty: self.info.tty,
            stdin_open: self.info.stdin_open,
            terminal_snapshot_saved: self.capture.terminal_saved(),
            forwarding_drained: self.info.forwarding_drained,
        }
    }
    pub(crate) fn network_handle(&self) -> ServiceNetwork {
        self.network.clone()
    }
    pub(crate) fn egress_runtime_context(
        &self,
    ) -> wasmdroid_engine::Result<(
        tokio::runtime::Handle,
        wasmdroid_engine::session::EngineConnection,
    )> {
        Ok((
            self.executor
                .as_ref()
                .ok_or_else(|| {
                    wasmdroid_engine::EngineError::Conflict("worker executor is absent".into())
                })?
                .handle()
                .clone(),
            self._session.connection_handle(),
        ))
    }
    pub(crate) fn lan_handle(&self) -> Arc<crate::service_lan::ServiceLan> {
        self.lan.clone()
    }
    pub(crate) fn egress_context(
        &self,
        id: &str,
    ) -> wasmdroid_engine::Result<(
        tokio::runtime::Handle,
        wasmdroid_engine::session::EngineOwnership,
        wasmdroid_engine::state::InstanceRecord,
    )> {
        let handle = self
            .executor
            .as_ref()
            .ok_or_else(|| {
                wasmdroid_engine::EngineError::Conflict("worker executor is absent".into())
            })?
            .handle()
            .clone();
        Ok((
            handle,
            self._session.ownership(),
            self._session.state().instance(id)?,
        ))
    }
    pub fn request_stop(&mut self) {
        self.lan.close_generation();
        if let Some(input) = &self.input {
            input.close();
        }
        self.tasks.fence_new_guests();
        self.network.close();
        for stop in self.forwards.drain(..) {
            let _ = stop.send(ForwardStop::Abort);
        }
        self.process.signal_process(Signal::Sigterm);
    }
    pub fn request_kill(&mut self) {
        self.request_stop();
        self.process.signal_process(Signal::Sigkill);
        self.tasks.interrupt_all();
    }
    pub fn input(&self) -> Option<InputControl> {
        self.input.clone()
    }
    pub fn terminal_handle(&self) -> Option<crate::TerminalHandle> {
        self.capture.terminal_handle()
    }
    pub fn interrupt(&self) {
        self.lan.revoke_all();
        self.network.revoke_current_egress();
        self.process.signal_process(Signal::Sigint);
    }
    pub fn resize(&self, cols: u32, rows: u32, width: u32, height: u32) -> std::io::Result<()> {
        self.terminal.resize(cols, rows, width, height)?;
        self.process.signal_process(Signal::Sigwinch);
        Ok(())
    }
    /// Main exit alone is not completion: descendants and callbacks must finish.
    pub fn poll(&mut self) -> Result<Option<ExecutionResult>> {
        if let Some(result) = &self.result {
            return Ok(Some(result.clone()));
        }
        if self.tasks.active_guests() != 0 {
            return Ok(None);
        }
        let Some(status) = self.execution.status().into_finished() else {
            return Ok(None);
        };
        let code = status
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .raw();
        if let Some(input) = &self.input {
            input.close();
        }
        if self.drain_started.is_none() {
            self.lan.close_generation();
            self.tasks.fence_new_guests();
            self.network.fence();
            for stop in self.forwards.drain(..) {
                let _ = stop.send(ForwardStop::Drain);
            }
            self.drain_started = Some(Instant::now());
        }
        // Never wait under the driver's execution mutex. The independent LAN
        // owner task drains; a later poll observes its actual terminal receipt.
        if !self.lan.drained()? || !self.network.egress_drained()? {
            return Ok(None);
        }
        if self.forward_tasks.iter().any(|task| !task.is_finished())
            && self
                .drain_started
                .is_some_and(|started| started.elapsed() < Duration::from_secs(3))
        {
            return Ok(None);
        }
        let mut drained = true;
        for task in self.forward_tasks.drain(..) {
            if task.is_finished() {
                drained = self
                    .executor
                    .as_ref()
                    .context("missing worker executor")?
                    .block_on(task)
                    .unwrap_or(false)
                    && drained;
            } else {
                task.abort();
                drained = false;
            }
        }
        self.info.forwarding_drained = drained;
        self.network.close();
        let (stdout, stderr) = self.capture.finish()?;
        let result = ExecutionResult {
            exit_code: code,
            stdout: stdout.0,
            stderr: stderr.0,
            elapsed_ms: self
                .started
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            stdout_truncated: stdout.1,
            stderr_truncated: stderr.1,
        };
        self.result = Some(result.clone());
        Ok(Some(result))
    }
}
impl Drop for ServiceExecution {
    fn drop(&mut self) {
        self.request_kill();
        // A failed native unwind may survive this bounded shutdown. The Android
        // controller still owns the worker and must await its OS/Binder death.
        if let Some(executor) = self.executor.take() {
            executor.shutdown_timeout(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod origin_preflight_tests {
    #[test]
    fn outbound_lan_request_remains_unsupported_without_runtime_grant_integration() {
        let mut spec: wasmdroid_engine::api::InstanceSpec =
            serde_json::from_value(serde_json::json!({
                "artifact_ref":"wasmer.io/example/service@1.0.0","command":"serve"
            }))
            .unwrap();
        assert!(super::validate_spec(&spec).is_ok());
        spec.lan_access = true;
        assert!(matches!(
            super::validate_spec(&spec),
            Err(wasmdroid_engine::EngineError::Unsupported(_))
        ));
    }
}

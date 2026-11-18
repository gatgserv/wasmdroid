//! Supervised Stage 0 HTTP harness using verified WebC and inbound-only TCP.
//! The guest sees an in-memory document root; no host files or sockets are exposed.
use crate::{
    loopback::InboundNetwork,
    tasks::TrackedTasks,
    webc_execution::{ExecutionResult, capture_tail, load_verified_package},
};
use anyhow::{Context, Result};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::{io::copy_bidirectional, net::TcpListener, sync::oneshot, task::JoinSet};
use wasmdroid_engine::{
    api::{Artifact, NetworkMode, PortBinding, Protocol},
    network::PortLease,
    store::ContentStore,
};
use wasmer_wasix::{
    Pipe, PluggableRuntime, Runtime,
    bin_factory::spawn_exec,
    os::task::{TaskJoinHandle, process::WasiProcess},
    runners::wasi::{PackageOrHash, RuntimeOrEngine, WasiRunner},
    virtual_fs::TmpFileSystem,
};
use wasmer_wasix_types::wasi::Signal;

pub const HTTP_DOCUMENT: &str =
    "<!doctype html><title>WasmDroid</title><h1>WASMDROID_HTTP_OK</h1>\n";
static NEXT_DOCUMENT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct HttpStopResult {
    pub execution: ExecutionResult,
    pub forced: bool,
    pub active_execution_tasks: u32,
}

pub(crate) struct Capture {
    finished: Arc<AtomicBool>,
    stdout: Option<JoinHandle<std::io::Result<(String, bool)>>>,
    stderr: Option<JoinHandle<std::io::Result<(String, bool)>>>,
}
impl Capture {
    pub(crate) fn new() -> (Self, Pipe, Pipe) {
        let (stdout, reader) = Pipe::channel();
        let (stderr, errors) = Pipe::channel();
        let finished = Arc::new(AtomicBool::new(false));
        let out_finished = finished.clone();
        let err_finished = finished.clone();
        (
            Self {
                finished,
                stdout: Some(std::thread::spawn(move || {
                    capture_tail(reader, 64 * 1024, out_finished)
                })),
                stderr: Some(std::thread::spawn(move || {
                    capture_tail(errors, 64 * 1024, err_finished)
                })),
            },
            stdout,
            stderr,
        )
    }
    pub(crate) fn finish(&mut self) -> Result<((String, bool), (String, bool))> {
        self.finished.store(true, Ordering::Release);
        let stdout = self
            .stdout
            .take()
            .context("output already consumed")?
            .join()
            .map_err(|_| anyhow::anyhow!("stdout capture panicked"))??;
        let stderr = self
            .stderr
            .take()
            .context("errors already consumed")?
            .join()
            .map_err(|_| anyhow::anyhow!("stderr capture panicked"))??;
        Ok((stdout, stderr))
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Release);
        if let Some(thread) = self.stdout.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr.take() {
            let _ = thread.join();
        }
    }
}

pub struct HttpInstance {
    executor: tokio::runtime::Runtime,
    tasks: Arc<TrackedTasks>,
    process: WasiProcess,
    execution: TaskJoinHandle,
    network: InboundNetwork,
    forward_stop: Option<oneshot::Sender<ForwardStop>>,
    forward_task: Option<tokio::task::JoinHandle<bool>>,
    capture: Capture,
    url: String,
    document_token: String,
    started: Instant,
    result: Option<HttpStopResult>,
}

impl HttpInstance {
    /// Fixed public-server acceptance recipe. This is not yet arbitrary instance
    /// configuration; unsupported capabilities are never silently enabled.
    pub fn start(
        store: &ContentStore,
        root: &Artifact,
        images: &[Artifact],
        progress: &mut dyn FnMut(&str),
    ) -> Result<Self> {
        let started = Instant::now();
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let _guard = executor.enter();
        let tasks = Arc::new(TrackedTasks::new(executor.handle().clone()));
        let mut runtime = PluggableRuntime::new(tasks.clone());
        let network = InboundNetwork::new(8080);
        runtime.set_networking_implementation(network.clone());
        progress("parsing_verified_webc");
        let package = load_verified_package(&executor, &mut runtime, store, root, images)?;
        let command = "static-web-server";
        let selected = package
            .get_command(command)
            .context("static-web-server command is missing")?;
        anyhow::ensure!(
            selected
                .metadata()
                .runner
                .starts_with("https://webc.org/runner/wasi"),
            "unsupported HTTP runner"
        );
        let wasi = selected
            .metadata()
            .annotation("wasi")?
            .unwrap_or_else(|| webc::metadata::annotations::Wasi::new(command));
        let fs = TmpFileSystem::new();
        // A public test marker, never an authentication token. Different roots
        // let device tests detect cross-instance forwarding or filesystem leaks.
        let document_token = format!(
            "WASMDROID_INSTANCE_{}",
            NEXT_DOCUMENT.fetch_add(1, Ordering::Relaxed)
        );
        let document = format!("{HTTP_DOCUMENT}<p>{document_token}</p>\n");
        fs.new_open_options_ext().insert_ro_file(
            std::path::Path::new("/index.html"),
            document.into_bytes().into(),
        )?;
        let (capture, stdout, stderr) = Capture::new();
        let mut runner = WasiRunner::new();
        runner
            .with_args([
                "--host",
                "127.0.0.1",
                "--port",
                "8080",
                "--root",
                "/public",
                "--threads-multiplier",
                "1",
                "--max-blocking-threads",
                "2",
            ])
            .with_forward_host_env(false)
            .with_mount("/public".into(), Arc::new(fs))
            .with_stdout(Box::new(stdout))
            .with_stderr(Box::new(stderr));
        let runtime: Arc<dyn Runtime + Send + Sync> = Arc::new(runtime);
        let env = runner
            .prepare_webc_env(
                command,
                &wasi,
                PackageOrHash::Package(&package),
                RuntimeOrEngine::Runtime(runtime.clone()),
                None,
            )?
            .build()?;
        let process = env.process.clone();
        let lease = PortLease::reserve(
            &PortBinding {
                host_port: 0,
                instance_port: 8080,
                protocol: Protocol::Tcp,
            },
            NetworkMode::Loopback,
        )?;
        let url = format!("http://{}/", lease.host_address()?);
        let listener = lease.into_listener();
        listener.set_nonblocking(true)?;
        let listener = TcpListener::from_std(listener)?;
        progress("compiling_and_starting_http");
        let execution = executor.block_on(spawn_exec(package, command, env, &runtime))?;
        let (forward_stop, stop) = oneshot::channel();
        let forward_task = executor.spawn(forward(listener, network.clone(), stop));
        drop(_guard);
        progress("http_port_reserved");
        Ok(Self {
            executor,
            tasks,
            process,
            execution,
            network,
            forward_stop: Some(forward_stop),
            forward_task: Some(forward_task),
            capture,
            url,
            document_token,
            started,
            result: None,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn document_token(&self) -> &str {
        &self.document_token
    }

    /// Wake blocked guest I/O and wait for actual exit without interrupting a
    /// native thread inside Wasmer's experimental host-signal trap handler.
    pub fn stop(&mut self) -> Result<HttpStopResult> {
        if let Some(result) = &self.result {
            return Ok(result.clone());
        }
        self.tasks.fence_new_guests();
        if let Some(stop) = self.forward_stop.take() {
            let _ = stop.send(ForwardStop::Abort);
        }
        self.process.signal_process(Signal::Sigint);
        self.network.close();
        if let Some(forward) = self.forward_task.take() {
            self.executor.block_on(forward)?;
        }
        let (code, forced) = self.executor.block_on(async {
            match tokio::time::timeout(
                Duration::from_secs(5),
                wait_for_probe_exit(&mut self.execution, &self.tasks),
            )
            .await
            {
                Ok(result) => result.map(|code| (code, false)),
                Err(_) => {
                    // Guest-directed SIGKILL wakes WASIX atomic waiters. It
                    // does not send a native signal to a running host thread.
                    self.process.signal_process(Signal::Sigkill);
                    let code = tokio::time::timeout(
                        Duration::from_secs(10),
                        wait_for_probe_exit(&mut self.execution, &self.tasks),
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "HTTP guest did not confirm exit ({} active callbacks)",
                            self.tasks.active()
                        )
                    })??;
                    Ok((code, true))
                }
            }
        })?;
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
        let result = HttpStopResult {
            execution: result,
            forced,
            active_execution_tasks: self.tasks.active().try_into().unwrap_or(u32::MAX),
        };
        self.result = Some(result.clone());
        Ok(result)
    }
}

async fn wait_for_probe_exit(execution: &mut TaskJoinHandle, tasks: &TrackedTasks) -> Result<i32> {
    let result = execution.wait_finished().await;
    // Main status can settle while secondary execution callbacks are alive.
    while tasks.active() != 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    result
        .map(|code| code.raw())
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

impl Drop for HttpInstance {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(crate) trait ForwardTarget: Clone + Send + Sync + 'static {
    fn connect_forwarder(
        &self,
    ) -> Option<Box<dyn wasmer_wasix::virtual_net::VirtualTcpSocket + Sync>>;
}
pub(crate) enum ForwardStop {
    Abort,
    Drain,
}
impl ForwardTarget for InboundNetwork {
    fn connect_forwarder(
        &self,
    ) -> Option<Box<dyn wasmer_wasix::virtual_net::VirtualTcpSocket + Sync>> {
        InboundNetwork::connect_forwarder(self)
    }
}

pub(crate) async fn forward<T: ForwardTarget>(
    listener: TcpListener,
    network: T,
    mut stop: oneshot::Receiver<ForwardStop>,
) -> bool {
    let mut connections = JoinSet::new();
    let shutdown = loop {
        tokio::select! {
            command=&mut stop=>break command.unwrap_or(ForwardStop::Abort),
            accepted=listener.accept()=>{
                let Ok((mut host,_))=accepted else { break ForwardStop::Abort; };
                if connections.len()>=32 { continue; }
                let network=network.clone();
                connections.spawn(async move {
                    let guest=tokio::time::timeout(Duration::from_secs(30),async {
                        loop {
                            if let Some(guest)=network.connect_forwarder() { break guest; }
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                    }).await;
                    // Only waiting for the guest listener has a startup timeout.
                    // Established streams (databases, WebSockets, keep-alive)
                    // live until EOF/error or service-stop cancellation.
                    if let Ok(mut guest)=guest {let _=copy_bidirectional(&mut host,&mut guest).await;}
                });
            },
            _=connections.join_next(), if !connections.is_empty()=>{}
        }
    };
    drop(listener);
    let drained = matches!(shutdown, ForwardStop::Drain)
        && tokio::time::timeout(Duration::from_secs(2), async {
            let mut success = true;
            while let Some(result) = connections.join_next().await {
                success = success && result.is_ok();
            }
            success
        })
        .await
        .unwrap_or(false);
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    drained
}

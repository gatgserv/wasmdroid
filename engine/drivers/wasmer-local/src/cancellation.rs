//! Real WASIX CPU-loop cancellation regression, separate from product commands.
use crate::tasks::TrackedTasks;
use anyhow::{Context, Result};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    PluggableRuntime, Runtime,
    bin_factory::spawn_exec_module,
    os::task::TaskJoinHandle,
    runners::wasi::{PackageOrHash, RuntimeOrEngine, WasiRunner},
};
use wasmer_wasix_types::wasi::Signal;

#[derive(Debug)]
struct ProbeRuntime {
    inner: PluggableRuntime,
    taints: std::sync::Mutex<Vec<String>>,
    host_interrupted: std::sync::atomic::AtomicBool,
}
impl ProbeRuntime {
    fn new(tasks: Arc<TrackedTasks>) -> Self {
        Self {
            inner: PluggableRuntime::new(tasks),
            taints: Default::default(),
            host_interrupted: Default::default(),
        }
    }
}
impl Runtime for ProbeRuntime {
    fn networking(&self) -> &wasmer_wasix::virtual_net::DynVirtualNetworking {
        self.inner.networking()
    }
    fn task_manager(&self) -> &Arc<dyn wasmer_wasix::runtime::VirtualTaskManager> {
        self.inner.task_manager()
    }
    fn source(&self) -> Arc<dyn wasmer_wasix::runtime::resolver::Source + Send + Sync> {
        self.inner.source()
    }
    fn engine(&self) -> wasmer::Engine {
        self.inner.engine()
    }
    fn on_taint(&self, reason: wasmer_wasix::runtime::TaintReason) {
        use wasmer_wasix::runtime::TaintReason;
        let reason = match reason {
            TaintReason::RuntimeError(error) => {
                if error.clone().to_trap() == Some(wasmer_types::TrapCode::HostInterrupt) {
                    self.host_interrupted
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                format!("{error:?}")
            }
            TaintReason::UnknownWasiVersion => "unknown WASI version".into(),
            TaintReason::NonZeroExitCode(code) => format!("exit:{}", code.raw()),
            TaintReason::DlSymbolResolutionFailed(_) => "unresolved fixture symbol".into(),
        };
        self.taints.lock().unwrap().push(reason);
    }
}

#[derive(Debug, serde::Serialize)]
pub struct CancellationResult {
    pub first_host_interrupt_confirmed: bool,
    pub second_host_interrupt_confirmed: bool,
    pub first_active_tasks_after_stop: usize,
    pub second_was_running_after_first_stop: bool,
    pub second_active_tasks_after_stop: usize,
    pub first_result: String,
    pub second_result: String,
    pub elapsed_ms: u64,
}

pub fn run_probe() -> Result<CancellationResult> {
    anyhow::ensure!(
        cfg!(unix),
        "native interruption is unavailable on this host"
    );
    let started = Instant::now();
    let executor = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _guard = executor.enter();
    let first_tasks = Arc::new(TrackedTasks::new(executor.handle().clone()));
    let second_tasks = Arc::new(TrackedTasks::new(executor.handle().clone()));
    let first_runtime = Arc::new(ProbeRuntime::new(first_tasks.clone()));
    let second_runtime = Arc::new(ProbeRuntime::new(second_tasks.clone()));
    let wasm = wasmer::wat2wasm(include_bytes!("../../../../fixtures/hot-loop-wasi.wat"))?;
    let spawn = |runtime: Arc<dyn Runtime + Send + Sync>| -> Result<_> {
        let module = Module::new(&runtime.engine(), &wasm)?;
        let env = WasiRunner::new()
            .prepare_webc_env(
                "cpu-loop",
                &webc::metadata::annotations::Wasi::new("cpu-loop"),
                PackageOrHash::Hash(ModuleHash::new(&wasm)),
                RuntimeOrEngine::Runtime(runtime.clone()),
                None,
            )?
            .build()?;
        let process = env.process.clone();
        let handle = spawn_exec_module(module, env, &runtime)?;
        Ok((process, handle))
    };
    let (first_process, mut first) = spawn(first_runtime.clone())?;
    let (second_process, mut second) = match spawn(second_runtime.clone()) {
        Ok(second) => second,
        Err(error) => {
            first_process.signal_process(Signal::Sigkill);
            let _ = stop(&executor, &first_tasks, &mut first);
            return Err(error);
        }
    };
    // Give both independently owned stores time to enter actual machine code.
    std::thread::sleep(Duration::from_millis(100));
    if first.status().into_finished().is_some()
        || second.status().into_finished().is_some()
        || first_tasks.active() == 0
        || second_tasks.active() == 0
    {
        let diagnostic = format!(
            "hot fixtures did not both start: first={:?}, second={:?}, first errors={:?}, second errors={:?}",
            first.status(),
            second.status(),
            first_runtime.taints.lock().unwrap(),
            second_runtime.taints.lock().unwrap()
        );
        first_process.signal_process(Signal::Sigkill);
        second_process.signal_process(Signal::Sigkill);
        let _ = stop(&executor, &first_tasks, &mut first);
        let _ = stop(&executor, &second_tasks, &mut second);
        anyhow::bail!(diagnostic);
    }
    first_process.signal_process(Signal::Sigkill);
    let first_result = stop(&executor, &first_tasks, &mut first);
    let second_was_running_after_first_stop =
        second.status().into_finished().is_none() && second_tasks.active() > 0;
    // Always stop the second, even if isolation failed, before reporting failure.
    second_process.signal_process(Signal::Sigkill);
    let second_result = stop(&executor, &second_tasks, &mut second);
    let first_result = first_result?;
    let second_result = second_result?;
    let first_host_interrupt_confirmed = first_runtime
        .host_interrupted
        .load(std::sync::atomic::Ordering::Acquire);
    let second_host_interrupt_confirmed = second_runtime
        .host_interrupted
        .load(std::sync::atomic::Ordering::Acquire);
    anyhow::ensure!(
        first_host_interrupt_confirmed && second_host_interrupt_confirmed,
        "CPU callbacks exited without confirmed HostInterrupt traps"
    );
    anyhow::ensure!(
        second_was_running_after_first_stop,
        "canceling one store interrupted another"
    );
    Ok(CancellationResult {
        first_host_interrupt_confirmed,
        second_host_interrupt_confirmed,
        first_active_tasks_after_stop: first_tasks.active(),
        second_was_running_after_first_stop,
        second_active_tasks_after_stop: second_tasks.active(),
        first_result,
        second_result,
        elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

fn stop(
    executor: &tokio::runtime::Runtime,
    tasks: &TrackedTasks,
    handle: &mut TaskJoinHandle,
) -> Result<String> {
    executor.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            tasks.interrupt_all();
            if tasks.active() == 0 {
                break;
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "CPU-loop execution did not actually unwind; registered signal masks: {:?}",
                tasks.interrupt_diagnostics()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let result = tokio::time::timeout(Duration::from_secs(1), handle.wait_finished())
            .await
            .context("task result did not settle after callback exit")?;
        Ok(match result {
            Ok(code) => format!("exit:{}", code.raw()),
            Err(error) => error.to_string(),
        })
    })
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn stops_hot_wasix_loop_without_interrupting_a_second_instance() {
        let result = super::run_probe().unwrap();
        assert_eq!(result.first_active_tasks_after_stop, 0);
        assert_eq!(result.second_active_tasks_after_stop, 0);
        assert!(result.second_was_running_after_first_stop);
    }
}

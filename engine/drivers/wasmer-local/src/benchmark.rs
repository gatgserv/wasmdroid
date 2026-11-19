//! Explicit, finite Stage 0 measurement harness. Registry transfer is outside
//! these timings; warm means same-process compiled memory cache, never disk.
use crate::webc_execution::{ExecutionResult, execute_verified_command, load_verified_package};
use anyhow::{Context, Result};
use std::{borrow::Cow, sync::Arc, time::Instant};
use wasmdroid_engine::{api::Artifact, store::ContentStore};
use wasmer_wasix::{
    PluggableRuntime, Runtime,
    runtime::{ModuleInput, task_manager::tokio::TokioTaskManager},
};

pub fn compiler_name() -> &'static str {
    if cfg!(feature = "cranelift") {
        "cranelift"
    } else if cfg!(feature = "singlepass") {
        "singlepass"
    } else {
        "unavailable"
    }
}

#[derive(Debug, serde::Serialize)]
pub struct BenchmarkResult {
    pub schema_version: u32,
    pub backend: String,
    pub reference: String,
    pub digest: String,
    pub graph_prepare_us: u64,
    pub cold_compile_us: u64,
    pub cold_start_to_exit_us: u64,
    pub warm_cache_lookup_us: u64,
    pub warm_start_to_exit_us: Vec<u64>,
    pub outputs: Vec<ExecutionResult>,
    pub process_rss_bytes: Option<u64>,
    pub process_peak_rss_bytes: Option<u64>,
}

#[derive(Debug, serde::Serialize)]
pub struct CoreBenchmarkResult {
    pub schema_version: u32,
    pub backend: String,
    pub iterations_per_sample: u32,
    pub cold_compile_us: u64,
    pub instantiate_us: u64,
    pub execution_us: Vec<u64>,
    pub checksum: i32,
    pub process_rss_bytes: Option<u64>,
    pub process_peak_rss_bytes: Option<u64>,
}

/// Deterministic integer-only microbenchmark, not a substitute for WebC/WASIX
/// compatibility. No guest imports, filesystem, network or unbounded loop input.
pub fn core() -> Result<CoreBenchmarkResult> {
    use wasmer::{Instance, Module, Store, imports};
    let wasm = wasmer::wat2wasm(include_bytes!("../../../../fixtures/integer-benchmark.wat"))?;
    let mut store = Store::default();
    let compile = Instant::now();
    let module = Module::new(&store, wasm)?;
    let cold_compile_us = micros(compile);
    let start = Instant::now();
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let compute = instance
        .exports
        .get_typed_function::<i32, i32>(&store, "compute")?;
    let instantiate_us = micros(start);
    let iterations_per_sample = 1_000_000u32;
    let checksum = (0..iterations_per_sample).fold(0u32, |value, i| {
        (value ^ i).rotate_left(5).wrapping_add(1013904223)
    }) as i32;
    let mut execution_us = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let value = compute.call(&mut store, iterations_per_sample as i32)?;
        execution_us.push(micros(start));
        anyhow::ensure!(value == checksum, "core benchmark checksum mismatch");
    }
    let (process_rss_bytes, process_peak_rss_bytes) = process_memory();
    Ok(CoreBenchmarkResult {
        schema_version: 1,
        backend: compiler_name().into(),
        iterations_per_sample,
        cold_compile_us,
        instantiate_us,
        execution_us,
        checksum,
        process_rss_bytes,
        process_peak_rss_bytes,
    })
}

pub fn python(
    store: &ContentStore,
    root: &Artifact,
    images: &[Artifact],
    progress: &mut dyn FnMut(&str),
) -> Result<BenchmarkResult> {
    let prepare = Instant::now();
    let executor = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _guard = executor.enter();
    let mut runtime =
        PluggableRuntime::new(Arc::new(TokioTaskManager::new(executor.handle().clone())));
    let package = load_verified_package(&executor, &mut runtime, store, root, images)?;
    let graph_prepare_us = micros(prepare);
    let runtime: Arc<dyn Runtime + Send + Sync> = Arc::new(runtime);
    let command = package
        .get_command("python")
        .context("Python command is missing")?;
    let args = [
        "-c".into(),
        include_str!("../../../../fixtures/python-benchmark.py").into(),
    ];
    anyhow::ensure!(
        !executor.block_on(
            runtime
                .module_cache()
                .contains(*command.hash(), &runtime.engine())
        )?,
        "benchmark cold cache is unexpectedly populated"
    );
    progress("benchmark_cold_compile");
    let cold_start = Instant::now();
    let compile = Instant::now();
    let _module = executor.block_on(runtime.resolve_module(
        ModuleInput::Command(Cow::Borrowed(command)),
        None,
        None,
    ))?;
    let cold_compile_us = micros(compile);
    anyhow::ensure!(
        executor.block_on(
            runtime
                .module_cache()
                .contains(*command.hash(), &runtime.engine())
        )?,
        "compiled module was not cached; warm measurements would be invalid"
    );
    let first = execute_verified_command(runtime.clone(), &package, "python", &args, &mut |_| {})?;
    let cold_start_to_exit_us = micros(cold_start);
    check_output(&first)?;
    let warm_lookup = Instant::now();
    let _warm = executor.block_on(runtime.resolve_module(
        ModuleInput::Command(Cow::Borrowed(command)),
        None,
        None,
    ))?;
    let warm_cache_lookup_us = micros(warm_lookup);
    let mut outputs = vec![first];
    let mut warm_start_to_exit_us = Vec::new();
    for _ in 0..3 {
        progress("benchmark_warm_start");
        let start = Instant::now();
        let result =
            execute_verified_command(runtime.clone(), &package, "python", &args, &mut |_| {})?;
        warm_start_to_exit_us.push(micros(start));
        check_output(&result)?;
        outputs.push(result);
    }
    let (process_rss_bytes, process_peak_rss_bytes) = process_memory();
    Ok(BenchmarkResult {
        schema_version: 1,
        backend: compiler_name().into(),
        reference: root.reference.as_str().into(),
        digest: root.digest.as_str().into(),
        graph_prepare_us,
        cold_compile_us,
        cold_start_to_exit_us,
        warm_cache_lookup_us,
        warm_start_to_exit_us,
        outputs,
        process_rss_bytes,
        process_peak_rss_bytes,
    })
}

fn micros(start: Instant) -> u64 {
    start.elapsed().as_micros().try_into().unwrap_or(u64::MAX)
}
fn check_output(result: &ExecutionResult) -> Result<()> {
    anyhow::ensure!(
        result.exit_code == 0 && result.stdout.contains("WASMDROID_BENCHMARK_OK 4999950000"),
        "Python benchmark failed: {}",
        result.stderr
    );
    Ok(())
}

/// Kernel high-water RSS for the whole current process, including registry/JVM
/// overhead since process launch. Not per-guest memory or a resettable peak.
fn process_memory() -> (Option<u64>, Option<u64>) {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return (None, None);
    };
    (
        status_bytes(&status, "VmRSS:"),
        status_bytes(&status, "VmHWM:"),
    )
}
fn status_bytes(status: &str, label: &str) -> Option<u64> {
    let line = status.lines().find_map(|line| line.strip_prefix(label))?;
    let mut fields = line.split_whitespace();
    let kib: u64 = fields.next()?.parse().ok()?;
    if fields.next() != Some("kB") {
        return None;
    }
    kib.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rss_uses_kernel_kib_units_and_never_invents_missing_measurements() {
        let status = "VmRSS:\t123 kB\nVmHWM: 456 kB\n";
        assert_eq!(status_bytes(status, "VmRSS:"), Some(125952));
        assert_eq!(status_bytes(status, "VmHWM:"), Some(466944));
        assert_eq!(status_bytes("VmRSS: 12 MB", "VmRSS:"), None);
        assert_eq!(status_bytes("VmRSS: unknown kB", "VmRSS:"), None);
        assert_eq!(status_bytes(status, "VmPeak:"), None);
    }
}

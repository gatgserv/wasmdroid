//! Offline WebC execution. All containers are reverified from the engine's CAS;
//! the guest has neither host filesystem mounts nor a host networking backend.
use anyhow::{Context, Result};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use wasmdroid_engine::{api::Artifact, store::ContentStore};
use wasmer_config::package::{NamedPackageId, PackageId};
use wasmer_wasix::{
    Pipe, PluggableRuntime, WasiRuntimeError,
    bin_factory::BinaryPackage,
    runners::wasi::{RuntimeOrEngine, WasiRunner},
    runtime::{
        package_loader::{PackageLoader, load_package_tree},
        resolver::{
            DistributionInfo, InMemorySource, PackageInfo, PackageSummary, Resolution, WebcHash,
        },
        task_manager::tokio::TokioTaskManager,
    },
};
use webc::Container;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug)]
struct OfflineLoader {
    containers: HashMap<WebcHash, Container>,
}

#[async_trait::async_trait]
impl PackageLoader for OfflineLoader {
    async fn load(&self, summary: &PackageSummary) -> Result<Container> {
        self.containers
            .get(&summary.dist.webc_sha256)
            .cloned()
            .context("dependency is not installed in the verified offline set")
    }
    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &Resolution,
        root_is_local_dir: bool,
    ) -> Result<BinaryPackage> {
        load_package_tree(root, self, resolution, root_is_local_dir).await
    }
}

/// Finite Stage 0 harness. Lifecycle supervision and arbitrary user commands are
/// separate from the current FFI probe, which runs a fixed script.
pub fn run_offline(
    store: &ContentStore,
    root: &Artifact,
    images: &[Artifact],
    command: &str,
    args: &[String],
) -> Result<ExecutionResult> {
    run_offline_with_progress(store, root, images, command, args, &mut |_| {})
}

pub fn run_offline_with_progress(
    store: &ContentStore,
    root: &Artifact,
    images: &[Artifact],
    command: &str,
    args: &[String],
    progress: &mut dyn FnMut(&str),
) -> Result<ExecutionResult> {
    progress("parsing_verified_webc");
    let start = Instant::now();
    let executor = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _guard = executor.enter();
    let tasks = TokioTaskManager::new(executor.handle().clone());
    let mut runtime = PluggableRuntime::new(Arc::new(tasks));
    let package = load_verified_package(&executor, &mut runtime, store, root, images)?;
    let mut result =
        execute_verified_command(Arc::new(runtime), &package, command, args, progress)?;
    result.elapsed_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    Ok(result)
}

pub(crate) fn execute_verified_command(
    runtime: Arc<dyn wasmer_wasix::Runtime + Send + Sync>,
    package: &BinaryPackage,
    command: &str,
    args: &[String],
    progress: &mut dyn FnMut(&str),
) -> Result<ExecutionResult> {
    let start = Instant::now();
    progress("preparing_command");
    let selected = package
        .get_command(command)
        .context("command is not present in package")?;
    anyhow::ensure!(
        selected
            .metadata()
            .runner
            .starts_with("https://webc.org/runner/wasi"),
        "unsupported WebC runner"
    );
    let (stdout, stdout_reader) = Pipe::channel();
    let (stderr, stderr_reader) = Pipe::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let stdout_finished = finished.clone();
    let stderr_finished = finished.clone();
    let output_thread =
        std::thread::spawn(move || capture_tail(stdout_reader, 64 * 1024, stdout_finished));
    let error_thread =
        std::thread::spawn(move || capture_tail(stderr_reader, 64 * 1024, stderr_finished));
    progress("compiling_and_executing");
    let result = {
        let mut runner = WasiRunner::new();
        runner
            .with_args(args)
            .with_forward_host_env(false)
            .with_stdout(Box::new(stdout))
            .with_stderr(Box::new(stderr));
        runner.run_command(command, package, RuntimeOrEngine::Runtime(runtime))
    };
    finished.store(true, Ordering::Release);
    progress("draining_output");
    let output = output_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stdout capture failed"))??;
    let errors = error_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stderr capture failed"))??;
    let exit_code = match result {
        Ok(()) => 0,
        Err(error) => match error
            .downcast_ref::<WasiRuntimeError>()
            .and_then(WasiRuntimeError::as_exit_code)
        {
            Some(code) => code.raw(),
            None => {
                return Err(error.context(format!("WebC execution failed; stderr: {}", errors.0)));
            }
        },
    };
    Ok(ExecutionResult {
        exit_code,
        stdout: output.0,
        stderr: errors.0,
        elapsed_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        stdout_truncated: output.1,
        stderr_truncated: errors.1,
    })
}

pub(crate) fn capture_tail(
    mut input: Pipe,
    limit: usize,
    finished: Arc<AtomicBool>,
) -> std::io::Result<(String, bool)> {
    let mut retained = VecDeque::new();
    let mut buffer = [0u8; 8192];
    let mut total = 0usize;
    loop {
        let read = match input.try_read(&mut buffer) {
            Some(0) => break,
            Some(count) => count,
            None if finished.load(Ordering::Acquire) => break,
            None => {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
        };
        total = total.saturating_add(read);
        retained.extend(&buffer[..read]);
        if retained.len() > limit {
            retained.drain(..retained.len() - limit);
        }
    }
    Ok((
        String::from_utf8_lossy(&retained.into_iter().collect::<Vec<_>>()).into_owned(),
        total > limit,
    ))
}

/// Shared verified offline graph loader for finite and supervised commands.
pub(crate) fn load_verified_package(
    executor: &tokio::runtime::Runtime,
    runtime: &mut PluggableRuntime,
    store: &ContentStore,
    root: &Artifact,
    images: &[Artifact],
) -> Result<BinaryPackage> {
    let mut containers = HashMap::new();
    let mut source = InMemorySource::new();
    for image in images {
        let bytes = store.read_verified(&image.digest, image.size_bytes)?;
        anyhow::ensure!(
            bytes.len() as u64 == image.size_bytes,
            "installed size mismatch"
        );
        let container = Container::from_bytes_and_version(bytes.into(), webc::Version::V3)?;
        let named = image
            .reference
            .as_str()
            .strip_prefix("wasmer.io/")
            .context("unsupported registry in offline set")?;
        let (name, version) = named
            .rsplit_once('@')
            .context("offline images must be version pinned")?;
        let id = PackageId::Named(NamedPackageId {
            full_name: name.into(),
            version: version.parse()?,
        });
        let hash = WebcHash::parse_hex(image.digest.as_str().trim_start_matches("sha256:"))?;
        let info = PackageInfo::from_manifest(id, container.manifest(), container.version())?;
        // Descriptive only: OfflineLoader never opens URLs.
        let location = format!(
            "https://offline.invalid/{}",
            image.digest.as_str().trim_start_matches("sha256:")
        )
        .parse()?;
        source.add(PackageSummary {
            pkg: info,
            dist: DistributionInfo {
                webc: location,
                webc_sha256: hash,
            },
        });
        containers.insert(hash, container);
    }
    let root_hash = WebcHash::parse_hex(root.digest.as_str().trim_start_matches("sha256:"))?;
    let container = containers
        .get(&root_hash)
        .context("root package is missing from installed graph")?
        .clone();
    runtime
        .set_source(source)
        .set_package_loader(OfflineLoader { containers });
    executor.block_on(BinaryPackage::from_webc(&container, runtime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn drains_bounded_tail_after_exit_while_descriptor_clones_remain_open() {
        let (mut writer, reader) = Pipe::channel();
        writer.write_all(&vec![b'x'; 9000]).unwrap();
        writer.write_all(b"tail").unwrap();
        let finished = Arc::new(AtomicBool::new(true));
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(capture_tail(reader, 4, finished));
        });
        let result = receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("capture must finish without waiting for every writer clone to close")
            .unwrap();
        assert_eq!(result, ("tail".into(), true));
        drop(writer);
    }
}

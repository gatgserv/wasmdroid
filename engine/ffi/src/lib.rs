uniffi::setup_scaffolding!();

#[derive(Debug, uniffi::Record)]
pub struct EngineInfo {
    pub api_version: u32,
    pub version: String,
    pub endpoint_id: String,
    pub runtime_probe_available: bool,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("{reason}")]
    Operation { reason: String },
}

#[uniffi::export]
pub fn engine_info() -> EngineInfo {
    EngineInfo {
        api_version: wasmdroid_engine::api::API_VERSION,
        version: env!("CARGO_PKG_VERSION").into(),
        endpoint_id: "local".into(),
        runtime_probe_available: cfg!(feature = "wasmer"),
    }
}

/// Boundary uses the versioned, serializable catalog contract. No UI policy here.
#[uniffi::export]
pub fn validate_catalog(json: String) -> Result<u32, BridgeError> {
    wasmdroid_engine::catalog::Catalog::parse(&json)
        .map(|catalog| catalog.packages.len() as u32)
        .map_err(|err| BridgeError::Operation {
            reason: err.to_string(),
        })
}

/// Finite built-in WASI probe only. Dispatch off the Android main thread.
#[uniffi::export]
pub fn run_wasi_probe() -> Result<String, BridgeError> {
    #[cfg(feature = "wasmer")]
    {
        wasmdroid_wasmer_local::run_probe().map_err(|err| BridgeError::Operation {
            reason: err.to_string(),
        })
    }
    #[cfg(not(feature = "wasmer"))]
    {
        Err(BridgeError::Operation {
            reason: "This build does not include the Wasmer runtime probe".into(),
        })
    }
}

#[derive(Debug, uniffi::Record)]
pub struct RegistryProbeResult {
    pub reference: String,
    pub digest: String,
    pub installed_images: u32,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub elapsed_ms: u64,
}

#[uniffi::export(callback_interface)]
pub trait ProbeObserver: Send + Sync {
    fn on_phase(&self, phase: String);
}

/// Explicit live Stage 0 acceptance harness, not a user-facing arbitrary runner.
/// Pulls the pinned Python release and transitive dependencies, then executes
/// only the fixed sandbox regression script without host networking or mounts.
#[uniffi::export]
pub fn run_registry_python_probe(
    data_dir: String,
    observer: Option<Box<dyn ProbeObserver>>,
) -> Result<RegistryProbeResult, BridgeError> {
    #[cfg(feature = "wasmer")]
    {
        registry_python_probe(&data_dir, observer.as_deref()).map_err(|error| {
            BridgeError::Operation {
                reason: error.to_string(),
            }
        })
    }
    #[cfg(not(feature = "wasmer"))]
    {
        let _ = (data_dir, observer);
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

#[cfg(feature = "wasmer")]
fn registry_python_probe(
    data_dir: &str,
    observer: Option<&dyn ProbeObserver>,
) -> Result<RegistryProbeResult, Box<dyn std::error::Error>> {
    use wasmdroid_engine::{
        api::ArtifactReference,
        installer::{InstallLimits, Installer},
        registry::wasmer::WasmerRegistry,
        state::StateDb,
        store::ContentStore,
    };
    let root = std::path::Path::new(data_dir);
    std::fs::create_dir_all(root)?;
    let registry = WasmerRegistry::new()?;
    let store = ContentStore::open(root.join("store"))?;
    let state = StateDb::open(root.join("engine.sqlite"))?;
    let installer = Installer {
        registry: &registry,
        store: &store,
        state: &state,
        limits: InstallLimits::default(),
    };
    if let Some(observer) = observer {
        observer.on_phase("resolving_registry".into());
    }
    let graph = installer.pull(
        &ArtifactReference::parse("wasmer.io/python/python@3.13.20")?,
        &mut |progress| {
            if progress.bytes_downloaded == 0
                && let Some(observer) = observer
            {
                observer.on_phase(format!(
                    "{:?}:{}",
                    progress.phase,
                    progress.reference.as_str()
                ));
            }
        },
    )?;
    let script = include_str!("../../../fixtures/python-sandbox-probe.py");
    let result = wasmdroid_wasmer_local::webc_execution::run_offline_with_progress(
        &store,
        &graph.root,
        &graph.images,
        "python",
        &["-c".into(), script.into()],
        &mut |phase| {
            if let Some(observer) = observer {
                observer.on_phase(phase.into());
            }
        },
    )?;
    Ok(RegistryProbeResult {
        reference: graph.root.reference.as_str().into(),
        digest: graph.root.digest.as_str().into(),
        installed_images: graph.images.len() as u32,
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
        elapsed_ms: result.elapsed_ms,
    })
}

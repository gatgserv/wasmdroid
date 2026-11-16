uniffi::setup_scaffolding!();

#[derive(Debug, uniffi::Record)]
pub struct EngineInfo {
    pub api_version: u32,
    pub version: String,
    pub endpoint_id: String,
    pub runtime_probe_available: bool,
    pub runtime_backend: String,
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
        runtime_probe_available: cfg!(feature = "runtime"),
        runtime_backend: {
            #[cfg(feature = "runtime")]
            {
                wasmdroid_wasmer_local::benchmark::compiler_name().into()
            }
            #[cfg(not(feature = "runtime"))]
            {
                "unavailable".into()
            }
        },
    }
}

/// Explicit fixed-workload benchmark. Invoke off the main thread. The returned
/// JSON records raw samples and units; no performance threshold is a test gate.
#[uniffi::export]
pub fn run_registry_benchmark(
    data_dir: String,
    observer: Option<Box<dyn ProbeObserver>>,
) -> Result<String, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        let result = (|| -> Result<String, Box<dyn std::error::Error>> {
            use wasmdroid_engine::{
                api::ArtifactReference,
                installer::{InstallLimits, Installer},
                registry::wasmer::WasmerRegistry,
                state::StateDb,
                store::ContentStore,
            };
            let root = std::path::Path::new(&data_dir);
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
            if let Some(observer) = &observer {
                observer.on_phase("benchmark_registry_setup_excluded_from_timings".into());
            }
            let graph = installer.pull(
                &ArtifactReference::parse("wasmer.io/python/python@3.13.20")?,
                &mut |_| {},
            )?;
            let result = wasmdroid_wasmer_local::benchmark::python(
                &store,
                &graph.root,
                &graph.images,
                &mut |phase| {
                    if let Some(observer) = &observer {
                        observer.on_phase(phase.into());
                    }
                },
            )?;
            Ok(serde_json::to_string(&result)?)
        })();
        result.map_err(|error| BridgeError::Operation {
            reason: error.to_string(),
        })
    }
    #[cfg(not(feature = "runtime"))]
    {
        let _ = (data_dir, observer);
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

#[uniffi::export]
pub fn run_core_benchmark() -> Result<String, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        wasmdroid_wasmer_local::benchmark::core()
            .and_then(|result| Ok(serde_json::to_string(&result)?))
            .map_err(|error| BridgeError::Operation {
                reason: error.to_string(),
            })
    }
    #[cfg(not(feature = "runtime"))]
    {
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
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
    #[cfg(feature = "runtime")]
    {
        wasmdroid_wasmer_local::run_probe().map_err(|err| BridgeError::Operation {
            reason: err.to_string(),
        })
    }
    #[cfg(not(feature = "runtime"))]
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
    #[cfg(feature = "runtime")]
    {
        registry_python_probe(&data_dir, observer.as_deref()).map_err(|error| {
            BridgeError::Operation {
                reason: error.to_string(),
            }
        })
    }
    #[cfg(not(feature = "runtime"))]
    {
        let _ = (data_dir, observer);
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

#[cfg(feature = "runtime")]
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct HttpProbeInfo {
    pub reference: String,
    pub digest: String,
    pub installed_images: u32,
    pub url: String,
    pub document_token: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct HttpProbeStopResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
    pub forced: bool,
    pub active_execution_tasks: u32,
}

/// Explicit live HTTP acceptance instance, owned until stop/drop. Product
/// lifecycle and Android service ownership will use the neutral driver API.
#[derive(uniffi::Object)]
pub struct RegistryHttpProbe {
    info: HttpProbeInfo,
    #[cfg(feature = "runtime")]
    inner: std::sync::Mutex<wasmdroid_wasmer_local::http_execution::HttpInstance>,
}

#[uniffi::export]
impl RegistryHttpProbe {
    pub fn info(&self) -> HttpProbeInfo {
        self.info.clone()
    }

    pub fn stop(&self) -> Result<HttpProbeStopResult, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            let mut inner = self.inner.lock().map_err(|_| BridgeError::Operation {
                reason: "HTTP instance lock failed".into(),
            })?;
            let result = inner.stop().map_err(|error| BridgeError::Operation {
                reason: error.to_string(),
            })?;
            Ok(HttpProbeStopResult {
                stdout: result.execution.stdout,
                stderr: result.execution.stderr,
                exit_code: result.execution.exit_code,
                elapsed_ms: result.execution.elapsed_ms,
                forced: result.forced,
                active_execution_tasks: result.active_execution_tasks,
            })
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(BridgeError::Operation {
                reason: "Wasmer runtime is not included".into(),
            })
        }
    }
}

#[uniffi::export]
pub fn start_registry_http_probe(
    data_dir: String,
    observer: Option<Box<dyn ProbeObserver>>,
) -> Result<std::sync::Arc<RegistryHttpProbe>, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        registry_http_probe(&data_dir, observer.as_deref())
            .map(std::sync::Arc::new)
            .map_err(|error| BridgeError::Operation {
                reason: error.to_string(),
            })
    }
    #[cfg(not(feature = "runtime"))]
    {
        let _ = (data_dir, observer);
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

#[cfg(feature = "runtime")]
fn registry_http_probe(
    data_dir: &str,
    observer: Option<&dyn ProbeObserver>,
) -> Result<RegistryHttpProbe, Box<dyn std::error::Error>> {
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
    let graph = installer.pull(
        &ArtifactReference::parse("wasmer.io/wasmer/static-web-server@2.44.0")?,
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
    let inner = wasmdroid_wasmer_local::http_execution::HttpInstance::start(
        &store,
        &graph.root,
        &graph.images,
        &mut |phase| {
            if let Some(observer) = observer {
                observer.on_phase(phase.into());
            }
        },
    )?;
    let info = HttpProbeInfo {
        reference: graph.root.reference.as_str().into(),
        digest: graph.root.digest.as_str().into(),
        installed_images: graph.images.len() as u32,
        url: inner.url().into(),
        document_token: inner.document_token().into(),
    };
    Ok(RegistryHttpProbe {
        info,
        inner: std::sync::Mutex::new(inner),
    })
}

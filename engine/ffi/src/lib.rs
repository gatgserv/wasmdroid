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

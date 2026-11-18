//! Common-owner catalog operations. Trust configuration is an explicit deployment
//! input, never obtained from the untrusted downloaded index or a package recipe.
use crate::{BridgeError, EngineStorage, InstallControl, storage_lock_error};
use std::sync::Arc;
use wasmdroid_engine::{
    catalog_updates::{CatalogUpdateConfig, CatalogUpdates},
    store::Digest,
};

fn config(json: &str) -> Result<CatalogUpdateConfig, BridgeError> {
    if json.len() > 16 * 1024 {
        return Err(BridgeError::Operation {
            reason: "catalog trust configuration exceeds limit".into(),
        });
    }
    let config: CatalogUpdateConfig = serde_json::from_str(json)?;
    config.validate()?;
    Ok(config)
}

#[uniffi::export]
impl EngineStorage {
    /// Off-main-thread only. Releases the storage mutex before any HTTPS work;
    /// the independent session retains the engine's existing ownership lease.
    pub fn refresh_catalog(
        &self,
        config_json: String,
        now_unix_seconds: u64,
        control: Arc<InstallControl>,
    ) -> Result<String, BridgeError> {
        let config = config(&config_json)?;
        let handle = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection_handle();
        let session = handle.connect()?;
        let mut updates = CatalogUpdates::open(&session, config)?;
        let snapshot = updates.refresh(now_unix_seconds, &control.inner)?;
        Ok(serde_json::to_string(&snapshot)?)
    }

    /// Returns JSON null when there is no accepted remote snapshot. Network
    /// failure never substitutes arbitrary downloaded bytes for this snapshot.
    pub fn catalog_snapshot(
        &self,
        config_json: String,
        now_unix_seconds: u64,
    ) -> Result<String, BridgeError> {
        let config = config(&config_json)?;
        let handle = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection_handle();
        let session = handle.connect()?;
        let updates = CatalogUpdates::open(&session, config)?;
        Ok(serde_json::to_string(&updates.snapshot(now_unix_seconds)?)?)
    }

    /// Returns the existing CatalogPackage JSON for the controlled installer,
    /// pinned to the exact sequence and catalog digest seen by the user.
    pub fn catalog_package_for_install(
        &self,
        config_json: String,
        sequence: u64,
        catalog_digest: String,
        package_id: String,
        now_unix_seconds: u64,
    ) -> Result<String, BridgeError> {
        let config = config(&config_json)?;
        let handle = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection_handle();
        let session = handle.connect()?;
        let updates = CatalogUpdates::open(&session, config)?;
        let package = updates.package_for_install(
            sequence,
            &Digest::parse(catalog_digest)?,
            &package_id,
            now_unix_seconds,
        )?;
        Ok(serde_json::to_string(&package)?)
    }
}

//! Read-only fixed-root accounting. The observer owns neither native paths nor
//! a quota policy; scanning retains the existing engine ownership independently.
use crate::{BridgeError, EngineStorage, storage_lock_error};
use std::{sync::atomic::AtomicBool, time::Duration};
use wasmdroid_engine::storage_usage::{ScanBudget, StorageUsage};

#[uniffi::export]
impl EngineStorage {
    /// Sample known logical file categories off the UI thread. Capture only the
    /// trusted directory capability under the facade mutex; release it before
    /// walking any files. Values are observations, not atomic usage or quotas.
    pub fn storage_usage(&self) -> Result<String, BridgeError> {
        let usage = {
            let session = self.session.lock().map_err(|_| storage_lock_error())?;
            StorageUsage::capture(&session)?
        };
        let budget = ScanBudget::new(65_536, 32, Duration::from_secs(2))?;
        let snapshot = usage.snapshot(budget, &AtomicBool::new(false));
        Ok(serde_json::to_string(&snapshot)?)
    }
}

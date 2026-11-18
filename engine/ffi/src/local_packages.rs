use super::{BridgeError, storage_lock_error};
use std::sync::Mutex;

/// A private single-use import capability. Retains engine ownership until the
/// input is committed, aborted or dropped; cancellation uses InstallControl and
/// never needs this mutex, including while native inspection is in progress.
#[derive(uniffi::Object)]
pub struct LocalPackageTransfer {
    pub(crate) inner: Mutex<wasmdroid_engine::local_import::LocalImport>,
}
#[uniffi::export]
impl LocalPackageTransfer {
    pub fn write(&self, bytes: Vec<u8>) -> Result<u64, BridgeError> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| storage_lock_error())?
            .append(&bytes)?)
    }
    /// Repeat after installing missing dependencies. Every new preview replaces
    /// its token; commit always rechecks the installed dependency fingerprints.
    pub fn preview(&self) -> Result<String, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            let mut inner = self.inner.lock().map_err(|_| storage_lock_error())?;
            Ok(serde_json::to_string(inner.preview(
                &wasmdroid_wasmer_local::local_package::LocalInspector,
            )?)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(BridgeError::Operation {
                reason: "local package inspection requires the runtime driver".into(),
            })
        }
    }
    pub fn commit(&self, preview_token: String) -> Result<String, BridgeError> {
        Ok(serde_json::to_string(
            &self
                .inner
                .lock()
                .map_err(|_| storage_lock_error())?
                .commit(&preview_token)?,
        )?)
    }
    pub fn abort(&self) -> Result<(), BridgeError> {
        self.inner.lock().map_err(|_| storage_lock_error())?.abort();
        Ok(())
    }
}

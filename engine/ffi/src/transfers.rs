use super::{BridgeError, storage_lock_error};
use std::sync::Mutex;
#[derive(uniffi::Object)]
pub struct DataTransferControl {
    pub(crate) inner: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
#[uniffi::export]
impl DataTransferControl {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            inner: Default::default(),
        })
    }
    pub fn cancel(&self) {
        self.inner.store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(std::sync::atomic::Ordering::Acquire)
    }
}

#[derive(uniffi::Object)]
pub struct DataWriteTransfer {
    pub(crate) inner: Mutex<wasmdroid_engine::data::DataImport>,
}
#[uniffi::export]
impl DataWriteTransfer {
    pub fn write(&self, bytes: Vec<u8>) -> Result<u64, BridgeError> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| storage_lock_error())?
            .append(&bytes)?)
    }
    pub fn commit(&self) -> Result<u64, BridgeError> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| storage_lock_error())?
            .commit()?)
    }
    pub fn abort(&self) -> Result<(), BridgeError> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| storage_lock_error())?
            .abort()?)
    }
}
#[derive(uniffi::Object)]
pub struct DataReadTransfer {
    pub(crate) inner: Mutex<wasmdroid_engine::data::DataReader>,
}
#[uniffi::export]
impl DataReadTransfer {
    pub fn size_bytes(&self) -> Result<Option<u64>, BridgeError> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| storage_lock_error())?
            .size_bytes)
    }
    pub fn read(&self, max_bytes: u32) -> Result<Vec<u8>, BridgeError> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| storage_lock_error())?
            .read(max_bytes)?)
    }
}
#[derive(uniffi::Object)]
pub struct BackupRestoreTransfer {
    pub(crate) inner: Mutex<wasmdroid_engine::backup::RestoreInput>,
}
#[uniffi::export]
impl BackupRestoreTransfer {
    pub fn write(&self, bytes: Vec<u8>) -> Result<u64, BridgeError> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| storage_lock_error())?
            .append(&bytes)?)
    }
    pub fn commit(&self, available_bytes: u64) -> Result<String, BridgeError> {
        let mut inner = self.inner.lock().map_err(|_| storage_lock_error())?;
        inner.limit_unpacked(available_bytes);
        Ok(serde_json::to_string(&inner.commit()?)?)
    }
    pub fn abort(&self) -> Result<(), BridgeError> {
        self.inner.lock().map_err(|_| storage_lock_error())?.abort();
        Ok(())
    }
}

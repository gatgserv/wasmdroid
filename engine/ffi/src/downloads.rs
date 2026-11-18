use std::sync::Arc;
use wasmdroid_engine::operation::{OperationControl, OperationState};

#[derive(uniffi::Object)]
pub struct InstallControl {
    pub(crate) inner: Arc<OperationControl>,
}
#[derive(uniffi::Enum)]
pub enum InstallStatus {
    Active,
    Cancelled,
    Publishing,
    Committed,
    Failed,
}
#[uniffi::export]
impl InstallControl {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(OperationControl::default()),
        })
    }
    /// False means commit/termination already won. Do not show a fake cancel.
    pub fn cancel(&self) -> bool {
        self.inner.cancel()
    }
    pub fn status(&self) -> InstallStatus {
        match self.inner.state() {
            OperationState::Active => InstallStatus::Active,
            OperationState::Cancelled => InstallStatus::Cancelled,
            OperationState::Publishing => InstallStatus::Publishing,
            OperationState::Committed => InstallStatus::Committed,
            OperationState::Failed => InstallStatus::Failed,
        }
    }
}

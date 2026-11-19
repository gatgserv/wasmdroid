//! A cancellation request must not race an already accepted metadata commit.
use crate::{EngineError, Result};
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Active,
    Cancelled,
    Publishing,
    Committed,
    Failed,
}
#[derive(Debug, Default)]
pub struct OperationControl {
    state: AtomicU8,
    claimed: AtomicBool,
    wake: tokio::sync::Notify,
}
impl OperationControl {
    pub fn state(&self) -> OperationState {
        match self.state.load(Ordering::Acquire) {
            0 => OperationState::Active,
            1 => OperationState::Cancelled,
            2 => OperationState::Publishing,
            3 => OperationState::Committed,
            _ => OperationState::Failed,
        }
    }
    /// False means publication/termination has already won; never undo it.
    pub fn cancel(&self) -> bool {
        if self
            .state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.wake.notify_waiters();
            true
        } else {
            self.state() == OperationState::Cancelled
        }
    }
    pub fn check(&self) -> Result<()> {
        if self.state() == OperationState::Cancelled {
            Err(EngineError::Cancelled)
        } else {
            Ok(())
        }
    }
    pub async fn cancelled(&self) {
        loop {
            let wait = self.wake.notified();
            tokio::pin!(wait);
            // Register before testing the atomic to avoid losing notify_waiters.
            wait.as_mut().enable();
            if self.state() == OperationState::Cancelled {
                return;
            }
            wait.await;
        }
    }
    pub fn claim(self: &Arc<Self>) -> Result<OperationGuard> {
        self.check()?;
        if self.state() != OperationState::Active || self.claimed.swap(true, Ordering::AcqRel) {
            return Err(EngineError::Conflict(
                "operation control was already used".into(),
            ));
        }
        Ok(OperationGuard(self.clone()))
    }
    pub fn begin_publication(&self) -> Result<()> {
        match self
            .state
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(1) => Err(EngineError::Cancelled),
            _ => Err(EngineError::Conflict("operation cannot publish".into())),
        }
    }
    pub fn committed(&self) {
        let _ = self
            .state
            .compare_exchange(2, 3, Ordering::AcqRel, Ordering::Acquire);
    }
    fn failed(&self) {
        for active in [0, 2] {
            let _ = self
                .state
                .compare_exchange(active, 4, Ordering::AcqRel, Ordering::Acquire);
        }
    }
}
pub struct OperationGuard(Arc<OperationControl>);
impl OperationGuard {
    pub fn control(&self) -> &OperationControl {
        &self.0
    }
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.0.failed();
    }
}

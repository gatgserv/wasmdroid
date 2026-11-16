//! Blocking driver contract. Supervisors must dispatch calls off the UI thread.
use crate::{
    Result,
    api::{ArtifactKind, InstanceSpec},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DriverCapabilities {
    pub id: String,
    pub artifact_kinds: Vec<ArtifactKind>,
    pub pause: bool,
    pub tty: bool,
    pub memory_limit: bool,
    pub cpu_shares: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeStats {
    pub memory_bytes: Option<u64>,
    pub cpu_time_ns: Option<u64>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopSignal {
    Terminate,
    Interrupt,
}

pub trait RuntimeDriver: Send + Sync {
    fn capabilities(&self) -> DriverCapabilities;
    /// Validate all requested features before any side effects. Reject unsupported fields.
    fn create(&self, instance_id: &str, spec: &InstanceSpec) -> Result<()>;
    fn start(&self, instance_id: &str) -> Result<()>;
    fn stop(&self, instance_id: &str, signal: StopSignal, timeout_seconds: u64) -> Result<()>;
    fn kill(&self, instance_id: &str) -> Result<()>;
    fn wait(&self, instance_id: &str) -> Result<i32>;
    fn attach_stdio(&self, instance_id: &str, input: &[u8]) -> Result<()>;
    fn stats(&self, instance_id: &str) -> Result<RuntimeStats>;
    fn remove(&self, instance_id: &str) -> Result<()>;
}

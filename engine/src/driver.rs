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
    /// Whole owned worker, including host VM, compiler, allocator and descendants
    /// implemented as guest tasks. Never describe this as guest linear memory.
    pub scope: StatsScope,
    /// Current resident set, not virtual address space or the process peak.
    pub memory_bytes: Option<u64>,
    /// Cumulative user + kernel CPU consumed by all threads since process birth.
    pub cpu_time_ns: Option<u64>,
    /// CLOCK_MONOTONIC sample timestamp. Compare only within the same generation.
    pub sampled_at_monotonic_ns: Option<u64>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatsScope {
    WorkerProcess,
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
    /// Request a signal and arrange platform termination after the grace period.
    /// A successful return acknowledges the request, not resource reclamation.
    fn stop(&self, instance_id: &str, signal: StopSignal, timeout_seconds: u64) -> Result<()>;
    /// Request termination through the trusted platform endpoint, never a cached PID.
    fn kill(&self, instance_id: &str) -> Result<()>;
    /// Wait for guest tasks and their verified exit result. The platform must also
    /// observe worker death before releasing the service group or its resources.
    /// Forced platform death has no fabricated guest exit code.
    fn wait(&self, instance_id: &str) -> Result<i32>;
    fn attach_stdio(&self, instance_id: &str, input: &[u8]) -> Result<()>;
    fn stats(&self, instance_id: &str) -> Result<RuntimeStats>;
    /// Release completed native execution resources. Durable registry removal and
    /// volumes belong to the common coordinator, after observed worker death.
    fn remove(&self, instance_id: &str) -> Result<()>;
}

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{EngineError, Result};

pub const API_VERSION: u32 = 1;

/// Includes the registry host; mutable tags are resolved to a digest before install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ArtifactReference(String);

impl ArtifactReference {
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let Some((host, path)) = value.split_once('/') else {
            return Err(EngineError::Invalid(
                "reference must include registry host".into(),
            ));
        };
        if !host.contains('.')
            || host.split('.').any(|label| {
                label.is_empty()
                    || !label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
            || host.contains(':')
            || path.is_empty()
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"./_@:+-".contains(&c))
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(EngineError::Invalid("invalid artifact reference".into()));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ArtifactReference {
    type Error = EngineError;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(value)
    }
}
impl From<ArtifactReference> for String {
    fn from(value: ArtifactReference) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Webc,
    Wasm,
    OciImage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub reference: ArtifactReference,
    pub digest: crate::store::Digest,
    pub kind: ArtifactKind,
    pub driver: String,
    pub platform: String,
    pub commands: Vec<String>,
    pub size_bytes: u64,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub uri: String,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            id: "local".into(),
            name: "This phone".into(),
            uri: "local".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    Created,
    Running,
    Paused,
    Restarting,
    Exited,
    Dead,
}

impl InstanceState {
    pub fn transition(self, next: Self) -> Result<Self> {
        use InstanceState::*;
        if matches!(
            (self, next),
            (Created, Running | Dead)
                | (Running, Paused | Restarting | Exited | Dead)
                | (Paused, Running | Exited | Dead)
                | (Restarting, Running | Exited | Dead)
                | (Exited, Running | Restarting | Dead)
        ) {
            Ok(next)
        } else {
            Err(EngineError::InvalidTransition(format!(
                "{self:?} -> {next:?}"
            )))
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    #[default]
    Loopback,
    Lan,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    #[default]
    No,
    OnFailure,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortBinding {
    /// Zero requests automatic host allocation by the network manager.
    pub host_port: u16,
    pub instance_port: u16,
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "source", rename_all = "snake_case")]
pub enum MountSource {
    Volume(String),
    HostPath(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    pub source: MountSource,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub memory_max_bytes: Option<u64>,
    pub cpu_shares: Option<u32>,
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceSpec {
    pub artifact_ref: ArtifactReference,
    #[serde(default = "local_endpoint")]
    pub endpoint_id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub working_dir: Option<String>,
    #[serde(default)]
    pub mounts: Vec<Mount>,
    #[serde(default)]
    pub ports: Vec<PortBinding>,
    #[serde(default)]
    pub network: NetworkMode,
    #[serde(default)]
    pub internet_access: bool,
    #[serde(default)]
    pub resources: ResourceLimits,
    #[serde(default)]
    pub restart_policy: RestartPolicy,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub tty: bool,
    #[serde(default)]
    pub stdin_open: bool,
}

fn local_endpoint() -> String {
    "local".into()
}

impl InstanceSpec {
    pub fn validate(&self) -> Result<()> {
        // Revalidate deserialized newtypes at the trust boundary.
        ArtifactReference::parse(self.artifact_ref.as_str())?;
        if self.endpoint_id.is_empty() || self.command.trim().is_empty() {
            return Err(EngineError::Invalid(
                "endpoint and command are required".into(),
            ));
        }
        for mount in &self.mounts {
            if !mount.target.starts_with('/') || mount.target.split('/').any(|p| p == "..") {
                return Err(EngineError::Invalid(
                    "mount target must be absolute without traversal".into(),
                ));
            }
            if let MountSource::Volume(name) = &mount.source {
                validate_name(name)?;
            }
        }
        if self.ports.iter().any(|p| p.instance_port == 0) {
            return Err(EngineError::Invalid("instance port must be nonzero".into()));
        }
        if self.resources.memory_max_bytes == Some(0)
            || self.resources.cpu_shares == Some(0)
            || self.resources.timeout_seconds == Some(0)
        {
            return Err(EngineError::Invalid(
                "resource limits must be positive".into(),
            ));
        }
        Ok(())
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return Err(EngineError::Invalid(
            "name must contain 1..128 ASCII letters, digits, _ or -".into(),
        ));
    }
    Ok(())
}

//! Narrow normal-app permission edits. No caller specification/profile assignment
//! is accepted; identity/version and stopped/group constraints are native gates.
use super::*;
use crate::api::{InstanceState, NetworkMode};
use serde::Serialize;

#[derive(Serialize)]
pub struct NetworkPermissionSource {
    pub id: String,
    pub identity: String,
    pub version: u64,
    pub name: String,
    pub state: InstanceState,
    pub managed: bool,
    pub network_enabled: bool,
    pub internet: bool,
    pub lan: bool,
}
impl std::fmt::Debug for NetworkPermissionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NetworkPermissionSource(private)")
    }
}
impl StateDb {
    pub fn network_permission_source(&self, id: &str) -> Result<NetworkPermissionSource> {
        // Keep the name/version/private birth identity from one actual snapshot.
        let transaction = self.connection.unchecked_transaction()?;
        let record = self.instance(id)?;
        let source = NetworkPermissionSource {
            id: id.into(),
            identity: self.browser_identity(id)?,
            version: record.version,
            name: record
                .spec
                .labels
                .get("dev.wasmdroid.ui.name")
                .filter(|name| name.len() <= 160 && !name.chars().any(char::is_control))
                .cloned()
                .unwrap_or_else(|| record.spec.command.chars().take(160).collect()),
            state: record.state,
            managed: self.stack_for_instance(id)?.is_some(),
            network_enabled: record.spec.network == NetworkMode::Loopback,
            internet: record.spec.internet_access,
            lan: record.spec.lan_access,
        };
        transaction.commit()?;
        Ok(source)
    }
    pub fn set_network_permissions(
        &self,
        id: &str,
        identity: &str,
        expected_version: u64,
        internet: bool,
        lan: bool,
    ) -> Result<InstanceRecord> {
        if identity.is_empty() || identity.len() > 128 {
            return Err(EngineError::Conflict("application identity changed".into()));
        }
        let mut spec = self.instance(id)?.spec;
        if (internet || lan) && spec.network != NetworkMode::Loopback {
            return Err(EngineError::Unsupported(
                "application networking is disabled".into(),
            ));
        }
        spec.internet_access = internet;
        spec.lan_access = lan;
        // Identity check occurs inside the same transaction as version/state,
        // artifact/data validation, authority cancellation and actual spec write.
        self.configure_instance_checked(id, expected_version, &spec, Some(identity))
    }
}

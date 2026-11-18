//! Opaque actual native effective permissions. Raw managed specs remain neutral.
//! This proof is not serde, a packet boolean, runtime ownership or a public grant.
use super::group_network_permissions::GrantSnapshot;
use super::*;
use crate::api::{InstanceSpec, InstanceState, NetworkMode};
use std::path::PathBuf;

pub struct EffectiveWorkerSpec {
    database: PathBuf,
    id: String,
    profile: String,
    raw: InstanceSpec,
    specification: String,
    version: u64,
    state: InstanceState,
    authority: RunAuthority,
    grant: Option<GrantSnapshot>,
    internet: bool,
    lan: bool,
}
impl EffectiveWorkerSpec {
    pub fn raw_spec(&self) -> &InstanceSpec {
        &self.raw
    }
    pub fn internet(&self) -> bool {
        self.internet
    }
    pub fn lan(&self) -> bool {
        self.lan
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    fn semantic_current(&self, state: &StateDb) -> Result<Self> {
        let current = state.read_effective_worker(&self.id)?;
        if current.database != self.database
            || current.profile != self.profile
            || current.specification != self.specification
            || current.grant != self.grant
            || current.internet != self.internet
            || current.lan != self.lan
        {
            return Err(EngineError::Conflict(
                "effective worker authority changed".into(),
            ));
        }
        if (current.internet || current.lan) && current.raw.network != NetworkMode::Loopback {
            return Err(EngineError::Unsupported(
                "effective outbound networking is disabled".into(),
            ));
        }
        Ok(current)
    }
    pub fn validate_starting(&self, state: &StateDb, generation: &str) -> Result<()> {
        let tx = state.connection.unchecked_transaction()?;
        let current = self.semantic_current(state)?;
        if !matches!(self.state, InstanceState::Created | InstanceState::Exited)
            || current.state != self.state
            || current.version != self.version
            || !self.authority.desired_run
            || self.authority.outcome != RunOutcome::Starting
            || self.authority.generation.as_deref() != Some(generation)
            || serde_json::to_string(&self.authority)? != serde_json::to_string(&current.authority)?
        {
            return Err(EngineError::Conflict(
                "effective starting authority changed".into(),
            ));
        }
        tx.commit()?;
        Ok(())
    }
    pub fn validate_running(&self, state: &StateDb, generation: &str) -> Result<()> {
        let tx = state.connection.unchecked_transaction()?;
        self.check_running_snapshot(state, generation)?;
        tx.commit()?;
        Ok(())
    }
    /// Serialize durable Stop/grant edits with a final LOCAL namespace CAS.
    /// Publish must not await, call a foreign/provider callback, or start another
    /// transaction. If commit fails after local publication, the caller must
    /// revoke that publication: SQL cannot undo a namespace side effect.
    pub fn with_running<T>(
        &self,
        state: &StateDb,
        generation: &str,
        publish: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let tx = rusqlite::Transaction::new_unchecked(
            &state.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        self.check_running_snapshot(state, generation)?;
        let result = publish()?;
        tx.commit()?;
        Ok(result)
    }
    fn check_running_snapshot(&self, state: &StateDb, generation: &str) -> Result<()> {
        let current = self.semantic_current(state)?;
        let ready_delta = matches!(self.state, InstanceState::Created | InstanceState::Exited)
            && self.authority.outcome == RunOutcome::Starting
            && self.version.checked_add(1) == Some(current.version)
            && self.authority.revision.checked_add(1) == Some(current.authority.revision);
        let captured_running = self.state == InstanceState::Running
            && self.authority.outcome == RunOutcome::Running
            && self.version == current.version
            && self.authority.revision == current.authority.revision;
        if !(ready_delta || captured_running)
            || current.state != InstanceState::Running
            || current.authority.outcome != RunOutcome::Running
            || !self.authority.desired_run
            || !current.authority.desired_run
            || self.authority.generation.as_deref() != Some(generation)
            || current.authority.generation.as_deref() != Some(generation)
            || self.authority.boot_epoch != current.authority.boot_epoch
        {
            return Err(EngineError::Conflict(
                "effective running authority changed".into(),
            ));
        }
        Ok(())
    }
}
impl StateDb {
    pub(super) fn read_effective_worker(&self, id: &str) -> Result<EffectiveWorkerSpec> {
        let instance = self.instance(id)?;
        if instance.spec.endpoint_id != "local" || instance.driver != "wasmer-local" {
            return Err(EngineError::Unsupported(
                "effective worker requires the local Wasmer driver".into(),
            ));
        }
        let grant = self.managed_grant(id, &instance.spec)?;
        let (internet, lan) = grant
            .as_ref()
            .map(|grant| (grant.internet, grant.lan))
            .unwrap_or((instance.spec.internet_access, instance.spec.lan_access));
        Ok(EffectiveWorkerSpec {
            database: self.permission_database()?,
            id: id.into(),
            profile: self.browser_identity(id)?,
            specification: serde_json::to_string(&instance.spec)?,
            raw: instance.spec,
            version: instance.version,
            state: instance.state,
            authority: self.run_authority(id)?,
            grant,
            internet,
            lan,
        })
    }
    /// Useful for an owner-only launch hint before admission as well as for a
    /// sealed post-admission Starting proof. Getters alone authorize no runtime.
    pub fn effective_worker_spec(&self, id: &str) -> Result<EffectiveWorkerSpec> {
        let tx = self.connection.unchecked_transaction()?;
        let source = self.read_effective_worker(id)?;
        tx.commit()?;
        Ok(source)
    }
}

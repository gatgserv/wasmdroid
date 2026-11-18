//! Metadata-only editing for the next execution. The common coordinator must
//! additionally fence live/starting/stopping workers and data operations.
use super::StateDb;
use crate::{
    EngineError, Result,
    api::{InstanceSpec, InstanceState, MountSource},
    state::lifecycle::{event, kind_name},
};
use rusqlite::params;

impl StateDb {
    /// Preserve identity, runtime history and datasets. New mounted volumes must
    /// already exist; removing a mount never deletes its underlying volume.
    pub fn configure_instance(
        &self,
        id: &str,
        expected_version: u64,
        spec: &InstanceSpec,
    ) -> Result<super::InstanceRecord> {
        self.configure_instance_checked(id, expected_version, spec, None)
    }

    /// Ordinary platform edits may retain permissions already approved for this
    /// instance. Permission changes belong to the separate network access flow.
    pub fn configure_instance_preserving_network_permissions(
        &self,
        id: &str,
        expected_version: u64,
        spec: &InstanceSpec,
    ) -> Result<super::InstanceRecord> {
        self.configure_instance_with_fences(id, expected_version, spec, None, true)
    }

    pub(super) fn configure_instance_checked(
        &self,
        id: &str,
        expected_version: u64,
        spec: &InstanceSpec,
        expected_browser_identity: Option<&str>,
    ) -> Result<super::InstanceRecord> {
        self.configure_instance_with_fences(
            id,
            expected_version,
            spec,
            expected_browser_identity,
            false,
        )
    }

    fn configure_instance_with_fences(
        &self,
        id: &str,
        expected_version: u64,
        spec: &InstanceSpec,
        expected_browser_identity: Option<&str>,
        preserve_network_permissions: bool,
    ) -> Result<super::InstanceRecord> {
        spec.validate()?;
        let serialized = serde_json::to_string(spec)?;
        if serialized.len() > 128 * 1024 {
            return Err(EngineError::Invalid(
                "service specification exceeds limit".into(),
            ));
        }
        let transaction = self.connection.unchecked_transaction()?;
        let previous = self.instance(id)?;
        if previous.version != expected_version {
            return Err(EngineError::Conflict("stale instance revision".into()));
        }
        if let Some(identity) = expected_browser_identity
            && self.browser_identity(id)? != identity
        {
            return Err(EngineError::Conflict("application identity changed".into()));
        }
        if !matches!(
            previous.state,
            InstanceState::Created | InstanceState::Exited | InstanceState::Dead
        ) {
            return Err(EngineError::Conflict(
                "stop the instance before configuring it".into(),
            ));
        }
        if self.stack_for_instance(id)?.is_some()
            || [&previous.spec.labels, &spec.labels].iter().any(|labels| {
                labels.contains_key(crate::stacks::STACK_LABEL)
                    || labels.contains_key(crate::stacks::SERVICE_LABEL)
            })
        {
            return Err(EngineError::Unsupported(
                "configure managed services through their stack definition".into(),
            ));
        }
        if spec.artifact_ref != previous.spec.artifact_ref
            || spec.endpoint_id != previous.spec.endpoint_id
        {
            return Err(EngineError::Invalid(
                "configuration cannot replace package or endpoint identity".into(),
            ));
        }
        if preserve_network_permissions
            && (spec.internet_access != previous.spec.internet_access
                || spec.lan_access != previous.spec.lan_access)
        {
            return Err(EngineError::Invalid(
                "change outbound permissions through network access settings".into(),
            ));
        }
        let artifact = self.artifact(&spec.artifact_ref)?;
        if artifact.kind != previous.kind
            || artifact.driver != previous.driver
            || !artifact.commands.contains(&spec.command)
        {
            return Err(EngineError::Conflict(
                "installed package command or driver changed".into(),
            ));
        }
        let pending = self.volume_deletions()?;
        for mount in &spec.mounts {
            if let MountSource::Volume(name) = &mount.source {
                self.volume(name)?;
                if pending.contains(name) {
                    return Err(EngineError::Conflict(
                        "selected volume removal is pending".into(),
                    ));
                }
            }
        }
        self.clear_run_intent_for_edit(&transaction, id)?;
        // A duplicate spec save does not manufacture an instance revision or
        // configuration event; clearing run authority is a separate operation.
        if serialized == serde_json::to_string(&previous.spec)? {
            transaction.commit()?;
            return Ok(previous);
        }
        let version = expected_version
            .checked_add(1)
            .and_then(|v| i64::try_from(v).ok())
            .ok_or_else(|| EngineError::Invalid("instance revision overflow".into()))?;
        let expected = i64::try_from(expected_version)
            .map_err(|_| EngineError::Invalid("instance revision overflow".into()))?;
        let updated = transaction.execute(
            "UPDATE instances SET spec_json=?1,version=?2 WHERE id=?3 AND version=?4",
            params![serialized, version, id, expected],
        )?;
        if updated != 1 {
            return Err(EngineError::Conflict("stale instance revision".into()));
        }
        transaction.execute("DELETE FROM instance_volumes WHERE instance_id=?1", [id])?;
        for mount in &spec.mounts {
            if let MountSource::Volume(name) = &mount.source {
                transaction.execute("INSERT INTO instance_volumes(instance_id,volume_name,target,read_only) VALUES (?1,?2,?3,?4)",
                    params![id,name,mount.target,mount.read_only])?;
            }
        }
        // No args, environment, labels, guest paths or volume names enter events.
        event(
            &transaction,
            Some(id),
            kind_name(previous.kind),
            &previous.driver,
            serde_json::json!({"type":"instance_configured","version":version}),
        )?;
        transaction.commit()?;
        self.instance(id)
    }
}

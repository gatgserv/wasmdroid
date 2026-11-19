//! Explicit manual-only managed startup admission. This never enables managed
//! recovery, boot or automatic restart; terminal outcomes withdraw desired run.
use super::group_network_permissions::next_revision;
use super::lifecycle::{event, kind_name};
use super::*;
use crate::api::InstanceState;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedRunStart {
    pub instance_id: String,
    pub version: u64,
    pub generation: String,
}
impl StateDb {
    /// Owner-only acknowledgement that this admitted managed job never reached
    /// Binder binding. This is not guest exit/death evidence and cannot withdraw
    /// a Running authority. The common owner must prove !bindRequested.
    pub fn withdraw_unbound_managed_run(
        &self,
        id: &str,
        version: u64,
        generation: &str,
    ) -> Result<RunAuthority> {
        validate_name(generation)?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let instance = self.instance(id)?;
        let mut authority = self.run_authority(id)?;
        if self.stack_for_instance(id)?.is_none()
            || instance.version != version
            || !matches!(
                instance.state,
                InstanceState::Created | InstanceState::Exited
            )
            || authority.generation.as_deref() != Some(generation)
        {
            return Err(EngineError::Conflict(
                "unbound managed admission changed".into(),
            ));
        }
        if !authority.desired_run && authority.outcome == RunOutcome::Stopped {
            tx.commit()?;
            return Ok(authority);
        }
        if authority.outcome != RunOutcome::Starting {
            return Err(EngineError::Conflict(
                "managed admission already reached runtime".into(),
            ));
        }
        authority.desired_run = false;
        authority.outcome = RunOutcome::Stopped;
        authority.revision = next_revision(authority.revision)?;
        self.write_authority(&tx, &authority)?;
        event(
            &tx,
            Some(id),
            kind_name(instance.kind),
            &instance.driver,
            serde_json::json!({"type":"managed_manual_admission_withdrawn","revision":authority.revision}),
        )?;
        tx.commit()?;
        Ok(authority)
    }

    pub(crate) fn require_group_network_idle(&self, id: &str) -> Result<()> {
        let stack = self.stack(id)?;
        crate::stacks::require_stopped(&stack)?;
        for member in stack.instances {
            if matches!(
                self.run_authority(&member.id)?.outcome,
                RunOutcome::Starting | RunOutcome::Running
            ) {
                return Err(EngineError::Conflict(
                    "managed run authority still owns a listed service".into(),
                ));
            }
        }
        Ok(())
    }
    /// Common owner must reserve the actual requested jobs first. Native review
    /// binds ALL current members/authorities; starts is only the needed subset.
    pub fn begin_managed_runs(
        &self,
        review: &GroupNetworkReview,
        starts: &[ManagedRunStart],
    ) -> Result<Vec<RunAuthority>> {
        if starts.is_empty() || starts.len() > 8 {
            return Err(EngineError::Invalid(
                "managed run batch requires 1..8 members".into(),
            ));
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let current = review.validate(self)?;
        let allowed: BTreeSet<_> = current.member_ids().collect();
        let mut ids = BTreeSet::new();
        let mut generations = BTreeSet::new();
        let mut pending = Vec::new();
        for start in starts {
            validate_name(&start.generation)?;
            if start.generation.len() > 128
                || !allowed.contains(start.instance_id.as_str())
                || !ids.insert(&start.instance_id)
                || !generations.insert(&start.generation)
            {
                return Err(EngineError::Conflict("managed run batch changed".into()));
            }
            let instance = self.instance(&start.instance_id)?;
            let mut authority = self.run_authority(&start.instance_id)?;
            if instance.version != start.version
                || !matches!(
                    instance.state,
                    InstanceState::Created | InstanceState::Exited
                )
                || matches!(
                    authority.outcome,
                    RunOutcome::Starting | RunOutcome::Running
                )
                || instance.spec.endpoint_id != "local"
                || instance.driver != "wasmer-local"
            {
                return Err(EngineError::Conflict(
                    "managed member is stale or active".into(),
                ));
            }
            // Read actual native effective authority; a raw positive managed
            // specification is never a substitute for operator consent.
            self.read_effective_worker(&start.instance_id)?;
            authority.revision = next_revision(authority.revision)?;
            authority.desired_run = true;
            authority.generation = Some(start.generation.clone());
            authority.boot_epoch = None;
            authority.outcome = RunOutcome::Starting;
            pending.push((instance, authority));
        }
        let mut result = Vec::new();
        for (instance, authority) in pending {
            self.write_authority(&tx, &authority)?;
            event(
                &tx,
                Some(&instance.id),
                kind_name(instance.kind),
                &instance.driver,
                serde_json::json!({"type":"managed_manual_run_admitted","revision":authority.revision}),
            )?;
            result.push(authority);
        }
        tx.commit()?;
        Ok(result)
    }

    /// Existing actual-generation READY/Stop/terminal APIs are shared. A common
    /// owner may stop one or every member with stop_run_intent/stop_all_run_intents;
    /// actual terminal metadata uses finish_authorized_run after observed death.
    pub(super) fn close_managed_desire(
        &self,
        authority: &mut RunAuthority,
        next: InstanceState,
    ) -> Result<()> {
        if matches!(next, InstanceState::Exited | InstanceState::Dead)
            && self.stack_for_instance(&authority.instance_id)?.is_some()
        {
            authority.desired_run = false;
        }
        Ok(())
    }
}

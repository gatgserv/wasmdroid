//! Durable standalone run authority. Legacy absence is opt-out. No preference,
//! label, sticky service or unverified process identifier grants recovery.
use super::lifecycle::{event, kind_name};
use super::*;
use crate::api::{InstanceState, RestartPolicy};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Idle,
    Starting,
    Running,
    Succeeded,
    Failed,
    Interrupted,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAuthority {
    pub instance_id: String,
    pub revision: u64,
    pub desired_run: bool,
    pub generation: Option<String>,
    pub boot_epoch: Option<u64>,
    pub outcome: RunOutcome,
}

#[derive(Serialize)]
pub struct AutomaticStartSource {
    id: String,
    identity: String,
    version: u64,
    name: String,
    editable: bool,
    start_on_boot: bool,
    restart_policy: RestartPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStartSource {
    Manual,
    Boot,
    Restart,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCandidate {
    pub id: String,
    pub version: u64,
    pub state: InstanceState,
    pub policy: RestartPolicy,
    pub authority: RunAuthority,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPage {
    pub items: Vec<RecoveryCandidate>,
    pub next_cursor: Option<String>,
    pub scanned: u32,
    pub skipped: u32,
}

fn sql(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| EngineError::Invalid("automation revision exceeds storage range".into()))
}
fn next(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| EngineError::Invalid("automation revision overflow".into()))
}
fn name(outcome: RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Idle => "idle",
        RunOutcome::Starting => "starting",
        RunOutcome::Running => "running",
        RunOutcome::Succeeded => "succeeded",
        RunOutcome::Failed => "failed",
        RunOutcome::Interrupted => "interrupted",
        RunOutcome::Stopped => "stopped",
    }
}
fn active(outcome: RunOutcome) -> bool {
    matches!(outcome, RunOutcome::Starting | RunOutcome::Running)
}

impl StateDb {
    pub fn automatic_start_source(&self, id: &str) -> Result<AutomaticStartSource> {
        let tx = self.connection.unchecked_transaction()?;
        let record = self.instance(id)?;
        if self.stack_for_instance(id)?.is_some()
            || record.spec.endpoint_id != "local"
            || record.driver != "wasmer-local"
        {
            return Err(EngineError::Unsupported(
                "automatic startup requires an individual local application".into(),
            ));
        }
        let name = record
            .spec
            .labels
            .get("dev.wasmdroid.ui.name")
            .map(String::as_str)
            .unwrap_or(&record.spec.command);
        let source = AutomaticStartSource {
            id: record.id,
            identity: self.browser_identity(id)?,
            version: record.version,
            name: name
                .chars()
                .filter(|ch| !ch.is_control())
                .take(160)
                .collect(),
            editable: matches!(
                record.state,
                InstanceState::Created | InstanceState::Exited | InstanceState::Dead
            ) && !active(self.run_authority(id)?.outcome),
            start_on_boot: record.spec.start_on_boot,
            restart_policy: record.spec.restart_policy,
        };
        tx.commit()?;
        Ok(source)
    }

    pub fn set_automatic_start(
        &self,
        id: &str,
        identity: &str,
        version: u64,
        start_on_boot: bool,
        restart_policy: RestartPolicy,
    ) -> Result<u64> {
        let record = self.instance(id)?;
        if record.spec.endpoint_id != "local" || record.driver != "wasmer-local" {
            return Err(EngineError::Unsupported(
                "automatic startup requires an individual local application".into(),
            ));
        }
        let mut spec = record.spec;
        spec.start_on_boot = start_on_boot;
        spec.restart_policy = restart_policy;
        let updated = self.configure_instance_checked(id, version, &spec, Some(identity))?;
        Ok(updated.version)
    }

    pub fn run_authority(&self, id: &str) -> Result<RunAuthority> {
        self.instance(id)?;
        let raw=self.connection.query_row("SELECT revision,desired_run,generation,boot_epoch,outcome FROM instance_automation WHERE instance_id=?1",[id],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,bool>(1)?,row.get::<_,Option<String>>(2)?,row.get::<_,Option<i64>>(3)?,row.get::<_,String>(4)?))).optional()?;
        let Some((revision, desired_run, generation, boot_epoch, outcome)) = raw else {
            return Ok(RunAuthority {
                instance_id: id.into(),
                revision: 0,
                desired_run: false,
                generation: None,
                boot_epoch: None,
                outcome: RunOutcome::Idle,
            });
        };
        Ok(RunAuthority {
            instance_id: id.into(),
            revision: revision as u64,
            desired_run,
            generation,
            boot_epoch: boot_epoch.map(|value| value as u64),
            outcome: serde_json::from_value(outcome.into())?,
        })
    }
    pub(super) fn write_authority(&self, tx: &Connection, authority: &RunAuthority) -> Result<()> {
        tx.execute("INSERT INTO instance_automation(instance_id,revision,desired_run,generation,boot_epoch,outcome) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(instance_id) DO UPDATE SET revision=excluded.revision,desired_run=excluded.desired_run,generation=excluded.generation,boot_epoch=excluded.boot_epoch,outcome=excluded.outcome",
            params![authority.instance_id,sql(authority.revision)?,authority.desired_run,authority.generation,authority.boot_epoch.map(sql).transpose()?,name(authority.outcome)])?;
        Ok(())
    }
    /// Commit before any user Stop signal, configuration/data mutation or
    /// removal. An active generation stays active until real terminal evidence;
    /// a crash during this window can never recover a durable desired=false.
    pub fn stop_run_intent(
        &self,
        id: &str,
        instance_version: u64,
        revision: u64,
        generation: Option<&str>,
    ) -> Result<RunAuthority> {
        let tx = self.connection.unchecked_transaction()?;
        let instance = self.instance(id)?;
        let mut authority = self.run_authority(id)?;
        if instance.version != instance_version
            || authority.revision != revision
            || authority.generation.as_deref() != generation
        {
            return Err(EngineError::Conflict("stale run authority".into()));
        }
        if !authority.desired_run {
            tx.commit()?;
            return Ok(authority);
        }
        authority.desired_run = false;
        authority.revision = next(authority.revision)?;
        if !active(authority.outcome) {
            authority.outcome = RunOutcome::Stopped
        }
        self.write_authority(&tx, &authority)?;
        event(
            &tx,
            Some(id),
            kind_name(instance.kind),
            &instance.driver,
            serde_json::json!({"type":"instance_run_intent","desired_run":false,"revision":authority.revision}),
        )?;
        tx.commit()?;
        Ok(authority)
    }
    /// One transaction precedes every StopAll signal. A process death between
    /// individual worker controls cannot leave another desired=true record.
    pub fn stop_all_run_intents(&self) -> Result<u64> {
        let tx = self.connection.unchecked_transaction()?;
        let mut query = tx.prepare(
            "SELECT instance_id FROM instance_automation WHERE desired_run=1 ORDER BY instance_id",
        )?;
        let ids = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(query);
        for id in &ids {
            let instance = self.instance(id)?;
            let mut authority = self.run_authority(id)?;
            authority.revision = next(authority.revision)?;
            authority.desired_run = false;
            if !active(authority.outcome) {
                authority.outcome = RunOutcome::Stopped
            }
            self.write_authority(&tx, &authority)?;
            event(
                &tx,
                Some(id),
                kind_name(instance.kind),
                &instance.driver,
                serde_json::json!({"type":"instance_run_intent","desired_run":false,"revision":authority.revision}),
            )?;
        }
        event(
            &tx,
            None,
            "application",
            "local-owner",
            serde_json::json!({"type":"application_stop_all"}),
        )?;
        tx.commit()?;
        Ok(ids.len() as u64)
    }
    fn eligible_authority(
        &self,
        instance: &super::InstanceRecord,
        authority: &RunAuthority,
        boot_epoch: Option<u64>,
        cold: bool,
    ) -> Result<bool> {
        Ok(authority.desired_run
            && authority.generation.is_some()
            && (!cold || authority.boot_epoch.is_some())
            && authority.boot_epoch == boot_epoch
            && instance.spec.endpoint_id == "local"
            && instance.driver == "wasmer-local"
            && self.stack_for_instance(&instance.id)?.is_none()
            && matches!(
                instance.state,
                InstanceState::Created | InstanceState::Exited | InstanceState::Dead
            )
            && match instance.spec.restart_policy {
                RestartPolicy::No => false,
                RestartPolicy::Always => matches!(
                    authority.outcome,
                    RunOutcome::Succeeded | RunOutcome::Failed | RunOutcome::Interrupted
                ),
                RestartPolicy::OnFailure => matches!(
                    authority.outcome,
                    RunOutcome::Failed | RunOutcome::Interrupted
                ),
            })
    }
    pub fn recovery_candidates(
        &self,
        boot_epoch: Option<u64>,
        after: Option<&str>,
        limit: u32,
    ) -> Result<RecoveryPage> {
        if limit == 0 || limit > 100 {
            return Err(EngineError::Invalid(
                "recovery page limit must be 1..100".into(),
            ));
        }
        if let Some(after) = after {
            validate_name(after)?
        }
        let Some(epoch) = boot_epoch else {
            return Ok(RecoveryPage {
                items: Vec::new(),
                next_cursor: None,
                scanned: 0,
                skipped: 0,
            });
        };
        let mut query=self.connection.prepare("SELECT i.id FROM instance_automation a JOIN instances i ON i.id=a.instance_id WHERE a.desired_run=1 AND a.boot_epoch=?1 AND a.generation IS NOT NULL AND a.outcome IN ('succeeded','failed','interrupted') AND i.state IN ('created','exited','dead') AND i.endpoint_id='local' AND i.driver='wasmer-local' AND i.id>?2 AND NOT EXISTS(SELECT 1 FROM stack_services WHERE instance_id=i.id) ORDER BY i.id LIMIT ?3")?;
        let mut ids = query
            .query_map(
                params![sql(epoch)?, after.unwrap_or(""), i64::from(limit) + 1],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let more = ids.len() > limit as usize;
        ids.truncate(limit as usize);
        let next_cursor = if more { ids.last().cloned() } else { None };
        let scanned = ids.len() as u32;
        let mut items = Vec::new();
        for id in ids {
            // At most one bounded page is inspected. Private specs never leave
            // this function; actual selected specs are re-read at admission.
            let instance = self.instance(&id)?;
            let authority = self.run_authority(&id)?;
            if self.eligible_authority(&instance, &authority, boot_epoch, true)? {
                items.push(RecoveryCandidate {
                    id,
                    version: instance.version,
                    state: instance.state,
                    policy: instance.spec.restart_policy,
                    authority,
                });
            }
        }
        let page = RecoveryPage {
            skipped: scanned - items.len() as u32,
            items,
            next_cursor,
            scanned,
        };
        if serde_json::to_vec(&page)?.len() > 256 * 1024 {
            return Err(EngineError::Invalid(
                "recovery metadata page exceeds input limit".into(),
            ));
        }
        Ok(page)
    }
    /// Atomic desired-run + generation admission before runtime side effects.
    /// Missing boot identity allows a manual/boot run but never automatic resume.
    pub fn begin_authorized_run(
        &self,
        id: &str,
        instance_version: u64,
        revision: u64,
        generation: &str,
        boot_epoch: Option<u64>,
        source: RunStartSource,
    ) -> Result<RunAuthority> {
        validate_name(generation)?;
        boot_epoch.map(sql).transpose()?;
        let tx = self.connection.unchecked_transaction()?;
        let instance = self.instance(id)?;
        let mut authority = self.run_authority(id)?;
        if instance.version != instance_version
            || authority.revision != revision
            || !matches!(
                instance.state,
                InstanceState::Created | InstanceState::Exited
            )
            || active(authority.outcome)
        {
            return Err(EngineError::Conflict(
                "stale or active run authority".into(),
            ));
        }
        if instance.spec.endpoint_id != "local" || instance.driver != "wasmer-local" {
            return Err(EngineError::Unsupported(
                "automatic authority requires the local Wasmer driver".into(),
            ));
        }
        if self.stack_for_instance(id)?.is_some() {
            return Err(EngineError::Unsupported(
                "managed restart authority requires group policy".into(),
            ));
        }
        match source {
            RunStartSource::Manual => {}
            RunStartSource::Boot => {
                if !instance.spec.start_on_boot {
                    return Err(EngineError::Conflict("boot opt-in is disabled".into()));
                }
            }
            RunStartSource::Restart | RunStartSource::Recovery => {
                if !self.eligible_authority(
                    &instance,
                    &authority,
                    boot_epoch,
                    source == RunStartSource::Recovery,
                )? {
                    return Err(EngineError::Conflict(
                        "automatic run authority changed or boot identity unavailable".into(),
                    ));
                }
            }
        }
        authority.revision = next(authority.revision)?;
        authority.desired_run = true;
        authority.generation = Some(generation.into());
        authority.boot_epoch = boot_epoch;
        authority.outcome = RunOutcome::Starting;
        self.write_authority(&tx, &authority)?;
        event(
            &tx,
            Some(id),
            kind_name(instance.kind),
            &instance.driver,
            serde_json::json!({"type":"instance_run_admitted","revision":authority.revision}),
        )?;
        tx.commit()?;
        Ok(authority)
    }
    pub(super) fn guard_automation_transition(
        &self,
        id: &str,
        generation: Option<&str>,
    ) -> Result<RunAuthority> {
        let authority = self.run_authority(id)?;
        if generation.is_some()
            && (authority.generation.as_deref() != generation || !active(authority.outcome))
            || generation.is_none() && active(authority.outcome)
        {
            return Err(EngineError::Conflict(
                "stale or missing run generation".into(),
            ));
        }
        Ok(authority)
    }
    pub(super) fn record_automation_transition(
        &self,
        tx: &Connection,
        mut authority: RunAuthority,
        next_state: InstanceState,
        exit: Option<i32>,
        error: Option<&str>,
    ) -> Result<()> {
        if authority.revision == 0 {
            return Ok(());
        }
        authority.outcome = match next_state {
            InstanceState::Running => RunOutcome::Running,
            InstanceState::Exited | InstanceState::Dead => {
                if !authority.desired_run {
                    RunOutcome::Stopped
                } else if next_state == InstanceState::Dead {
                    RunOutcome::Interrupted
                } else if exit == Some(0) && error.is_none() {
                    RunOutcome::Succeeded
                } else {
                    RunOutcome::Failed
                }
            }
            _ => return Ok(()),
        };
        self.close_managed_desire(&mut authority, next_state)?;
        authority.revision = next(authority.revision)?;
        self.write_authority(tx, &authority)
    }
    pub(super) fn copy_run_authority(&self, tx: &Connection, old: &str, new: &str) -> Result<()> {
        let mut authority = self.run_authority(old)?;
        if authority.revision != 0 {
            authority.instance_id = new.into();
            authority.revision = next(authority.revision)?;
            self.write_authority(tx, &authority)?
        }
        Ok(())
    }
    pub(super) fn clear_run_intent_for_edit(&self, tx: &Connection, id: &str) -> Result<()> {
        let mut authority = self.run_authority(id)?;
        if active(authority.outcome) {
            return Err(EngineError::Conflict(
                "active run authority still owns the instance".into(),
            ));
        }
        if authority.desired_run {
            let instance = self.instance(id)?;
            authority.desired_run = false;
            authority.outcome = RunOutcome::Stopped;
            authority.revision = next(authority.revision)?;
            self.write_authority(tx, &authority)?;
            event(
                tx,
                Some(id),
                kind_name(instance.kind),
                &instance.driver,
                serde_json::json!({"type":"instance_run_intent","desired_run":false,"revision":authority.revision}),
            )?;
        }
        Ok(())
    }
    pub fn ready_authorized_run(
        &self,
        id: &str,
        version: u64,
        generation: &str,
        ports: &[crate::api::PortBinding],
    ) -> Result<super::InstanceRecord> {
        let current = self.instance(id)?;
        let authority = self.run_authority(id)?;
        let keys = |ports: &[crate::api::PortBinding]| {
            let mut keys: Vec<_> = ports
                .iter()
                .map(|port| (port.host_port, port.instance_port, port.protocol as u8))
                .collect();
            keys.sort_unstable();
            keys
        };
        if version.checked_add(1) == Some(current.version)
            && current.state == InstanceState::Running
            && authority.generation.as_deref() == Some(generation)
            && authority.outcome == RunOutcome::Running
            && keys(&self.instance_ports(id)?) == keys(ports)
        {
            return Ok(current);
        }
        self.transition_with_ports(
            id,
            version,
            InstanceState::Running,
            None,
            None,
            (Some(ports), Some(generation)),
        )
    }
    /// Host must first observe actual worker death. Generation validation,
    /// terminal instance/event and automation outcome commit together.
    pub fn finish_authorized_run(
        &self,
        id: &str,
        version: u64,
        generation: &str,
        exit: Option<i32>,
        reason: Option<&str>,
        owner_stopped: bool,
    ) -> Result<super::InstanceRecord> {
        let prior = self.instance(id)?;
        let authority = self.run_authority(id)?;
        let same_generation = authority.generation.as_deref() == Some(generation);
        // A READY transaction may have committed while its acknowledgement was
        // lost. Only that same active generation may advance the stale version.
        let lost_ready = same_generation
            && prior.state == InstanceState::Running
            && authority.outcome == RunOutcome::Running
            && version.checked_add(1) == Some(prior.version);
        if (version.checked_add(1) == Some(prior.version)
            || version.checked_add(2) == Some(prior.version))
            && matches!(prior.state, InstanceState::Exited | InstanceState::Dead)
            && same_generation
            && !active(authority.outcome)
        {
            return Ok(prior);
        }
        let state = if exit.is_some() || owner_stopped && prior.state != InstanceState::Created {
            InstanceState::Exited
        } else {
            InstanceState::Dead
        };
        let reason = if owner_stopped && exit.is_none() {
            Some("Stopped by owner after observed worker death; guest exit code unknown")
        } else {
            reason
        };
        self.transition_with_ports(
            id,
            if lost_ready { prior.version } else { version },
            state,
            exit,
            reason,
            (None, Some(generation)),
        )
    }
}

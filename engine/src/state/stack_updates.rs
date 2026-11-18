//! Atomic stopped-stack reconciliation. Dataset bytes are never modified here.
use super::lifecycle::{event, kind_name};
use super::*;
use crate::{
    api::MountSource,
    stacks::{ServiceChange, StackPlan, StackRecord, StackUpdatePreview},
};

impl StateDb {
    pub(crate) fn stack_current_volume_names(&self, id: &str) -> Result<Vec<String>> {
        let mut query=self.connection.prepare("SELECT volume_name FROM stack_volumes WHERE stack_id=?1 UNION SELECT volume_name FROM instance_volumes JOIN stack_services ON instance_volumes.instance_id=stack_services.instance_id WHERE stack_id=?1 ORDER BY volume_name")?;
        Ok(query
            .query_map([id], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub(crate) fn stack_preserved_volume_count(&self, id: &str) -> Result<u64> {
        Ok(self.connection.query_row("SELECT count(*) FROM (SELECT volume_name FROM stack_volumes WHERE stack_id=?1 UNION SELECT volume_name FROM stack_retained_volumes WHERE stack_id=?1 UNION SELECT volume_name FROM instance_volumes JOIN stack_services ON instance_volumes.instance_id=stack_services.instance_id WHERE stack_id=?1)",[id],|row|row.get::<_,i64>(0))? as u64)
    }
    /// Explicit private historical definition; ordinary summaries expose no spec.
    pub fn stack_initial_plan(&self, id: &str) -> Result<StackPlan> {
        validate_name(id)?;
        let raw = self
            .connection
            .query_row(
                "SELECT COALESCE(initial_plan_json,plan_json) FROM stacks WHERE id=?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| EngineError::NotFound("stack".into()))?;
        Ok(serde_json::from_str(&raw)?)
    }
    pub(crate) fn publish_stack_update(&self, preview: &StackUpdatePreview) -> Result<StackRecord> {
        let tx = self.connection.unchecked_transaction()?;
        let old = preview.check_current(self)?;
        crate::stacks::require_stopped(&old)?;
        self.require_group_network_idle(&old.id)?;
        if preview.is_noop(&old)? {
            tx.commit()?;
            return Ok(old);
        }
        let plan = &preview.plan;
        let pending = self.volume_deletions()?;
        for volume in plan.volumes.values() {
            self.volume(&volume.name)?;
            if pending.contains(&volume.name) {
                return Err(EngineError::Conflict(
                    "stack volume removal is pending".into(),
                ));
            }
        }
        // Preserve every prior declared/restored dataset as a protected recovery
        // copy, including logical names rebound to another existing dataset.
        tx.execute("INSERT OR IGNORE INTO stack_retained_volumes(stack_id,volume_name) SELECT stack_id,volume_name FROM stack_volumes WHERE stack_id=?1",[&plan.id])?;
        tx.execute("INSERT OR IGNORE INTO stack_retained_volumes(stack_id,volume_name) SELECT stack_services.stack_id,volume_name FROM instance_volumes JOIN stack_services ON instance_volumes.instance_id=stack_services.instance_id WHERE stack_id=?1",[&plan.id])?;
        event(
            &tx,
            None,
            "stack",
            "wasmer-local",
            serde_json::json!({"type":"stack_updated","stack_id":plan.id,"service_count":plan.services.len()}),
        )?;
        let version = self.connection.last_insert_rowid();
        if version <= 0 {
            return Err(EngineError::Conflict(
                "invalid stack update revision".into(),
            ));
        }
        let previous = crate::stacks::current_by_name(&old);
        tx.execute("DELETE FROM stack_services WHERE stack_id=?1", [&plan.id])?;
        tx.execute("DELETE FROM stack_volumes WHERE stack_id=?1", [&plan.id])?;
        for (position, name) in plan.startup_order.iter().enumerate() {
            let service = &plan.services[name];
            let id = match preview.changes[name] {
                ServiceChange::Add | ServiceChange::Replace => {
                    // A removed/reintroduced logical name gets a new incarnation;
                    // stale per-instance controls cannot address its old record.
                    let replacement = format!("{}-u{version}", service.instance_id);
                    self.insert_instance_row(&tx, &replacement, &service.spec)?;
                    replacement
                }
                ServiceChange::Configure | ServiceChange::Keep => {
                    let instance = previous[name.as_str()];
                    let artifact = self.artifact(&service.spec.artifact_ref)?;
                    if artifact.kind != instance.kind || artifact.driver != instance.driver {
                        return Err(EngineError::Conflict(
                            "current stack package identity changed".into(),
                        ));
                    }
                    if preview.changes[name] == ServiceChange::Configure {
                        let revision = instance
                            .version
                            .checked_add(1)
                            .and_then(|value| i64::try_from(value).ok())
                            .ok_or_else(|| {
                                EngineError::Invalid("instance revision overflow".into())
                            })?;
                        let updated=tx.execute("UPDATE instances SET spec_json=?1,version=?2 WHERE id=?3 AND version=?4",
                            params![serde_json::to_string(&service.spec)?,revision,instance.id,revision-1])?;
                        if updated != 1 {
                            return Err(EngineError::Conflict(
                                "stale stack member revision".into(),
                            ));
                        }
                        tx.execute(
                            "DELETE FROM instance_volumes WHERE instance_id=?1",
                            [&instance.id],
                        )?;
                        for mount in &service.spec.mounts {
                            if let MountSource::Volume(volume) = &mount.source {
                                tx.execute("INSERT INTO instance_volumes(instance_id,volume_name,target,read_only) VALUES (?1,?2,?3,?4)",params![instance.id,volume,mount.target,mount.read_only])?;
                            }
                        }
                        event(
                            &tx,
                            Some(&instance.id),
                            kind_name(instance.kind),
                            &instance.driver,
                            serde_json::json!({"type":"instance_configured","version":revision}),
                        )?;
                    }
                    instance.id.clone()
                }
            };
            tx.execute("INSERT INTO stack_services(stack_id,service_name,instance_id,position) VALUES (?1,?2,?3,?4)",params![plan.id,name,id,position as i64])?;
            if preview.changes[name] == ServiceChange::Replace {
                let prior = previous[name.as_str()];
                tx.execute("DELETE FROM instances WHERE id=?1", [&prior.id])?;
                event(
                    &tx,
                    Some(&prior.id),
                    kind_name(prior.kind),
                    &prior.driver,
                    serde_json::json!({"type":"instance_replaced","replacement":id}),
                )?;
            }
        }
        for name in &preview.removed {
            let prior = previous[name.as_str()];
            tx.execute("DELETE FROM instances WHERE id=?1", [&prior.id])?;
            event(
                &tx,
                Some(&prior.id),
                kind_name(prior.kind),
                &prior.driver,
                serde_json::json!({"type":"instance_removed"}),
            )?;
        }
        for (logical, volume) in &plan.volumes {
            tx.execute("INSERT INTO stack_volumes(stack_id,logical_name,volume_name,external) VALUES (?1,?2,?3,?4)",params![plan.id,logical,volume.name,volume.external])?;
        }
        let changed=tx.execute("UPDATE stacks SET initial_plan_json=COALESCE(initial_plan_json,plan_json),plan_json=?1,version=?2 WHERE id=?3 AND version=?4",
            params![serde_json::to_string(plan)?,version,plan.id,i64::try_from(preview.stack_version).map_err(|_|EngineError::Invalid("stack revision overflow".into()))?])?;
        if changed != 1 {
            return Err(EngineError::Conflict("stale stack revision".into()));
        }
        tx.commit()?;
        self.stack(&plan.id)
    }
}

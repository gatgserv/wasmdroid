use super::lifecycle::{event, kind_name};
use super::*;
use crate::{
    api::InstanceState,
    stacks::{StackPlan, StackRecord},
};

impl StateDb {
    /// Explicit metadata inspection without copying private creation specs.
    pub fn stack_summary(&self, id: &str) -> Result<serde_json::Value> {
        validate_name(id)?;
        self.connection.query_row(
            "SELECT version,(SELECT count(*) FROM stack_services WHERE stack_id=stacks.id) FROM stacks WHERE id=?1",
            [id], |row| Ok(serde_json::json!({"id":id,"version":row.get::<_,i64>(0)?,"service_count":row.get::<_,i64>(1)?})),
        ).optional()?.ok_or_else(|| EngineError::NotFound("stack".into()))
    }
    pub fn stack_members(&self, id: &str) -> Result<Vec<serde_json::Value>> {
        validate_name(id)?;
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM stacks WHERE id=?1)",
            [id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(EngineError::NotFound("stack".into()));
        }
        let mut query=self.connection.prepare("SELECT service_name,instance_id,state,instances.version FROM stack_services JOIN instances ON instance_id=instances.id WHERE stack_id=?1 ORDER BY position")?;
        Ok(query.query_map([id],|row|Ok(serde_json::json!({"service":row.get::<_,String>(0)?,"instance_id":row.get::<_,String>(1)?,"state":row.get::<_,String>(2)?,"version":row.get::<_,i64>(3)?})))?.collect::<std::result::Result<Vec<_>,_>>()?)
    }
    pub fn stack_for_instance(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT stack_id FROM stack_services WHERE instance_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }
    pub(super) fn stack_references_volume(&self, name: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM stack_volumes WHERE volume_name=?1 UNION ALL SELECT 1 FROM stack_retained_volumes WHERE volume_name=?1)",
            [name],
            |row| row.get(0),
        )?)
    }
    pub fn stack(&self, id: &str) -> Result<StackRecord> {
        validate_name(id)?;
        let raw: Option<(i64, String)> = self
            .connection
            .query_row(
                "SELECT version,plan_json FROM stacks WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (version, json) = raw.ok_or_else(|| EngineError::NotFound("stack".into()))?;
        let plan: StackPlan = serde_json::from_str(&json)?;
        if plan.id != id || version < 0 {
            return Err(EngineError::Conflict(
                "invalid stored stack identity/revision".into(),
            ));
        }
        let mut query=self.connection.prepare("SELECT service_name,instance_id FROM stack_services WHERE stack_id=?1 ORDER BY position")?;
        let members = query
            .query_map([id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if members.len() != plan.services.len()
            || members.iter().enumerate().any(|(position, (name, _id))| {
                plan.startup_order.get(position) != Some(name) || !plan.services.contains_key(name)
            })
        {
            return Err(EngineError::Conflict(
                "stack membership does not match its definition".into(),
            ));
        }
        let instances = members
            .into_iter()
            .map(|(_, id)| self.instance(&id))
            .collect::<Result<Vec<_>>>()?;
        for (position, instance) in instances.iter().enumerate() {
            if instance.spec.labels.get(crate::stacks::STACK_LABEL) != Some(&plan.id)
                || instance.spec.labels.get(crate::stacks::SERVICE_LABEL)
                    != plan.startup_order.get(position)
            {
                return Err(EngineError::Conflict(
                    "stack member ownership does not match its definition".into(),
                ));
            }
        }
        Ok(StackRecord {
            id: id.into(),
            version: version as u64,
            plan,
            instances,
        })
    }
    /// Small paged metadata only. Specs/environment are available by explicit inspect.
    pub fn stack_summaries(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Vec<serde_json::Value>> {
        if limit == 0 || limit > 100 {
            return Err(EngineError::Invalid(
                "stack page limit must be 1..100".into(),
            ));
        }
        if let Some(after) = after {
            validate_name(after)?;
        }
        let mut query=self.connection.prepare("SELECT id,version,(SELECT count(*) FROM stack_services WHERE stack_id=stacks.id) FROM stacks WHERE id>?1 ORDER BY id LIMIT ?2")?;
        Ok(query.query_map(params![after.unwrap_or(""),limit],|row|Ok(serde_json::json!({"id":row.get::<_,String>(0)?,"version":row.get::<_,i64>(1)?,"service_count":row.get::<_,i64>(2)?})))?.collect::<std::result::Result<Vec<_>,_>>()?)
    }
    /// Metadata commit is all-or-nothing. Volume bytes must be prepared/leased by
    /// StackManager first; this method never starts a worker or downloads data.
    pub(crate) fn publish_stack(&self, plan: &StackPlan) -> Result<StackRecord> {
        let tx = self.connection.unchecked_transaction()?;
        plan.validate(self)?;
        tx.execute(
            "INSERT INTO stacks(id,endpoint_id,plan_json) VALUES (?1,?2,?3)",
            params![plan.id, plan.endpoint_id, serde_json::to_string(plan)?],
        )?;
        for (position, name) in plan.startup_order.iter().enumerate() {
            let service = &plan.services[name];
            self.insert_instance_row(&tx, &service.instance_id, &service.spec)?;
            tx.execute("INSERT INTO stack_services(stack_id,service_name,instance_id,position) VALUES (?1,?2,?3,?4)",params![plan.id,name,service.instance_id,position as i64])?;
        }
        for (logical, volume) in &plan.volumes {
            if self.volume_deletions()?.contains(&volume.name) {
                return Err(EngineError::Conflict(
                    "stack volume removal is pending".into(),
                ));
            }
            tx.execute("INSERT INTO stack_volumes(stack_id,logical_name,volume_name,external) VALUES (?1,?2,?3,?4)",params![plan.id,logical,volume.name,volume.external])?;
        }
        event(
            &tx,
            None,
            "stack",
            "wasmer-local",
            serde_json::json!({"type":"stack_created","stack_id":plan.id,"service_count":plan.services.len()}),
        )?;
        // The durable event sequence also identifies this definition incarnation.
        // Removing/recreating a named stack must not make an old version valid.
        let version = self.connection.last_insert_rowid();
        if version <= 0 {
            return Err(EngineError::Conflict(
                "invalid stack creation revision".into(),
            ));
        }
        tx.execute(
            "UPDATE stacks SET version=?2 WHERE id=?1",
            params![plan.id, version],
        )?;
        tx.commit()?;
        self.stack(&plan.id)
    }
    /// The common controller must additionally exclude pending worker starts and
    /// data operations. No dataset, cache or image is removed by this operation.
    pub fn remove_stack(&self, id: &str, expected_version: u64) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        let stack = self.stack(id)?;
        if stack.version != expected_version {
            return Err(EngineError::Conflict("stale stack revision".into()));
        }
        self.require_group_network_idle(id)?;
        if stack.instances.iter().any(|instance| {
            matches!(
                instance.state,
                InstanceState::Running | InstanceState::Paused | InstanceState::Restarting
            )
        }) {
            return Err(EngineError::Conflict(
                "stop all stack services before removal".into(),
            ));
        }
        tx.execute("DELETE FROM stacks WHERE id=?1", [id])?;
        for instance in stack.instances {
            tx.execute("DELETE FROM instances WHERE id=?1", [&instance.id])?;
            event(
                &tx,
                Some(&instance.id),
                kind_name(instance.kind),
                &instance.driver,
                serde_json::json!({"type":"instance_removed"}),
            )?;
        }
        event(
            &tx,
            None,
            "stack",
            "wasmer-local",
            serde_json::json!({"type":"stack_removed","stack_id":id}),
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Explicit managed retry preserves current restored data/specs and swaps the
    /// membership to a new identity. The creation plan remains immutable history.
    pub fn retry_stack_member(
        &self,
        stack_id: &str,
        stack_version: u64,
        service: &str,
        instance_version: u64,
        new_id: &str,
    ) -> Result<InstanceRecord> {
        validate_name(new_id)?;
        let tx = self.connection.unchecked_transaction()?;
        let stack = self.stack(stack_id)?;
        if stack.version != stack_version {
            return Err(EngineError::Conflict("stale stack revision".into()));
        }
        let position = stack
            .plan
            .startup_order
            .iter()
            .position(|name| name == service)
            .ok_or_else(|| EngineError::NotFound("stack member".into()))?;
        let previous = &stack.instances[position];
        if previous.version != instance_version || previous.state != InstanceState::Dead {
            return Err(EngineError::Conflict(
                "only the current dead stack member can be replaced".into(),
            ));
        }
        self.clear_run_intent_for_edit(&tx, &previous.id)?;
        self.insert_instance_row(&tx, new_id, &previous.spec)?;
        self.move_browser_identity(&tx, &previous.id, new_id)?;
        tx.execute(
            "UPDATE stack_services SET instance_id=?3 WHERE stack_id=?1 AND service_name=?2",
            params![stack_id, service, new_id],
        )?;
        tx.execute("DELETE FROM instances WHERE id=?1", [&previous.id])?;
        event(
            &tx,
            Some(&previous.id),
            kind_name(previous.kind),
            &previous.driver,
            serde_json::json!({"type":"instance_replaced","replacement":new_id}),
        )?;
        event(
            &tx,
            None,
            "stack",
            "wasmer-local",
            serde_json::json!({"type":"stack_member_replaced","stack_id":stack_id}),
        )?;
        let version = self.connection.last_insert_rowid();
        tx.execute(
            "UPDATE stacks SET version=?2 WHERE id=?1",
            params![stack_id, version],
        )?;
        tx.commit()?;
        self.instance(new_id)
    }
}
